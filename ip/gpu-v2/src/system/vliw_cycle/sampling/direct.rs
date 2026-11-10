//! Two-axis source-Q coordinate kernel and six asynchronous owner/result rows.
use super::super::{require, vertex::Bits, ContractError};
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub ce: bool,
    pub commit_allow: bool,
    pub valid: bool,
    pub uv: u32,
    pub quad: u8,
    pub lane: u8,
    pub mask: u8,
    pub lod: u8,
    pub max_n: u8,
    pub mip: bool,
    pub filter: u8,
    pub framebuffer: bool,
    pub clamp: bool,
    pub out_ready: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub ready: bool,
    pub advance: bool,
    pub valid: bool,
    pub owner: Bits,
    pub result: Bits,
    pub issue: bool,
    pub returned: bool,
    pub occupied: u8,
    pub completed: u8,
    pub phase: u8,
    pub write_row: u8,
    pub return_row: u8,
    pub read_row: u8,
    pub coordinate_valid: u8,
    pub coordinate_result: Bits,
}
#[derive(Clone, Debug)]
pub struct Direct {
    pub owners: [Bits; 6],
    pub results: [Bits; 6],
    pub result_bank: Bits,
    pub axis_u: bool,
    pub valid: u8,
    pub write: u8,
    pub returned: u8,
    pub read: u8,
    pub occupied: u8,
    pub completed: u8,
    source_guard: Option<(u16, u8, bool, bool, bool)>,
}
impl Default for Direct {
    fn default() -> Self {
        Self {
            owners: [Bits::default(); 6],
            results: [Bits::default(); 6],
            result_bank: Bits::default(),
            axis_u: true,
            valid: 0,
            write: 0,
            returned: 0,
            read: 0,
            occupied: 0,
            completed: 0,
            source_guard: None,
        }
    }
}
fn unpack_owner(c: Bits) -> Bits {
    let lambda = c.slice(0, 9);
    let n = c.slice(10, 4);
    let n1 = if n.is_known(4) {
        Bits::new(n.value.saturating_sub(1), 4)
    } else {
        Bits::default()
    };
    let last_fine = if lambda.is_known(9) {
        Bits::new(u64::from(lambda.value == 0), 1)
    } else {
        Bits::default()
    };
    let mut result = Bits::default();
    result.set(
        0,
        9,
        Bits {
            value: !lambda.value & 511,
            known: lambda.known,
        },
    );
    result.set(9, 9, lambda);
    result.set(18, 1, c.slice(9, 1));
    result.set(19, 4, n);
    result.set(23, 4, n1);
    result.set(27, 4, c.slice(14, 4));
    result.set(31, 6, c.slice(18, 6));
    result.set(37, 1, last_fine);
    result
}
fn context(i: Input) -> (u8, u16) {
    let distance = i.lod >> 3;
    let coarsest = distance >= i.max_n;
    let n = if i.mip {
        if coarsest {
            0
        } else {
            i.max_n - distance
        }
    } else {
        i.max_n
    };
    let fraction = if i.mip && !coarsest { i.lod & 7 } else { 0 };
    let lambda = if i.filter == 2 {
        (u16::from(fraction) << 6) - u16::from(fraction > 4)
    } else {
        0
    };
    (n, lambda)
}
fn axis(i: Input, u: bool, physical: u8) -> u32 {
    let uv = if u { i.uv as u16 } else { (i.uv >> 16) as u16 };
    let scaled = if i.framebuffer {
        (u32::from(uv) * if u { 25 } else { 15 }) >> 4
    } else {
        (u32::from(uv) << 2) >> 10u8.wrapping_sub(physical)
    };
    let fine = scaled as i32 - if i.filter == 0 { 0 } else { 128 };
    if i.framebuffer {
        let last = if u { 399 } else { 239 };
        if fine < 0 {
            0
        } else if fine >> 8 == last {
            (last << 8) as u32
        } else {
            fine as u32 & 0x1ffff
        }
    } else if i.clamp {
        fine.clamp(0, ((1i32 << physical) - 1) * 256) as u32
    } else {
        let mask = ((1u32 << physical) - 1) & 1023;
        ((fine as u32 >> 8 & mask) << 8) | (fine as u32 & 255)
    }
}
impl Direct {
    /// RAM16SDP4's vendor INIT defaults initialize payload rows to zero; the
    /// portable register-array branch leaves unwritten payload unknown.
    pub fn native() -> Self {
        Self {
            owners: [Bits::new(0, 24); 6],
            results: [Bits::new(0, 36); 6],
            ..Default::default()
        }
    }
    pub fn outputs(&self, i: Input) -> Output {
        let ready = !i.reset && self.axis_u && self.occupied < 6;
        let advance = i.ce && !i.reset && i.commit_allow;
        Output {
            ready,
            advance,
            valid: !i.reset && self.completed != 0,
            owner: unpack_owner(self.owners[usize::from(self.read)]),
            result: self.results[usize::from(self.read)],
            issue: advance && i.valid && ready,
            returned: advance && self.valid & 4 != 0,
            occupied: self.occupied,
            completed: self.completed,
            phase: u8::from(!self.axis_u),
            write_row: self.write,
            return_row: self.returned,
            read_row: self.read,
            coordinate_valid: self.valid,
            coordinate_result: self.result_bank,
        }
    }
    pub fn clock(&mut self, i: Input) -> Result<(), ContractError> {
        let o = self.outputs(i);
        let proposed = i.ce && !i.reset && i.valid && o.ready;
        let returned = self.valid & 4 != 0;
        let take = i.out_ready && o.valid;
        let (n, lambda) = context(i);
        let physical = if i.clamp { n } else { n.max(1) };
        if !i.reset {
            require(
                !proposed || i.mask & (1 << (i.lane & 3)) != 0,
                "direct LOD",
                "GPU contract: preflight_bad",
            )?;
            require(
                !i.ce
                    || !(proposed && self.occupied == 6
                        || returned && self.occupied == self.completed
                        || take && self.completed == 0),
                "direct coordinate store",
                "GPU contract: bad",
            )?;
            if o.advance && self.valid & 2 != 0 {
                require(
                    self.source_guard
                        == Some((
                            (i.uv >> 16) as u16,
                            physical,
                            i.filter == 0,
                            i.framebuffer,
                            i.clamp,
                        )),
                    "serial coordinate",
                    "coordinate V input changed before its source-Q consumption",
                )?;
            }
        }
        if i.reset {
            self.axis_u = true;
            self.valid = 0;
            self.write = 0;
            self.returned = 0;
            self.read = 0;
            self.occupied = 0;
            self.completed = 0;
            return Ok(());
        }
        if !o.advance {
            return Ok(());
        }
        let old_result = self.result_bank;
        let result = axis(i, self.axis_u, physical);
        if o.issue {
            self.result_bank.set(0, 8, Bits::new(result.into(), 8));
            self.result_bank
                .set(16, 10, Bits::new((result >> 8).into(), 10));
            self.source_guard = Some((
                (i.uv >> 16) as u16,
                physical,
                i.filter == 0,
                i.framebuffer,
                i.clamp,
            ));
        }
        if self.valid & 2 != 0 {
            self.result_bank.set(8, 8, Bits::new(result.into(), 8));
            self.result_bank
                .set(26, 10, Bits::new((result >> 8).into(), 10));
        }
        let next = |p: u8| if p == 5 { 0 } else { p + 1 };
        if o.issue {
            let canonical = u32::from(lambda)
                | (u32::from(i.filter == 0) << 9)
                | (u32::from(n) << 10)
                | (u32::from((i.quad & 15) * 4 + (i.lane & 3)) << 18);
            self.owners[usize::from(self.write)] = Bits::new(canonical.into(), 24);
            self.write = next(self.write);
        }
        if o.returned {
            self.results[usize::from(self.returned)] = old_result;
            self.returned = next(self.returned);
        }
        if take {
            self.read = next(self.read);
        }
        self.occupied = self
            .occupied
            .wrapping_add(u8::from(o.issue))
            .wrapping_sub(u8::from(take))
            & 7;
        self.completed = self
            .completed
            .wrapping_add(u8::from(o.returned))
            .wrapping_sub(u8::from(take))
            & 7;
        self.valid = ((self.valid << 1) & 4) | (u8::from(o.issue) << 1);
        self.axis_u = !self.axis_u;
        Ok(())
    }
}
