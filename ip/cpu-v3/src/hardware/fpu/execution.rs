//! Selected single-product calendar. Legacy standalone path models remain
//! independent references; this state follows the physical RF/compute owners.
use super::*;

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuExecRfInput {
    pub write_enable: Wire,
    pub write_address: Wires<9>,
    pub write_data: Wires<32>,
    pub read_a_address: Wires<9>,
    pub read_b_address: Wires<9>,
    pub read_b_enable: Wire,
}

pub(super) struct CpuV3FpuExecRf;
pub(super) struct CpuV3FpuExecMul;
pub(super) struct CpuV3FpuExecSpecial;

macro_rules! identity {
    ($module:ty, $name:literal) => {
        impl HardwareIdentity for $module {
            const TARGET_RESOURCE_LEAF: bool = true;
            fn verilog_identity() -> VerilogIdentity {
                VerilogIdentity::new($name).namespace(["components", "cpu", "cpu_v3"])
            }
        }
    };
}
identity!(CpuV3FpuExecRf, "CpuV3FpuExecRf");
identity!(CpuV3FpuExecMul, "CpuV3FpuExecMul");
identity!(CpuV3FpuExecSpecial, "CpuV3FpuExecSpecial");

impl Module for CpuV3FpuExecRf {
    type Input = CpuV3FpuExecRfInput;
    type Output = CpuV3FpuRegisterRamOutput;
    type EmuState = ();
    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;
    fn target_resources() -> Vec<TargetResourceRequest> {
        vec![TargetResourceRequest::new(BsramBlocks::new(2))]
    }
    fn verilog_testbench() -> Option<String> {
        Some(
            CpuV3FpuRegisterRam::verilog_testbench()?
                .replace("CpuV3FpuRegisterRam", "CpuV3FpuExecRf")
                .replace("reg clk = 0;", "reg clk = 0; reg read_b_enable = 1;"),
        )
    }
    fn verilog_source() -> Option<String> {
        // Keep exactly one canonical RF/LUT initialization source.
        let original = CpuV3FpuRegisterRam::verilog_source()?;
        let read = "read_b_data_r <= mirror_1[read_b_address];";
        assert!(original.contains(read));
        Some(
            original
                .replace("CpuV3FpuRegisterRam", "CpuV3FpuExecRf")
                .replace(
                    "input wire [8:0] read_b_address,",
                    "input wire [8:0] read_b_address,\n    input wire read_b_enable,",
                )
                .replace(read, &format!("if (read_b_enable) {read}")),
        )
    }
}
impl Module for CpuV3FpuExecMul {
    type Input = CpuV3FpuMulPipeInput;
    type Output = CpuV3FpuMulPipeOutput;
    type EmuState = ();
    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;
    fn target_resources() -> Vec<TargetResourceRequest> {
        CpuV3FpuMulPipe::target_resources()
    }
    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_exec_mul.v").to_string())
    }
    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_exec_mul_tb.v").to_string())
    }
}
impl Module for CpuV3FpuExecSpecial {
    type Input = CpuV3FpuSpecialPathInput;
    type Output = CpuV3FpuSpecialPathOutput;
    type EmuState = ();
    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;
    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_exec_special.v").to_string())
    }
    fn verilog_testbench() -> Option<String> {
        Some(super::execution_tb::special())
    }
}

#[derive(Clone, Default)]
pub struct CpuV3FpuState {
    frontend: CpuV3FpuFrontendState,
    pub(super) rf: CpuV3FpuRegisterRamState,
    run: bool,
    lane: u8,
    last: u8,
    read_a: u8,
    read_b: u8,
    destination: u16,
    returned: bool,
    returned_last: bool,
    returned_destination: u16,
    acc: i64,
    flags: [bool; 3],
    mul_valid: bool,
    product: i64,
    integer_owner: bool,
    phase: u8,
    special_rcp: bool,
    special_sincos: bool,
    negative: bool,
    residue: u16,
    scale: u8,
    zero: bool,
    interval_base: u32,
    interval_flags: u8,
    second_parameters: u16,
}

struct Decoded {
    subop: u8,
    fd: u16,
    base_a: u8,
    base_b: u8,
    stride: u8,
    broadcast: bool,
    multiply: bool,
    dot: bool,
    product: bool,
    cmp: bool,
    normal_load: bool,
    vector_load: bool,
    alu: u8,
}
struct Special {
    load: bool,
    busy: bool,
    sc: bool,
    rcp: bool,
    single: bool,
    read_a: usize,
    read_b: usize,
    interpolate: bool,
    mul: Option<(i64, i64)>,
    base: u32,
    flags: u8,
    next_second: u16,
    write: Option<(usize, u32)>,
    residue: u16,
    scale: u8,
    zero: bool,
    negative: bool,
}

