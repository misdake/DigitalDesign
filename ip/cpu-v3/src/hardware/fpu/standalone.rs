//! Independent standalone path implementations and qualification peers.
use super::*;

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuScalarPathInput {
    pub alu_result: Wires<32>,
    pub alu_lt: Wire,
    pub alu_eq: Wire,
    pub alu_gt: Wire,
    pub abort: Wire,
    pub instr_complete: Wire,
    pub instr_opcode: Wires<4>,
    pub word1_raw: Wires<16>,
    pub rf_read_a_data: Wires<32>,
    pub rf_read_b_data: Wires<32>,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuScalarPathOutput {
    pub alu_request_op: Wires<4>,
    pub rf_write_enable: Wire,
    pub rf_write_address: Wires<9>,
    pub rf_write_data: Wires<32>,
    pub flag_lt: Wire,
    pub flag_eq: Wire,
    pub flag_gt: Wire,
    pub busy: Wire,
    pub r_wait: Wires<4>,
    pub w_wait: Wires<4>,
    pub x_wait: Wires<4>,
}

/// FPU v2 scalar execution-path controller: on instr_complete (opcode 0xD)
/// it requests the unit-wide combinational ALU on the already-valid RF read
/// data (T0), captures the supplied result, writes back on T1, and drives the
/// section-16 R/W/X countdown skeleton. CMP only updates the flag registers;
/// abort cancels any in-flight write.
pub struct CpuV3FpuScalarPath;

impl HardwareIdentity for CpuV3FpuScalarPath {
    const TARGET_RESOURCE_LEAF: bool = false;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3FpuScalarPath").namespace(["components", "cpu", "cpu_v3"])
    }
}

#[derive(Default, Clone)]
pub struct CpuV3FpuScalarPathState {
    write_enable: bool,
    write_address: u16,
    write_data: u32,
    flag_lt: bool,
    flag_eq: bool,
    flag_gt: bool,
    w_count: u8,
    x_count: u8,
}

impl Module for CpuV3FpuScalarPath {
    type Input = CpuV3FpuScalarPathInput;
    type Output = CpuV3FpuScalarPathOutput;
    type EmuState = CpuV3FpuScalarPathState;

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        Self::EmuState::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        let subop = encoding::scalar_subop(input.word1_raw as u16);
        let load_now = input.instr_complete
            && input.instr_opcode == u64::from(encoding::OPCODE_SCALAR)
            && !matches!(
                subop,
                encoding::MUL | encoding::RCP | encoding::RSQRT | encoding::SINCOS
            )
            && !input.abort;
        let w_wait = if load_now { 2 } else { state.w_count };
        let x_wait = if load_now { 2 } else { state.x_count };
        output.drive(
            circuit,
            &CpuV3FpuScalarPathOutputValue {
                alu_request_op: u64::from(subop & 0xF),
                rf_write_enable: state.write_enable && !input.abort,
                rf_write_address: u64::from(state.write_address),
                rf_write_data: u64::from(state.write_data),
                flag_lt: state.flag_lt,
                flag_eq: state.flag_eq,
                flag_gt: state.flag_gt,
                busy: w_wait != 0,
                r_wait: 0,
                w_wait: u64::from(w_wait),
                x_wait: u64::from(x_wait),
            },
        );
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        if input.abort {
            state.write_enable = false;
            state.w_count = 0;
            state.x_count = 0;
            return;
        }
        let word1 = input.word1_raw as u16;
        let subop = encoding::scalar_subop(word1);
        let fd = encoding::word1_fd(word1);
        let is_cmp = subop == encoding::CMP;
        // Other arithmetic owners never fire this scalar controller.
        let load_now = input.instr_complete
            && input.instr_opcode == u64::from(encoding::OPCODE_SCALAR)
            && !matches!(
                subop,
                encoding::MUL | encoding::RCP | encoding::RSQRT | encoding::SINCOS
            );
        state.write_enable = load_now && !is_cmp;
        if load_now {
            state.write_address = u16::from(fd);
            state.write_data = input.alu_result as u32;
            if is_cmp {
                state.flag_lt = input.alu_lt;
                state.flag_eq = input.alu_eq;
                state.flag_gt = input.alu_gt;
            }
            state.w_count = 1;
            state.x_count = 1;
        } else {
            state.w_count = state.w_count.saturating_sub(1);
            state.x_count = state.x_count.saturating_sub(1);
        }
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_scalar_path.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        // Test-only ALU definition is not a physical child/resource of this controller.
        Some(format!(
            "{}\n{}",
            include_str!("cpu_v3_fpu_scalar_path_tb.v"),
            include_str!("cpu_v3_fpu_scalar_alu.v")
        ))
    }
}

