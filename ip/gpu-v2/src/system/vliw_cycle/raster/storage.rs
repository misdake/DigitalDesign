//! Physical RF, mailbox, dual-read step table and the two-edge origin DSP.
use super::super::{compute::math::signed, require, vertex::Bits, ContractError};
#[derive(Clone, Copy, Debug, Default)]
pub struct RfPorts {
    pub reset: bool,
    pub ce: bool,
    pub read: bool,
    pub write: bool,
    pub ra: u8,
    pub wa: u8,
    pub data: Bits,
}
#[derive(Clone, Debug)]
pub struct Rf<const N: usize> {
    pub cells: [Bits; N],
    pub q: Bits,
    pub width: u32,
}
impl<const N: usize> Rf<N> {
    pub fn new(width: u32) -> Self {
        Self {
            cells: [Bits::default(); N],
            q: Bits::new(0, width),
            width,
        }
    }
    pub fn clock(&mut self, p: RfPorts) -> Result<(), ContractError> {
        if p.reset {
            self.q = Bits::new(0, self.width);
            return Ok(());
        }
        if !p.ce {
            return Ok(());
        }
        require(
            !(p.read && p.write && p.ra == p.wa),
            "raster_rf",
            "independent RF/mailbox cross-port address collision",
        )?;
        if p.read {
            self.q = self.cells[usize::from(p.ra) & (N - 1)];
        }
        if p.write {
            self.cells[usize::from(p.wa) & (N - 1)] = p.data.slice(0, self.width);
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct StepPorts {
    pub reset: bool,
    pub ce: bool,
    pub w_read: bool,
    pub w_address: u8,
    pub write: bool,
    pub address: u8,
    pub data: Bits,
    pub n_write: bool,
    pub n_axis: bool,
    pub n_edge: u8,
    pub n_data: Bits,
    pub s_read: bool,
    pub s_address: u8,
}
#[derive(Clone, Debug)]
pub struct Steps {
    pub cells: [Bits; 256],
    pub wq: Bits,
    pub sq: Bits,
}
impl Default for Steps {
    fn default() -> Self {
        Self {
            cells: [Bits::default(); 256],
            wq: Bits::new(0, 36),
            sq: Bits::new(0, 30),
        }
    }
}
impl Steps {
    pub fn clock(&mut self, p: StepPorts) -> Result<(), ContractError> {
        if p.reset {
            self.wq = Bits::new(0, 36);
            self.sq = Bits::new(0, 30);
            return Ok(());
        }
        if !p.ce {
            return Ok(());
        }
        let check = |ok, rule| require(ok, "raster_steps", rule);
        check(
            !((p.write || p.n_write) && p.s_read),
            "step preparation write overlaps scanner read",
        )?;
        check(
            !(p.write && p.w_read && p.address == p.w_address),
            "step cross-port address collision",
        )?;
        check(
            !(p.n_write && (p.w_read || p.s_read)),
            "step import overlaps runtime reads",
        )?;
        if p.w_read {
            self.wq = self.cells[p.w_address as usize];
        }
        if p.s_read {
            self.sq = self.cells[p.s_address as usize].slice(0, 30);
        }
        if p.write {
            self.cells[p.address as usize] = Bits::concat(Bits::new(0, 6), p.data.slice(0, 30), 30);
        }
        if p.n_write {
            self.cells[usize::from(((p.n_edge & 3) << 6) | 62)].set(
                u32::from(p.n_axis) * 18,
                18,
                p.n_data,
            );
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct Origin {
    pub n: [Bits; 2],
    pub xy: [u32; 2],
    pub h: Bits,
    pub result: Bits,
}
impl Default for Origin {
    fn default() -> Self {
        Self {
            n: [Bits::new(0, 18); 2],
            xy: [0; 2],
            h: Bits::new(0, 54),
            result: Bits::new(0, 54),
        }
    }
}
impl Origin {
    pub fn clock(&mut self, reset: bool, ce: bool, n: [Bits; 2], xy: [u32; 2], h: Bits) {
        if reset {
            *self = Self::default();
            return;
        }
        if !ce {
            return;
        }
        self.result = if self.n.iter().all(|v| v.is_known(18)) && self.h.is_known(54) {
            Bits::new(
                (i128::from(signed(self.n[0].value, 18)) * i128::from(self.xy[0])
                    + i128::from(signed(self.n[1].value, 18)) * i128::from(self.xy[1])
                    + i128::from(signed(self.h.value, 54))) as u64,
                54,
            )
        } else {
            Bits::default()
        };
        self.n = n;
        self.xy = xy.map(|v| v & 0x3ffff);
        self.h = h;
    }
}