impl CpuV3FpuState {
    pub fn architectural_f_registers(&self) -> [i32; 64] {
        std::array::from_fn(|i| self.rf.mirror_a[i] as i32)
    }
    pub fn accumulator(&self) -> i64 {
        self.acc
    }

    fn decoded(&self, input: &CpuV3FpuInputValue) -> Decoded {
        let word0 = self.frontend.word0_raw;
        let word1 = self.frontend.word1_raw;
        let scalar = self.frontend.instr_opcode == 13;
        let vector = self.frontend.instr_opcode == 12;
        let subop = if vector {
            encoding::vector_subop(word1)
        } else {
            encoding::scalar_subop(word1)
        };
        let multiply = (scalar && subop == 2) || (vector && matches!(subop, 2 | 3));
        let dot = vector && matches!(subop, 13..=15);
        let vector_owned = matches!(
            subop,
            encoding::VADD
                | encoding::VSUB
                | encoding::VMUL
                | encoding::VMULS
                | encoding::VMIN
                | encoding::VMAX
                | encoding::VABS
                | encoding::VNEG
                | encoding::VMOV
                | encoding::DOT
                | encoding::DOTADD
                | encoding::DOTSTORE
        );
        let scalar_owned = !matches!(subop, 12..=14);
        let scalar_load = self.frontend.instr_complete && scalar && scalar_owned && !input.abort;
        let vector_load = self.frontend.instr_complete && vector && vector_owned && !input.abort;
        let alu = if vector {
            match subop {
                0 => 0,
                1 => 1,
                4 => 3,
                5 => 4,
                6 => 5,
                7 => 6,
                12 => 15,
                _ => 2,
            }
        } else {
            subop & 15
        };
        Decoded {
            subop,
            fd: u16::from(encoding::word1_fd(word1)),
            base_a: if self.frontend.instr_opcode == 14 {
                encoding::aux_fa(word0)
            } else {
                encoding::word0_fa(word0)
            },
            base_b: encoding::word0_fb(word0),
            stride: if dot {
                match word1 & 3 {
                    1 => 3,
                    2 => 4,
                    _ => 1,
                }
            } else if vector && subop == 3 {
                0
            } else {
                1
            },
            broadcast: vector && subop == 3,
            multiply,
            dot,
            product: multiply || dot,
            cmp: scalar && subop == 11,
            normal_load: scalar_load || vector_load,
            vector_load,
            alu,
        }
    }

