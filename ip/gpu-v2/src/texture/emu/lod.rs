//! LOD arithmetic and single-port registered ROM returns on the frozen calendar.
use super::derivative::{self, scalar, Calendar, Edge, Header, Registers};
pub const NUMERIC_BITS: usize = 277;
pub const CONTROL_BITS: usize = 60;
pub const SPAN: u8 = 27;
/// The exact71-bit downstream context. Diagnostic prefix/LOD/lambda values are
/// traced where produced; they are not a second persistent output row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoordinateContext {
    pub shift: i32,
    pub nearest: bool,
    pub halve: bool,
    pub side: [i16; 2],
    pub parents: [u16; 2],
    pub levels: [u8; 2],
    pub last_fine: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Input {
    pub slope: u64,
    pub bias: i16,
    pub header: Header,
}
impl From<derivative::Output> for Input {
    fn from(d: derivative::Output) -> Self {
        Self {
            slope: d.slope,
            bias: d.bias,
            header: d.header,
        }
    }
}
impl Input {
    fn rows(self) -> Vec<(&'static str, usize, i128)> {
        vec![
            ("slope", 0, i128::from(self.slope)),
            ("bias", 0, i128::from(self.bias)),
            ("meta", 0, i128::from(self.header.max_n)),
            ("meta", 1, i128::from(self.header.quad)),
            ("meta", 2, i128::from(self.header.mask)),
            ("meta", 3, i128::from(self.header.slot)),
            ("has_mip", 0, i128::from(self.header.has_mip)),
            ("filter", 0, i128::from(self.header.filter)),
        ]
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Output {
    pub context: CoordinateContext,
    pub quad: u8,
    pub mask: u8,
    pub slot: u8,
}
pub struct LodEmu {
    registers: Registers,
}
impl LodEmu {
    pub fn new(calendar: Calendar) -> Result<Self, String> {
        if calendar.numeric_bits() != NUMERIC_BITS || calendar.span() != SPAN {
            return Err("LOD allocation mismatch".into());
        }
        Ok(Self {
            registers: Registers::new(calendar),
        })
    }
    pub fn output(&self) -> Result<Option<Output>, String> {
        self.registers
            .output()?
            .map(|v| {
                Ok(Output {
                    context: CoordinateContext {
                        shift: scalar(&v, "shift0") as i32,
                        nearest: scalar(&v, "nearest") != 0,
                        halve: scalar(&v, "halve") != 0,
                        side: std::array::from_fn(|i| scalar(&v, &format!("side{i}")) as i16),
                        parents: std::array::from_fn(|i| scalar(&v, &format!("parent{i}")) as u16),
                        levels: std::array::from_fn(|i| scalar(&v, &format!("n{i}")) as u8),
                        last_fine: scalar(&v, "last_fine") != 0,
                    },
                    quad: scalar(&v, "quad") as u8,
                    mask: scalar(&v, "mask") as u8,
                    slot: scalar(&v, "slot") as u8,
                })
            })
            .transpose()
    }
    pub fn tick(&mut self, ce: bool, input: Option<Input>) -> Result<Edge, String> {
        if let Some(i) = input.filter(|_| ce) {
            if i.slope >= 1_u64 << 40
                || !(-8192..=8192).contains(&i.bias)
                || i.header.max_n > 10
                || i.header.quad >= 16
                || i.header.mask >= 16
                || i.header.slot >= 16
                || i.header.filter > 2
            {
                return Err("LOD input range".into());
            }
        }
        let rows = input.map(Input::rows);
        self.registers.tick(ce, rows.as_deref())
    }
    pub fn idle(&self) -> bool {
        self.registers.idle()
    }
    pub fn phase(&self) -> u8 {
        self.registers.phase()
    }
    pub fn bank(&self) -> &[u64] {
        self.registers.bank()
    }
}