/// Shared-wide special-function path: one added beat; direct tagged return writes.
pub struct CpuV3FpuSpecialPath;

impl HardwareIdentity for CpuV3FpuSpecialPath {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3FpuSpecialPath").namespace(["components", "cpu", "cpu_v3"])
    }
}

#[derive(Default, Clone)]
pub struct CpuV3FpuSpecialPathState {
    p0_valid: bool,
    p0_rcp: bool,
    p0_negative: bool,
    p0_magnitude: u32,
    p0_fd: u8,
    s1_valid: bool,
    s1_residue: u16,
    s1_shift: u8,
    s1_left: bool,
    s1_zero: bool,
    interp_wait: bool,
    meta_s1_valid: bool,
    meta_s1_base: u32,
    meta_s2_base: u32,
    sc_valid: bool,
    sc_stage: u8,
    sc_fd: u8,
    sc_mode: u8,
    sc_phase_reg: u32,
}

struct CpuV3FpuSpecialPathComputed {
    load_now: bool,
    is_rcp: bool,
    is_sincos: bool,
    p0_negative: bool,
    p0_magnitude: u32,
    read_a_address: u16,
    read_b_address: u16,
    write_enable: bool,
    write_address: u16,
    write_data: u32,
    busy: bool,
    r_wait: u8,
    w_wait: u8,
    x_wait: u8,
    mul_in_valid: bool,
    mul_in_a: i64,
    mul_in_b: i64,
    mul_in_tag: u16,
    s1_residue: u16,
    s1_shift: u8,
    s1_left: bool,
    s1_zero: bool,
    interp_issue: bool,
    capture_base: u32,
    fd: u8,
    sc_mode: u8,
}

