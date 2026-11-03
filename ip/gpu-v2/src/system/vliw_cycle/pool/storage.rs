use super::super::{require, vertex::Bits, ContractError};
#[derive(Clone, Debug)]
pub struct Payload {
    pub cells: [Bits; 512],
    pub q: Bits,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Ports {
    pub reset: bool,
    pub we: bool,
    pub mask: u8,
    pub wa: u16,
    pub data: Bits,
    pub re: bool,
    pub ra: u16,
}
impl Default for Payload {
    fn default() -> Self {
        Self {
            cells: [Bits::default(); 512],
            q: Bits::new(0, 36),
        }
    }
}
impl Payload {
    pub fn clock(&mut self, p: Ports) -> Result<(), ContractError> {
        if p.reset {
            self.q = Bits::new(0, 36);
            return Ok(());
        }
        require(
            !(p.we && p.re && p.wa == p.ra),
            "pool_ram",
            "pool physical bank collision",
        )?;
        if p.re {
            self.q = self.cells[usize::from(p.ra & 511)];
        }
        if p.we {
            for column in 0..4 {
                if p.mask & (1 << column) != 0 {
                    self.cells[usize::from(p.wa & 511)].set(
                        column * 9,
                        9,
                        p.data.slice(column * 9, 9),
                    );
                }
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct Depth {
    pub cells: [Bits; 128],
    pub q: Bits,
}
impl Default for Depth {
    fn default() -> Self {
        Self {
            cells: [Bits::default(); 128],
            q: Bits::new(0, 24),
        }
    }
}
impl Depth {
    pub fn clock(
        &mut self,
        reset: bool,
        we: bool,
        wa: u8,
        data: Bits,
        re: bool,
        ra: u8,
    ) -> Result<(), ContractError> {
        if reset {
            self.q = Bits::new(0, 24);
            return Ok(());
        }
        require(
            !(we && re && wa == ra),
            "pool_depth",
            "pool depth bank collision",
        )?;
        if re {
            self.q = self.cells[usize::from(ra & 127)];
        }
        if we {
            self.cells[usize::from(wa & 127)] = data.slice(0, 24);
        }
        Ok(())
    }
}