    fn special(&self, input: &CpuV3FpuInputValue) -> Special {
        let word1 = self.frontend.word1_raw;
        let op = encoding::scalar_subop(word1);
        let scalar = self.frontend.instr_opcode == 13;
        let sc = scalar && op == 14;
        let rcp = scalar && op == 12;
        let load = self.frontend.instr_complete && scalar && matches!(op, 12..=14) && !input.abort;
        let mode = word1 & 3;
        let single = matches!(mode, 1 | 2);
        let active_sc = self.phase != 0 && self.special_sincos;
        let x = self.rf.read_a_data;
        let negative = x >> 31 != 0;
        let magnitude = if negative { x.wrapping_neg() } else { x };
        let clz = magnitude.leading_zeros();
        let window = (magnitude.wrapping_shl(clz) >> 15) & 0xffff;
        let table = if rcp || clz & 1 != 0 { 128 } else { 256 };
        let mut read_a = if load && rcp {
            table + (window >> 9) as usize
        } else {
            0
        };
        let read_b = if load && !rcp && !sc {
            table + (window >> 9) as usize
        } else {
            0
        };
        let phase = ((self.product as u64 >> 32) & 0x3ffff) as u32;
        let q = phase >> 16;
        let fraction = phase & 0xffff;
        let sin_u = if q & 1 != 0 {
            0x10000 - fraction
        } else {
            fraction
        };
        let cos_u = if q & 1 != 0 {
            fraction
        } else {
            0x10000 - fraction
        };
        let first_cos = mode == 2;
        if active_sc && (self.phase == 1 || (!single && self.phase == 2)) {
            let u = if first_cos || self.phase == 2 {
                cos_u
            } else {
                sin_u
            };
            read_a = 256 + ((u >> 8) & 255) as usize;
        }
        let second = active_sc && self.phase == 3;
        let u = if second {
            ((u32::from(self.second_parameters) & 0x100) << 8)
                | (u32::from(self.second_parameters) & 255)
        } else if first_cos {
            cos_u
        } else {
            sin_u
        };
        let sign = if second {
            (self.second_parameters >> 9) & 1
        } else if first_cos {
            ((q >> 1) ^ q) as u16 & 1
        } else {
            (q >> 1) as u16 & 1
        };
        let packed = if active_sc || self.special_rcp {
            self.rf.read_a_data
        } else {
            self.rf.read_b_data
        };
        let base = packed & 0x1ffff;
        let delta = (((packed >> 17) & 1023) as i32) << 22 >> 22;
        let interpolate = (self.phase == 1 && !self.special_sincos)
            || (active_sc && (self.phase == 2 || (!single && self.phase == 3)));
        let mul = if load && sc {
            Some((i64::from(x as i32), SINCOS_K))
        } else if interpolate {
            Some((
                i64::from(delta),
                i64::from(if active_sc {
                    u & 255
                } else {
                    u32::from(self.residue)
                }),
            ))
        } else {
            None
        };
        let flags = (((u >> 16) as u8 & 1) << 1) | sign as u8;
        let sc_return = active_sc && matches!(self.phase, 3 | 4);
        let rcp_return = self.phase == 2 && !self.special_sincos;
        let valid = self.mul_valid && !self.integer_owner && !input.abort;
        let write = if valid && (sc_return || rcp_return) {
            let correction = if sc_return {
                self.product >> 8
            } else {
                i64::from(((self.product >> 9) as i32) << 22 >> 22)
            };
            let interpolated = self.interval_base.wrapping_add(correction as u32) & 0x1ffff;
            let value = if sc_return {
                let mag = if self.interval_flags & 2 != 0 {
                    0x10000
                } else {
                    interpolated
                };
                if self.interval_flags & 1 != 0 {
                    mag.wrapping_neg()
                } else {
                    mag
                }
            } else if self.zero {
                if self.special_rcp {
                    if self.negative {
                        0x8000_0001
                    } else {
                        0x7fff_ffff
                    }
                } else {
                    0
                }
            } else {
                let scaled = (u64::from(interpolated) << 16) >> self.scale;
                let mag = if self.special_rcp && scaled >> 31 != 0 {
                    0x7fff_ffff
                } else {
                    scaled as u32
                };
                if self.special_rcp && self.negative {
                    mag.wrapping_neg()
                } else {
                    mag
                }
            };
            Some((
                ((word1 >> 10) as usize + usize::from(sc_return && self.phase == 4)) & 63,
                value,
            ))
        } else {
            None
        };
        Special {
            load,
            busy: (load || self.phase != 0) && !input.abort,
            sc,
            rcp,
            single,
            read_a,
            read_b,
            interpolate,
            mul,
            base,
            flags,
            next_second: ((((q >> 1) ^ q) & 1) << 9 | ((cos_u >> 16) & 1) << 8 | (cos_u & 255))
                as u16,
            write,
            residue: (window & 511) as u16,
            scale: if rcp {
                (!clz & 31) as u8
            } else {
                (23 - (clz >> 1)) as u8
            },
            zero: magnitude == 0 || (!rcp && negative),
            negative,
        }
    }

    pub(crate) fn comb(&self, input: &CpuV3FpuInputValue) -> CpuV3FpuOutputValue {
        let d = self.decoded(input);
        CpuV3FpuOutputValue {
            busy: (d.normal_load || self.run || self.returned || self.special(input).busy)
                && !input.abort,
            flag_lt: self.flags[0],
            flag_eq: self.flags[1],
            flag_gt: self.flags[2],
            instr_complete: self.frontend.instr_complete,
            ext_read_data: u64::from(self.rf.read_b_data),
            integer_out_valid: self.mul_valid && self.integer_owner && !input.abort,
            integer_out_product: self.product as u64 & 0xffff_ffff,
        }
    }