impl CpuV3FpuSpecialPathState {
    fn compute(&self, input: &CpuV3FpuSpecialPathInputValue) -> CpuV3FpuSpecialPathComputed {
        let instr_opcode = input.instr_opcode as u8;
        let word1 = input.word1_raw as u16;
        let subop = encoding::scalar_subop(word1);
        let fd = encoding::word1_fd(word1);
        let mode = encoding::word1_mode(word1) & 0x3;
        let is_scalar = instr_opcode == encoding::OPCODE_SCALAR;
        let is_rcp = is_scalar && subop == encoding::RCP;
        let is_rsqrt = is_scalar && subop == encoding::RSQRT;
        let is_sincos = is_scalar && subop == encoding::SINCOS;
        let load_now = input.instr_complete && (is_rcp || is_rsqrt || is_sincos) && !input.abort;

        let x0 = input.rf_read_a_data as u32;
        let x0_negative = x0 & 0x8000_0000 != 0;
        let x0_magnitude = if x0_negative { x0.wrapping_neg() } else { x0 };
        let p0_zero = self.p0_magnitude == 0;
        let clz = self.p0_magnitude.leading_zeros();

        let normalized = self.p0_magnitude.wrapping_shl(clz);
        let norm_index = ((normalized >> 24) & 0x7F) as u8;
        let norm_residue = ((normalized >> 15) & 0x1FF) as u16;

        let rcp_shift = clz.abs_diff(15) as u8;
        let rcp_left = clz >= 15;

        let rsqrt_odd = clz & 1 == 0;
        let rsqrt_left = clz > 15;
        let rsqrt_distance = clz.abs_diff(15);
        let rsqrt_shift = if rsqrt_left {
            rsqrt_distance.div_ceil(2)
        } else {
            rsqrt_distance / 2
        };

        let table_address = if self.p0_rcp || !rsqrt_odd {
            128u16 | u16::from(norm_index)
        } else {
            256u16 | u16::from(norm_index)
        };

        let sc_q = ((self.sc_phase_reg >> 16) & 3) as u8;
        let sc_f = self.sc_phase_reg as u16;
        let sc_u_sin = if sc_q & 1 == 1 {
            0x1_0000 - i64::from(sc_f)
        } else {
            i64::from(sc_f)
        };
        let sc_u_cos = if sc_q & 1 == 1 {
            i64::from(sc_f)
        } else {
            0x1_0000 - i64::from(sc_f)
        };
        let sc_single = matches!(self.sc_mode, 1 | 2);
        let sc_first_is_cos = self.sc_mode == 2;
        let sc_interp_is_cos = sc_first_is_cos || (!sc_single && self.sc_stage == 5);
        let sc_interp_u = if sc_interp_is_cos { sc_u_cos } else { sc_u_sin };
        let sc_residue = sc_interp_u & 0xFF;

        let packed_interval = if self.p0_rcp {
            input.rf_read_a_data as u32
        } else {
            input.rf_read_b_data as u32
        };
        let interval_current = packed_interval & 0x1_FFFF;
        let delta_bits = ((packed_interval >> 17) & 0x03FF) as i32;
        let interval_delta = (delta_bits << 22) >> 22;

        let sc_delta_bits = ((input.rf_read_a_data as u32 >> 17) & 0x03FF) as i32;
        let sc_delta = ((sc_delta_bits << 22) >> 22) as i64;
        let mul_x = if self.sc_valid {
            sc_delta
        } else {
            i64::from(interval_delta)
        };
        let mul_y = if self.sc_valid {
            sc_residue
        } else {
            i64::from(self.s1_residue)
        };
        let mul_product = input.mul_out_product as i64;
        let interpolated = (self.meta_s2_base as i32 + (mul_product >> 9) as i32) as u32;

        let interp_issue = self.s1_valid
            || (self.sc_valid && (self.sc_stage == 4 || (!sc_single && self.sc_stage == 5)));
        let range_issue = load_now && is_sincos;
        let issue_sign = if sc_interp_is_cos {
            ((sc_q >> 1) & 1) ^ (sc_q & 1)
        } else {
            (sc_q >> 1) & 1
        };
        let mul_in_valid = (range_issue || interp_issue) && !input.abort;
        let mul_in_a = if range_issue {
            i64::from(x0 as i32)
        } else {
            mul_x
        };
        let mul_in_b = if range_issue { SINCOS_K } else { mul_y };
        let mul_in_tag = if range_issue {
            0x81
        } else if self.sc_valid {
            0xc0 | (((sc_interp_u >> 15) as u16) & 2) | u16::from(issue_sign)
        } else {
            0x80
        };

        let sc_addr_u = if self.sc_stage == 4 || sc_first_is_cos {
            sc_u_cos
        } else {
            sc_u_sin
        };
        let sc_lut_address = 256u16 + ((sc_addr_u as u32 & 0xFFFF) >> 8) as u16;

        let read_a_address = if self.p0_valid && self.p0_rcp {
            table_address
        } else if self.sc_valid && (self.sc_stage == 3 || (!sc_single && self.sc_stage == 4)) {
            sc_lut_address
        } else {
            0
        };
        let read_b_address = if self.p0_valid && !self.p0_rcp {
            table_address
        } else {
            0
        };

        let sc_current = (input.rf_read_a_data as u32) & 0x1_FFFF;
        let sc_interp_sum = i64::from(self.meta_s2_base) + (mul_product >> 8);
        let sc_interp_mag = if input.mul_out_tag & 2 != 0 {
            0x1_0000
        } else {
            (sc_interp_sum & 0x1_FFFF) as u32
        };
        let sc_result_sign = input.mul_out_tag as u8 & 1;
        let sc_result: u32 = if sc_result_sign == 1 {
            (-(sc_interp_mag as i32)) as u32
        } else {
            sc_interp_mag
        };

        let returned_interpolated = interpolated & 0x1_ffff;
        let scaled = if self.s1_left {
            let wide = u64::from(returned_interpolated) << self.s1_shift;
            if self.p0_rcp && wide > 0x7FFF_FFFF {
                0x7FFF_FFFF
            } else {
                wide as u32
            }
        } else {
            returned_interpolated >> self.s1_shift
        };
        let rcp_value = if self.p0_negative {
            scaled.wrapping_neg()
        } else {
            scaled
        };
        let out_rcp = if self.s1_zero {
            if self.p0_negative {
                0x8000_0001
            } else {
                0x7FFF_FFFF
            }
        } else {
            rcp_value
        };
        let out_rsqrt = if self.s1_zero { 0 } else { scaled };
        let rcp_result = if self.p0_rcp { out_rcp } else { out_rsqrt };

        let sc_write = self.sc_valid
            && input.mul_out_valid
            && input.mul_out_tag & 0xc0 == 0xc0
            && !input.abort;
        let rcp_return = input.mul_out_valid && input.mul_out_tag == 0x80;
        let write_enable = (rcp_return || sc_write) && !input.abort;
        let write_address = if sc_write {
            u16::from(self.sc_fd.wrapping_add(u8::from(self.sc_stage == 7)) & 0x3F)
        } else {
            u16::from(self.p0_fd)
        };
        let write_data = if sc_write { sc_result } else { rcp_result };

        CpuV3FpuSpecialPathComputed {
            load_now,
            is_rcp,
            is_sincos,
            p0_negative: x0_negative,
            p0_magnitude: x0_magnitude,
            read_a_address,
            read_b_address,
            write_enable,
            write_address,
            write_data,
            busy: (load_now || self.p0_valid || self.s1_valid || self.interp_wait || self.sc_valid)
                && !input.abort,
            r_wait: 0,
            w_wait: if load_now {
                if is_sincos {
                    if matches!(mode, 1 | 2) {
                        7
                    } else {
                        8
                    }
                } else {
                    5
                }
            } else {
                0
            },
            x_wait: if load_now {
                if is_sincos {
                    if matches!(mode, 1 | 2) {
                        7
                    } else {
                        8
                    }
                } else {
                    5
                }
            } else {
                0
            },
            mul_in_valid,
            mul_in_a,
            mul_in_b,
            mul_in_tag,
            s1_residue: norm_residue,
            s1_shift: if self.p0_rcp {
                rcp_shift
            } else {
                rsqrt_shift as u8
            },
            s1_left: if self.p0_rcp { rcp_left } else { rsqrt_left },
            s1_zero: if self.p0_rcp {
                p0_zero
            } else {
                self.p0_negative || p0_zero
            },
            interp_issue,
            capture_base: if self.sc_valid {
                sc_current
            } else {
                interval_current
            },
            fd,
            sc_mode: mode,
        }
    }

