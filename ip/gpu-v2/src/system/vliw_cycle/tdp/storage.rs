use super::super::{require, vertex::Bits, ContractError};
#[derive(Clone, Copy, Debug, Default)]
pub struct Port {
    pub en: bool,
    pub we: bool,
    pub address: u16,
    pub data: Bits,
}
/// Fixed 1024x16 physical bank; each port has its own held read-before-write Q.
#[derive(Clone, Debug)]
pub struct Bank {
    pub cells: [Bits; 1024],
    pub aq: Bits,
    pub bq: Bits,
}
impl Default for Bank {
    fn default() -> Self {
        Self {
            cells: [Bits::default(); 1024],
            aq: Bits::new(0, 16),
            bq: Bits::new(0, 16),
        }
    }
}
impl Bank {
    pub fn clock(&mut self, reset: bool, a: Port, b: Port) -> Result<(), ContractError> {
        if reset {
            self.aq = Bits::new(0, 16);
            self.bq = Bits::new(0, 16);
            return Ok(());
        }
        require(
            !(a.en && b.en && (a.we || b.we) && (a.address & 1023) == (b.address & 1023)),
            "tdp_bank",
            "framebuffer TDP cross-port collision",
        )?;
        if a.en && !a.we {
            self.aq = self.cells[usize::from(a.address & 1023)];
        }
        if b.en && !b.we {
            self.bq = self.cells[usize::from(b.address & 1023)];
        }
        if a.en && a.we {
            self.cells[usize::from(a.address & 1023)] = a.data.slice(0, 16);
        }
        if b.en && b.we {
            self.cells[usize::from(b.address & 1023)] = b.data.slice(0, 16);
        }
        Ok(())
    }
}
