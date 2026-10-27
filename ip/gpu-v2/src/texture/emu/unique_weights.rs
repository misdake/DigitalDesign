//! Private Public16 immutable weights and the actual one-DO/one-result calendar.
//! The generic Group72 cache emulator does not acquire these closed premises.
use super::cache::tap_map;
use crate::texture::ports::Group4;

pub fn member_address(member: u128) -> usize {
    ((((member >> 68) & 15) << 3) | (((member >> 72) & 3) << 1) | ((member >> 64) & 1)) as usize
}
pub fn group_address(group: u64) -> usize {
    ((((group >> 30) & 15) << 3) | (((group >> 34) & 3) << 1) | ((group >> 4) & 1)) as usize
}
pub fn restore(group: u64, weights: u64) -> Result<u128, String> {
    if group >> 40 != 0 || weights >> 36 != 0 {
        return Err("closed weight codec width".into());
    }
    let mut w = 0u128;
    for tap in 0..4 {
        if group & (1 << (36 + tap)) != 0 {
            w |= u128::from((weights >> (tap * 9)) & 511) << (28 + tap * 9);
        }
    }
    Ok((u128::from(group & ((1 << 28) - 1))) | w | (u128::from((group >> 28) & 255) << 64))
}
pub fn physical(group: u64, weights: u64) -> Result<u128, String> {
    let mut value = Group4::unpack72(restore(group, weights)? as i128)?;
    let (banks, _) = tap_map(value.top_left_local);
    let canonical = value.coefficients;
    for tap in 0..4 {
        value.coefficients[banks[tap] as usize] = canonical[tap];
    }
    value.pack72()
}
/// Independent geometric tile partition, not the RTL's abbreviated capture layout.
pub fn packet40(member: u128, tap: usize) -> Result<u64, String> {
    if member >> 76 != 0 || tap > 3 || (member >> (36 + tap)) & 1 == 0 {
        return Err("private packet member/tap".into());
    }
    let level = ((member >> 64) & 15) as u8;
    if level > 10 {
        return Err("private packet level".into());
    }
    let x = ((member >> 54) & 7) as u8;
    let y = ((member >> 57) & 7) as u8;
    let sx = level <= 3 || x != 7;
    let sy = level <= 3 || y != 7;
    let tx = ((member >> 40) & 127) as u8;
    let ty = ((member >> 47) & 127) as u8;
    let chosen_x = (tap & 1) != 0;
    let chosen_y = (tap & 2) != 0;
    let mut mask = 0u64;
    for i in 0..4 {
        if (sx || (i & 1 != 0) == chosen_x) && (sy || (i & 2 != 0) == chosen_y) {
            mask |= 1 << i;
        }
    }
    let tilemask = if level <= 3 {
        0
    } else {
        (1u16 << (level - 3)) - 1
    };
    let group = Group4 {
        key: crate::texture::ports::TileKey {
            slot: ((member >> 60) & 15) as u8,
            n: level,
            x: ((u16::from(tx) + u16::from(chosen_x && !sx)) & tilemask) as u8,
            y: ((u16::from(ty) + u16::from(chosen_y && !sy)) & tilemask) as u8,
        },
        top_left_local: [x, y],
        coefficients: [0; 4],
        quad_id: ((member >> 68) & 15) as u8,
        lane: ((member >> 72) & 3) as u8,
        first: member & (1 << 74) != 0 && tap == 0,
        last: member & (1 << 75) != 0 && (member >> (37 + tap)) & ((1 << (3 - tap)) - 1) == 0,
    };
    let packed = group.pack72()?;
    Ok((packed as u64 & ((1 << 28) - 1)) | ((((packed >> 64) & 255) as u64) << 28) | (mask << 36))
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Event {
    pub reset: bool,
    pub publish: Option<u8>,
    pub free: Option<u8>,
    pub write: Option<(usize, u64)>,
    pub read: Option<u64>,
    pub pop: bool,
}
pub struct Store {
    rows: [u64; 128],
    written: [bool; 128],
    live: u16,
    pub paid: Option<u64>,
    pub result: Option<(u64, u64)>,
    pub data_out: u64,
    pub writes: u64,
    pub reads: u64,
    pub captures: u64,
}
impl Default for Store {
    fn default() -> Self {
        Self {
            rows: [0; 128],
            written: [false; 128],
            live: 0,
            paid: None,
            result: None,
            data_out: 0,
            writes: 0,
            reads: 0,
            captures: 0,
        }
    }
}
impl Store {
    pub fn output(&self) -> Result<Option<u128>, String> {
        self.result.map(|(g, w)| physical(g, w)).transpose()
    }
    pub fn tick(&mut self, e: Event) -> Result<bool, String> {
        if e.reset {
            *self = Self::default();
            return Ok(false);
        }
        let capture = self.paid.is_some() && (self.result.is_none() || e.pop);
        if e.pop && self.result.is_none() {
            return Err("weight result pop without owner".into());
        }
        if e.read.is_some() && self.paid.is_some() && !capture {
            return Err("overwrite occupied paid DO".into());
        }
        if let Some((a, w)) = e.write {
            if a >= 128 || w >> 36 != 0 || self.live & (1 << (a / 8)) == 0 || self.written[a] {
                return Err("weight write alias/unpublished".into());
            }
            if e.read.is_some_and(|g| group_address(g) == a) {
                return Err("same-address read/write".into());
            }
        }
        if let Some(g) = e.read {
            let a = group_address(g);
            if self.live & (1 << (a / 8)) == 0 || !self.written[a] {
                return Err("read before immutable weight write".into());
            }
        }
        if let Some(q) = e.publish {
            if q >= 16 || self.live & (1 << q) != 0 {
                return Err("old-state public credit".into());
            }
        }
        if let Some(q) = e.free {
            if q >= 16 || self.live & (1 << q) == 0 {
                return Err("release nonlive public".into());
            }
            if self
                .paid
                .is_some_and(|g| group_address(g) / 8 == usize::from(q))
                || self
                    .result
                    .is_some_and(|(g, _)| group_address(g) / 8 == usize::from(q))
                || e.read
                    .is_some_and(|g| group_address(g) / 8 == usize::from(q))
            {
                return Err("public release before cache weight consumption".into());
            }
        }
        if e.pop {
            self.result = None;
        }
        if capture {
            self.result = Some((self.paid.take().unwrap(), self.data_out));
            self.captures += 1;
        }
        if let Some(g) = e.read {
            self.data_out = self.rows[group_address(g)];
            self.paid = Some(g);
            self.reads += 1;
        }
        if let Some((a, w)) = e.write {
            self.rows[a] = w;
            self.written[a] = true;
            self.writes += 1;
        }
        if let Some(q) = e.free {
            self.live &= !(1 << q);
        }
        if let Some(q) = e.publish {
            self.live |= 1 << q;
            self.written[usize::from(q) * 8..usize::from(q) * 8 + 8].fill(false);
        }
        Ok(capture)
    }
}

/// Compact Group34 stores geometric chosen tile XY, not coefficient-derived mask.
pub fn packet34(member: u128, tap: usize) -> Result<u64, String> {
    let g = packet40(member, tap)?;
    Ok(((tap as u64) << 32) | (((g >> 28) & 255) << 24) | ((g >> 4) & 0xffffff))
}
pub fn decode34(g: u64, slot: u8) -> u64 {
    let sx = (g & 15) <= 3 || ((g >> 18) & 7) != 7;
    let sy = (g & 15) <= 3 || ((g >> 21) & 7) != 7;
    let x = g & (1 << 32) != 0;
    let y = g & (1 << 33) != 0;
    let mask = u64::from((!x || sx) && (!y || sy))
        | (u64::from((x || sx) && (!y || sy)) << 1)
        | (u64::from((!x || sx) && (y || sy)) << 2)
        | (u64::from((x || sx) && (y || sy)) << 3);
    (mask << 36) | ((g & 0xffffffff) << 4) | u64::from(slot & 15)
}