    pub(super) fn comb(
        &self,
        input: &CpuV3FpuSpecialPathInputValue,
    ) -> CpuV3FpuSpecialPathOutputValue {
        let c = self.compute(input);
        CpuV3FpuSpecialPathOutputValue {
            rf_read_a_address: u64::from(c.read_a_address),
            rf_read_b_address: u64::from(c.read_b_address),
            rf_write_enable: c.write_enable,
            rf_write_address: u64::from(c.write_address),
            rf_write_data: u64::from(c.write_data),
            busy: c.busy,
            r_wait: u64::from(c.r_wait),
            w_wait: u64::from(c.w_wait),
            x_wait: u64::from(c.x_wait),
            mul_in_valid: c.mul_in_valid,
            mul_in_a: c.mul_in_a as u64,
            mul_in_b: c.mul_in_b as u64,
            mul_in_tag: u64::from(c.mul_in_tag),
        }
    }

    pub(super) fn tick(&mut self, input: &CpuV3FpuSpecialPathInputValue) {
        let c = self.compute(input);
        let old_meta_valid = self.meta_s1_valid;
        let old_meta_base = self.meta_s1_base;
        self.meta_s1_valid = c.interp_issue && !input.abort;
        if c.interp_issue {
            self.meta_s1_base = c.capture_base;
        }
        if old_meta_valid {
            self.meta_s2_base = old_meta_base;
        }
        if input.mul_out_valid && input.mul_out_tag == 0x81 {
            self.sc_phase_reg = ((input.mul_out_product as i64) >> 32) as u32 & 0x3_FFFF;
        }
        if input.abort {
            self.p0_valid = false;
            self.s1_valid = false;
            self.interp_wait = false;
            self.sc_valid = false;
            self.sc_stage = 0;
        } else if c.load_now {
            if c.is_sincos {
                self.sc_valid = true;
                self.sc_stage = 1;
                self.sc_fd = c.fd;
                self.sc_mode = c.sc_mode;
                self.p0_valid = false;
                self.s1_valid = false;
                self.interp_wait = false;
            } else {
                self.sc_valid = false;
                self.p0_valid = true;
                self.p0_rcp = c.is_rcp;
                self.p0_negative = c.p0_negative;
                self.p0_magnitude = c.p0_magnitude;
                self.p0_fd = c.fd;
                self.s1_valid = false;
                self.interp_wait = false;
            }
        } else if self.sc_valid {
            let sc_stage = self.sc_stage;
            let sc_single = matches!(self.sc_mode, 1 | 2);
            match sc_stage {
                6 if sc_single => {
                    self.sc_valid = false;
                    self.sc_stage = 0;
                }
                7 => {
                    self.sc_valid = false;
                    self.sc_stage = 0;
                }
                _ => {}
            }
            if sc_stage != 7 && !(sc_stage == 6 && sc_single) {
                self.sc_stage = sc_stage + 1;
            }
        } else if self.p0_valid {
            self.p0_valid = false;
            self.s1_valid = true;
            self.s1_residue = c.s1_residue;
            self.s1_shift = c.s1_shift;
            self.s1_left = c.s1_left;
            self.s1_zero = c.s1_zero;
            self.interp_wait = false;
        } else if self.s1_valid {
            self.interp_wait = true;
            self.s1_valid = false;
        } else if input.mul_out_valid && input.mul_out_tag == 0x80 {
            self.interp_wait = false;
        }
    }
}

