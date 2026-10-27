//! One-owner numeric vertex machine. No counted plan or oracle participates in tick.
use super::ports::{Context, PackedVertex};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Header {
    pub base: [i32; 3],
    /// Five-bit hardware field; 31 is a terminal error. Checked contexts use 0..=30.
    pub grid_shift: u8,
}

/// Driver preflight; coefficient transport is separate from vertex execution.
#[derive(Clone, Debug)]
pub struct CheckedContext {
    pub header: Header,
    pub words: [i32; 25],
}
impl CheckedContext {
    pub fn new(context: &Context) -> Result<Self, String> {
        context.validate()?;
        Ok(Self {
            header: Header {
                base: context.base,
                grid_shift: context.grid_shift,
            },
            words: std::array::from_fn(|i| {
                if i < 16 {
                    context.mvp[i / 4][i % 4]
                } else {
                    i32::from(context.normal_matrix[(i - 16) / 3][(i - 16) % 3])
                }
            }),
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ticket {
    pub slot: bool,
    pub epoch: u32,
    /// Six-bit vertex address within the caller's private transformed store.
    pub vertex: u8,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Inputs {
    pub reset: bool,
    /// Every transfer is enabled by CE; raw valid/ready may remain asserted in CE0.
    pub ce: bool,
    pub config: Option<Header>,
    pub coefficient: Option<i32>,
    pub vertex: Option<(PackedVertex, Ticket)>,
    pub write_ready: bool,
    pub publish_ready: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outputs {
    pub config_ready: bool,
    pub coefficient_ready: bool,
    pub coefficient_index: u8,
    pub vertex_ready: bool,
    pub write_valid: bool,
    pub row_index: u8,
    pub row_data: u64,
    pub publish_valid: bool,
    pub ticket: Ticket,
    /// 0=none, 1=grid, 2=coefficient, 3=tag, 4=position, 5=clip, 6=normal.
    pub fault: u8,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Idle = 0,
    Load = 1,
    Position = 2,
    InitRead = 3,
    InitReturn = 4,
    TermRead = 5,
    TermReturn = 6,
    Multiply = 7,
    WideWait = 8,
    Accumulate = 9,
    Round = 10,
    Write = 11,
    Publish = 12,
    Fault = 13,
}

/// Arithmetic registers exposed for qualification, not an answer/provenance cache.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Registers {
    pub state: State,
    pub loaded: bool,
    pub header: Header,
    pub coefficient_index: u8,
    pub raw: u128,
    pub position: [i32; 3],
    pub row: u8,
    pub normal_row: u8,
    pub col: u8,
    pub coefficient_q: i32,
    pub coefficient: i32,
    pub wide_stage0: i64,
    pub wide_product: i64,
    pub narrow_product: i32,
    /// Signed 66-bit in RTL, represented exactly in the host.
    pub accumulator: i128,
    pub normal_pack: u64,
    pub row_data: u64,
    pub ticket: Ticket,
    pub fault: u8,
}
#[derive(Clone, Debug, Default)]
pub struct VertexEmu {
    pub regs: Registers,
    memory: [i32; 32],
}

impl VertexEmu {
    pub fn outputs(&self, input: &Inputs) -> Outputs {
        let r = &self.regs;
        Outputs {
            config_ready: r.state == State::Idle,
            coefficient_ready: r.state == State::Load,
            coefficient_index: r.coefficient_index,
            vertex_ready: r.state == State::Idle && r.loaded && input.config.is_none(),
            write_valid: r.state == State::Write,
            row_index: r.row,
            row_data: r.row_data,
            publish_valid: r.state == State::Publish,
            ticket: r.ticket,
            fault: r.fault,
        }
    }

    /// Returns the pre-edge offer. Transfers require CE and absence of reset.
    pub fn tick(&mut self, input: Inputs) -> Outputs {
        let output = self.outputs(&input);
        if input.reset {
            self.regs = Registers::default();
            return output;
        }
        if !input.ce {
            return output;
        }
        let r = &mut self.regs;
        match r.state {
            State::Idle => {
                if let Some(header) = input.config {
                    if header.grid_shift > 30 {
                        fail(r, 1);
                    } else {
                        r.header = header;
                        r.loaded = false;
                        r.coefficient_index = 0;
                        r.state = State::Load;
                    }
                } else if r.loaded {
                    if let Some((vertex, ticket)) = input.vertex {
                        if vertex.bits() & 3 != 0 {
                            fail(r, 3);
                        } else {
                            r.raw = vertex.bits();
                            r.ticket = Ticket {
                                vertex: ticket.vertex & 63,
                                ..ticket
                            };
                            r.col = 0;
                            r.row = 0;
                            r.normal_row = 0;
                            r.normal_pack = 0;
                            r.state = State::Position;
                        }
                    }
                }
            }
            State::Load => {
                if let Some(word) = input.coefficient {
                    if r.coefficient_index >= 16 && word != i32::from(word as i16) {
                        fail(r, 2);
                    } else {
                        self.memory[usize::from(r.coefficient_index)] = word;
                        if r.coefficient_index == 24 {
                            r.loaded = true;
                            r.state = State::Idle;
                        } else {
                            r.coefficient_index += 1;
                        }
                    }
                }
            }
            State::Position => {
                let col = usize::from(r.col);
                let xyz = ((r.raw >> (2 + 10 * col)) & 1023) as u32;
                let grid = r.header.grid_shift;
                let high_overflow = grid >= 23 && xyz >> (32 - grid) != 0;
                let delta_low = xyz << grid;
                let sum = i64::from(r.header.base[col]) + i64::from(delta_low);
                if !high_overflow && i32::try_from(sum).is_ok() {
                    let position = sum as i32;
                    r.position[col] = position;
                    if col == 2 {
                        r.col = 0;
                        r.state = State::InitRead;
                    } else {
                        r.col += 1;
                    }
                } else {
                    fail(r, 4);
                }
            }
            State::InitRead => {
                r.coefficient_q = self.memory[usize::from(r.row) * 4 + 3];
                r.state = State::InitReturn;
            }
            State::InitReturn => {
                r.accumulator = i128::from(r.coefficient_q) << 16;
                r.col = 0;
                r.state = State::TermRead;
            }
            State::TermRead => {
                let address = if r.row < 4 {
                    r.row * 4 + r.col
                } else {
                    16 + r.normal_row * 3 + r.col
                };
                r.coefficient_q = self.memory[usize::from(address)];
                r.state = State::TermReturn;
            }
            State::TermReturn => {
                r.coefficient = r.coefficient_q;
                r.state = State::Multiply;
            }
            State::Multiply => {
                if r.row < 4 {
                    let old_stage0 = r.wide_stage0;
                    r.wide_stage0 =
                        i64::from(r.coefficient) * i64::from(r.position[usize::from(r.col)]);
                    // Both native wide registers clock together; output observes old stage0.
                    r.wide_product = old_stage0;
                    r.state = State::WideWait;
                } else {
                    let normal = ((r.raw >> (32 + 8 * usize::from(r.col))) as u8 as i8 as i16) << 7;
                    r.narrow_product = i32::from(r.coefficient as i16) * i32::from(normal);
                    r.state = State::Accumulate;
                }
            }
            State::WideWait => {
                r.wide_product = r.wide_stage0;
                r.state = State::Accumulate;
            }
            State::Accumulate => {
                let product = if r.row < 4 {
                    i128::from(r.wide_product)
                } else {
                    i128::from(r.narrow_product)
                };
                r.accumulator += product;
                assert!((-(1_i128 << 65)..(1_i128 << 65)).contains(&r.accumulator));
                if r.col == 2 {
                    r.state = State::Round;
                } else {
                    r.col += 1;
                    r.state = State::TermRead;
                }
            }
            State::Round => {
                let shift = if r.row < 4 { 16 } else { 18 };
                let floor = r.accumulator >> shift;
                let residue = r.accumulator & ((1_i128 << shift) - 1);
                let half = 1_i128 << (shift - 1);
                let rounded =
                    floor + i128::from(residue > half || (residue == half && floor & 1 != 0));
                if r.row < 4 {
                    if let Ok(value) = i32::try_from(rounded) {
                        r.row_data = u64::from(value as u32);
                        r.state = State::Write;
                    } else {
                        fail(r, 5);
                    }
                } else if !(-2048..=2047).contains(&rounded) {
                    fail(r, 6);
                } else {
                    let shift = 12 * r.normal_row;
                    r.normal_pack =
                        (r.normal_pack & !(4095 << shift)) | (((rounded as u64) & 4095) << shift);
                    if r.normal_row == 2 {
                        r.row_data = r.normal_pack;
                        r.state = State::Write;
                    } else {
                        r.normal_row += 1;
                        r.col = 0;
                        r.accumulator = 0;
                        r.state = State::TermRead;
                    }
                }
            }
            State::Write => {
                if input.write_ready {
                    match r.row {
                        0..=2 => {
                            r.row += 1;
                            r.state = State::InitRead;
                        }
                        3 => {
                            r.row = 4;
                            r.col = 0;
                            r.normal_row = 0;
                            r.accumulator = 0;
                            r.state = State::TermRead;
                        }
                        4 => {
                            r.row = 5;
                            r.row_data =
                                (((r.raw >> 56) & 4095) | (((r.raw >> 68) & 4095) << 12)) as u64;
                        }
                        5 => {
                            r.row = 6;
                            r.row_data = (r.raw >> 80) as u16 as u64;
                        }
                        6 => r.state = State::Publish,
                        _ => unreachable!(),
                    }
                }
            }
            State::Publish => {
                if input.publish_ready {
                    r.state = State::Idle;
                }
            }
            State::Fault => {}
        }
        output
    }
}
fn fail(r: &mut Registers, code: u8) {
    r.fault = code;
    r.state = State::Fault;
}
