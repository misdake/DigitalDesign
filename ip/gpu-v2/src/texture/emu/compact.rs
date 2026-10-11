//! Checked production encoding. Generic interfaces remain independent references.
use super::{coefficient, coordinate};
pub const RESULT_BITS: u32 = 36;
pub const METADATA_BITS: u32 = 36;
pub fn side_mask(n: u8) -> u16 {
    (1u16 << n.max(1)) - 1
}
pub fn coarse_fraction(base: u16, f: u8, halve: bool) -> u8 {
    if !halve {
        return f;
    }
    let high = u8::from(f & 128 == 0);
    (((base as u8 & 1) ^ high) << 7) | (high << 6) | ((f >> 1) & 63)
}
pub fn pack_result(o: coordinate::Output) -> u64 {
    u64::from(o.fractions[0][0])
        | u64::from(o.fractions[0][1]) << 8
        | u64::from(o.coordinates[0][0]) << 16
        | u64::from(o.coordinates[0][2]) << 26
}
pub fn expand_result(raw: u64, n: u8) -> coordinate::Output {
    let f = [raw as u8, (raw >> 8) as u8];
    let b = [((raw >> 16) & 1023) as u16, ((raw >> 26) & 1023) as u16];
    let c = std::array::from_fn::<_, 2, _>(|a| {
        if n > 1 {
            ((b[a] >> 1).wrapping_sub(u16::from(b[a] & 1 == 0 && f[a] & 128 == 0)))
                & side_mask(n - 1)
        } else {
            b[a]
        }
    });
    coordinate::Output {
        fractions: [
            f,
            std::array::from_fn(|a| coarse_fraction(b[a], f[a], n > 1)),
        ],
        coordinates: [(b, side_mask(n)), (c, side_mask(n.saturating_sub(1)))]
            .map(|(b, m)| [b[0], (b[0] + 1) & m, b[1], (b[1] + 1) & m]),
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Metadata(pub u64);
impl Metadata {
    pub fn capture(i: coefficient::Input) -> Result<Self, coefficient::Fault> {
        let m = i.metadata;
        let n = m.levels[0];
        let o = coordinate::Output {
            fractions: i.fractions,
            coordinates: m.coordinates,
        };
        if n > 10
            || m.levels[1] != n.saturating_sub(1)
            || m.coordinates.iter().flatten().any(|v| *v > 1023)
            || expand_result(pack_result(o), n) != o
            || m.slot > 15
            || m.key > 63
            || m.last_fine != (i.parents[1] == 0)
        {
            return Err(coefficient::Fault::Input);
        }
        let b = [m.coordinates[0][0], m.coordinates[0][2]];
        let borrow =
            std::array::from_fn::<_, 2, _>(|a| b[a] & 1 == 0 && i.fractions[0][a] & 128 == 0);
        Ok(Self(
            u64::from(b[0])
                | u64::from(b[1]) << 10
                | u64::from(borrow[0]) << 20
                | u64::from(borrow[1]) << 21
                | u64::from(n) << 22
                | u64::from(m.slot) << 26
                | u64::from(m.key) << 30,
        ))
    }
    pub fn n(self) -> u8 {
        ((self.0 >> 22) & 15) as u8
    }
    pub fn key(self) -> u8 {
        (self.0 >> 30) as u8
    }
    pub fn slot(self) -> u8 {
        ((self.0 >> 26) & 15) as u8
    }
    pub fn base(self, plane: usize) -> [u16; 2] {
        std::array::from_fn(|a| {
            let b = ((self.0 >> (a * 10)) & 1023) as u16;
            if plane == 0 || self.n() <= 1 {
                b
            } else {
                (b >> 1).wrapping_sub(((self.0 >> (20 + a)) & 1) as u16) & side_mask(self.n() - 1)
            }
        })
    }
    pub fn expand(self, last_fine: bool) -> coefficient::Metadata {
        coefficient::Metadata {
            coordinates: std::array::from_fn(|p| {
                let b = self.base(p);
                let mask = side_mask(self.n().saturating_sub(p as u8));
                [b[0], (b[0] + 1) & mask, b[1], (b[1] + 1) & mask]
            }),
            levels: [self.n(), self.n().saturating_sub(1)],
            slot: self.slot(),
            key: self.key(),
            last_fine,
        }
    }
}