impl Module for CpuV3FpuSpecialPath {
    type Input = CpuV3FpuSpecialPathInput;
    type Output = CpuV3FpuSpecialPathOutput;
    type EmuState = CpuV3FpuSpecialPathState;

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        Self::EmuState::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        output.drive(circuit, &state.comb(&input));
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        state.tick(&input);
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_special_path.v").to_string())
    }

    fn target_resources() -> Vec<TargetResourceRequest> {
        Vec::new()
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_special_path_tb.v").to_string())
    }
}
#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuVectorPathInput {
    pub alu_result: Wires<32>,
    pub abort: Wire,
    pub instr_complete: Wire,
    pub instr_opcode: Wires<4>,
    pub word1_raw: Wires<16>,
    pub base_a: Wires<6>,
    pub base_b: Wires<6>,
    pub rf_read_a_data: Wires<32>,
    pub rf_read_b_data: Wires<32>,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuVectorPathOutput {
    pub alu_request_op: Wires<4>,
    pub rf_read_a_address: Wires<9>,
    pub rf_read_b_address: Wires<9>,
    pub rf_write_enable: Wire,
    pub rf_write_address: Wires<9>,
    pub rf_write_data: Wires<32>,
    pub busy: Wire,
}

/// FPU v2 vector execution path (opcode 0xC): one lane per cycle through
/// the shared combinational scalar ALU. Emu lives in the unit top
/// (CpuV3FpuState); this leaf is verified through its Verilog testbench.
pub struct CpuV3FpuVectorPath;