    pub(crate) fn tick(&mut self, input: &CpuV3FpuInputValue) {
        let d = self.decoded(input);
        let sf = self.special(input);
        let issue = (d.normal_load || self.run) && !input.abort;
        let last = self.lane == self.last;
        let destination = self.destination;
        let prepare = input.word_valid && self.frontend.waiting_word1 && !input.abort;
        let own_return = self.returned && d.product && !input.abort;
        let acc_plus = self.acc.wrapping_add(self.product);
        let a = self.rf.read_a_data;
        let b = self.rf.read_b_data;
        let write = if input.ext_access {
            input.ext_write_enable.then_some((
                input.ext_write_address as usize,
                input.ext_write_data as u32,
            ))
        } else if issue && !d.product && !d.cmp {
            Some((
                destination as usize & 63,
                CpuV3FpuScalarPathState::alu(a, b, d.alu).0,
            ))
        } else if own_return && d.multiply {
            Some((
                self.returned_destination as usize,
                (self.product >> 16) as u32,
            ))
        } else if own_return && d.dot && self.returned_last && d.subop == 15 {
            Some((d.fd as usize, (acc_plus >> 16) as u32))
        } else {
            sf.write
        };
        let ra = if sf.busy {
            sf.read_a
        } else if d.normal_load || self.run {
            usize::from(self.read_a)
        } else {
            usize::from(d.base_a)
        };
        let rb = if sf.busy {
            sf.read_b
        } else if d.normal_load || self.run {
            usize::from(self.read_b)
        } else if input.ext_access {
            input.ext_read_address as usize
        } else {
            usize::from(d.base_b)
        };
        let next_a = self.rf.mirror_a[ra];
        let next_b = if d.broadcast && (d.vector_load || self.run) {
            self.rf.read_b_data
        } else {
            self.rf.mirror_b[rb]
        };
        let mul = if input.integer_in_valid {
            Some((input.integer_in_a as i64, input.integer_in_b as i64))
        } else if sf.mul.is_some() {
            sf.mul
        } else if issue && d.product {
            Some((i64::from(a as i32), i64::from(b as i32)))
        } else {
            None
        };
        if input.abort {
            self.run = false;
            self.returned = false;
            self.phase = 0;
        } else {
            if own_return && d.dot {
                self.acc = if self.returned_last && d.subop == 15 {
                    0
                } else {
                    acc_plus
                };
            }
            if d.normal_load && d.dot && d.subop != 14 {
                self.acc = 0;
            }
            self.returned = issue && d.product;
            if issue {
                self.returned_destination = destination;
                self.returned_last = last;
                if d.cmp {
                    self.flags = [(a as i32) < (b as i32), a == b, (a as i32) > (b as i32)];
                }
            }
            if prepare {
                let word = input.word as u16;
                let op = encoding::vector_subop(word);
                let vector = self.frontend.instr_opcode == 12;
                let stride = if vector && matches!(op, 13..=15) {
                    match word & 3 {
                        1 => 3,
                        2 => 4,
                        _ => 1,
                    }
                } else if vector && op == 3 {
                    0
                } else {
                    1
                };
                self.lane = 0;
                self.last = if vector {
                    encoding::decoded_last_lane(encoding::word1_len(word))
                } else {
                    0
                };
                self.destination = u16::from(encoding::word1_fd(word));
                self.read_a = d.base_a.wrapping_add(1) & 63;
                self.read_b = d.base_b.wrapping_add(stride) & 63;
            } else if issue {
                self.run = !last;
                self.lane = self.lane.wrapping_add(1) & 3;
                self.read_a = self.read_a.wrapping_add(1) & 63;
                self.read_b = self.read_b.wrapping_add(d.stride) & 63;
                if !d.dot {
                    self.destination = (self.destination + 1) & 127;
                }
            }
            if sf.interpolate {
                self.interval_base = sf.base;
                self.interval_flags = sf.flags;
            }
            if sf.load {
                self.phase = 1;
                self.special_rcp = sf.rcp;
                self.special_sincos = sf.sc;
                if !sf.sc {
                    self.negative = sf.negative;
                    self.residue = sf.residue;
                    self.scale = sf.scale;
                    self.zero = sf.zero;
                }
            } else if self.phase != 0 {
                if self.special_sincos && self.phase == 2 {
                    self.second_parameters = sf.next_second;
                }
                self.phase = if (!self.special_sincos && self.phase == 2)
                    || (self.special_sincos && ((sf.single && self.phase == 3) || self.phase == 4))
                {
                    0
                } else {
                    self.phase + 1
                };
            }
        }
        self.mul_valid = mul.is_some() && !input.abort;
        if let Some((ma, mb)) = mul {
            self.integer_owner = input.integer_in_valid;
            if !input.abort {
                self.product = ma.wrapping_mul(mb);
            }
        }
        let word = input.word as u16;
        let word0 = input.word_valid && !self.frontend.waiting_word1;
        let word1 = input.word_valid && self.frontend.waiting_word1 && !input.abort;
        self.frontend.instr_complete = word1;
        if word0 {
            self.frontend.word0_raw = word;
            self.frontend.instr_opcode = encoding::opcode(word);
            self.frontend.waiting_word1 = true;
        } else if word1 || input.abort {
            self.frontend.waiting_word1 = false;
        }
        if word1 {
            self.frontend.word1_raw = word;
        }
        self.rf.read_a_data = next_a;
        self.rf.read_b_data = next_b;
        if let Some((address, value)) = write {
            self.rf.mirror_a[address] = value;
            self.rf.mirror_b[address] = value;
        }
    }
}