impl HardwareIdentity for CpuV3FpuVectorPath {
    const TARGET_RESOURCE_LEAF: bool = false;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3FpuVectorPath").namespace(["components", "cpu", "cpu_v3"])
    }
}

impl Module for CpuV3FpuVectorPath {
    type Input = CpuV3FpuVectorPathInput;
    type Output = CpuV3FpuVectorPathOutput;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_vector_path.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(format!(
            "{}\n{}",
            include_str!("cpu_v3_fpu_vector_path_tb.v"),
            include_str!("cpu_v3_fpu_scalar_alu.v")
        ))
    }
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuAluPathInput {
    pub abort: Wire,
    pub instr_complete: Wire,
    pub instr_opcode: Wires<4>,
    pub word1_raw: Wires<16>,
    pub base_a: Wires<6>,
    pub base_b: Wires<6>,
    pub alu_result: Wires<32>,
    pub alu_lt: Wire,
    pub alu_eq: Wire,
    pub alu_gt: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuAluPathOutput {
    pub alu_request_op: Wires<4>,
    pub rf_read_a_address: Wires<9>,
    pub rf_read_b_address: Wires<9>,
    pub rf_write_enable: Wire,
    pub rf_write_address: Wires<9>,
    pub rf_write_data: Wires<32>,
    pub flag_lt: Wire,
    pub flag_eq: Wire,
    pub flag_gt: Wire,
    pub vector_busy: Wire,
    pub busy: Wire,
}

/// One scalar/vector instruction descriptor and RF result commit stage.
/// Scalar operands enter ready at T0, preserving T1 writeback; vector lanes
/// keep the synchronous-read pipeline. The unit value model remains cycle exact.
pub struct CpuV3FpuAluPath;

impl HardwareIdentity for CpuV3FpuAluPath {
    const TARGET_RESOURCE_LEAF: bool = false;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3FpuAluPath").namespace(["components", "cpu", "cpu_v3"])
    }
}

impl Module for CpuV3FpuAluPath {
    type Input = CpuV3FpuAluPathInput;
    type Output = CpuV3FpuAluPathOutput;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_alu_path.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        // Reuse the independent vector golden unchanged through the new ports.
        // Scalar fast-entry timing/flags/numerics are covered by the unit TB.
        let vector_tb = include_str!("cpu_v3_fpu_vector_path_tb.v")
            .replace(
                "CpuV3FpuVectorPath vector_path (",
                "CpuV3FpuAluPath vector_path (\n\
                 .alu_lt(alu_lt), .alu_eq(alu_eq), .alu_gt(alu_gt),\n\
                 .flag_lt(), .flag_eq(), .flag_gt(), .vector_busy(),",
            )
            .replace("    .rf_read_a_data(rf_read_a_data),", "")
            .replace("    .rf_read_b_data(rf_read_b_data),", "");
        Some(format!(
            "{vector_tb}\n{}",
            include_str!("cpu_v3_fpu_scalar_alu.v")
        ))
    }
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuMultiplyPathInput {
    pub abort: Wire,
    pub instr_complete: Wire,
    pub instr_opcode: Wires<4>,
    pub word1_raw: Wires<16>,
    pub base_a: Wires<6>,
    pub base_b: Wires<6>,
    pub rf_read_a_data: Wires<32>,
    pub rf_read_b_data: Wires<32>,
    pub mul_out_valid: Wire,
    pub mul_out_product: Wires<64>,
    pub mul_out_tag: Wires<9>,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuMultiplyPathOutput {
    pub mul_in_valid: Wire,
    pub mul_in_a: Wires<36>,
    pub mul_in_b: Wires<36>,
    pub mul_in_tag: Wires<9>,
    pub rf_read_a_address: Wires<9>,
    pub rf_read_b_address: Wires<9>,
    pub rf_write_enable: Wire,
    pub rf_write_address: Wires<9>,
    pub rf_write_data: Wires<32>,
    pub busy: Wire,
}

/// FPU v2 multiply execution path (VMUL/VMULS/scalar MUL): one lane per
/// cycle through an inferred 36x36 signed multiplier, three register stages,
/// narrowing to Q16.16 at the write port. Emu lives in the unit top.
pub struct CpuV3FpuMultiplyPath;

impl HardwareIdentity for CpuV3FpuMultiplyPath {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3FpuMultiplyPath").namespace(["components", "cpu", "cpu_v3"])
    }
}

impl Module for CpuV3FpuMultiplyPath {
    type Input = CpuV3FpuMultiplyPathInput;
    type Output = CpuV3FpuMultiplyPathOutput;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    // The multiplier lives in the shared CpuV3FpuMulPipe leaf; this
    // controller claims nothing itself.
    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_multiply_path.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_multiply_path_tb.v").to_string())
    }
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuDotPathInput {
    pub abort: Wire,
    pub instr_complete: Wire,
    pub instr_opcode: Wires<4>,
    pub word1_raw: Wires<16>,
    pub base_a: Wires<6>,
    pub base_b: Wires<6>,
    pub rf_read_a_data: Wires<32>,
    pub rf_read_b_data: Wires<32>,
    pub mul_out_valid: Wire,
    pub mul_out_product: Wires<64>,
    pub mul_out_tag: Wires<9>,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuDotPathOutput {
    pub mul_in_valid: Wire,
    pub mul_in_a: Wires<36>,
    pub mul_in_b: Wires<36>,
    pub mul_in_tag: Wires<9>,
    pub rf_read_a_address: Wires<9>,
    pub rf_read_b_address: Wires<9>,
    pub rf_write_enable: Wire,
    pub rf_write_address: Wires<9>,
    pub rf_write_data: Wires<32>,
    pub busy: Wire,
    pub acc_out: Wires<64>,
}

/// FPU v2 dot-product path (DOT/DOTADD/DOTSTORE, opcode 0xC): per-lane full
/// 72-bit products accumulate into a 64-bit Q32.32 fabric ACC (LUT adder,
/// syn_dspstyle="logic"), narrowing only at DOTSTORE. Emu lives in the unit
/// top; this leaf is verified through its Verilog testbench.
pub struct CpuV3FpuDotPath;

impl HardwareIdentity for CpuV3FpuDotPath {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3FpuDotPath").namespace(["components", "cpu", "cpu_v3"])
    }
}

impl Module for CpuV3FpuDotPath {
    type Input = CpuV3FpuDotPathInput;
    type Output = CpuV3FpuDotPathOutput;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn target_resources() -> Vec<TargetResourceRequest> {
        // The 64-bit accumulate maps to a MULTADDALU18X18 macro (a full
        // macro, two lanes) even though the multiplier moved into the shared
        // pipe leaf; the fusion is deliberate, see the leaf's header comment.
        vec![TargetResourceRequest::new(
            digital_design_hardware::resources::components::DspMultipliers::new(2),
        )]
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_dot_path.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_dot_path_tb.v").to_string())
    }
}

/// One physical product issuer, shared by ordinary multiply and dot modes.
/// Its actual register execution model lives in CpuV3FpuState.
pub struct CpuV3FpuProductPath;
impl HardwareIdentity for CpuV3FpuProductPath {
    const TARGET_RESOURCE_LEAF: bool = true;
    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3FpuProductPath").namespace(["components", "cpu", "cpu_v3"])
    }
}
impl Module for CpuV3FpuProductPath {
    type Input = CpuV3FpuDotPathInput;
    type Output = CpuV3FpuDotPathOutput;
    type EmuState = ();
    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;
    fn target_resources() -> Vec<TargetResourceRequest> {
        Vec::new()
    }
    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_product_path.v").to_string())
    }
}
