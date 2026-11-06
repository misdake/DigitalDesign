//! Shared scalar arithmetic leaf and instruction-model evaluation.
use super::*;

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuScalarAluInput {
    pub a: Wires<32>,
    pub b: Wires<32>,
    pub op: Wires<4>,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuScalarAluOutput {
    pub result: Wires<32>,
    pub flag_lt: Wire,
    pub flag_eq: Wire,
    pub flag_gt: Wire,
}

/// FPU v2 scalar Q16.16 ALU: purely combinational, all arithmetic in fabric
/// LUTs (syn_dspstyle="logic"; DSP absorption of wide adds is a measured
/// Gowin pitfall). Wrap overflow policy, round-half-up ROUND; flags are only
/// specified for op == CMP.
pub struct CpuV3FpuScalarAlu;

impl HardwareIdentity for CpuV3FpuScalarAlu {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3FpuScalarAlu").namespace(["components", "cpu", "cpu_v3"])
    }
}

impl Module for CpuV3FpuScalarAlu {
    type Input = CpuV3FpuScalarAluInput;
    type Output = CpuV3FpuScalarAluOutput;
    type EmuState = ();

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {}

    fn execute_emu(
        _state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        let a = input.a as u32;
        let b = input.b as u32;
        let sa = a as i32;
        let sb = b as i32;
        let floor_a = a & 0xFFFF_0000;
        let frac_nonzero = a & 0xFFFF != 0;
        let ceil_a = floor_a.wrapping_add(if frac_nonzero { 0x1_0000 } else { 0 });
        let result = match input.op & 0xF {
            0x0 => a.wrapping_add(b),
            0x1 => a.wrapping_sub(b),
            0x3 => {
                if sa < sb {
                    a
                } else {
                    b
                }
            }
            0x4 => {
                if sa > sb {
                    a
                } else {
                    b
                }
            }
            0x5 => {
                if sa < 0 {
                    a.wrapping_neg()
                } else {
                    a
                }
            }
            0x6 => a.wrapping_neg(),
            0x7 => floor_a,
            0x8 => ceil_a,
            0x9 => a.wrapping_add(0x8000) & 0xFFFF_0000,
            0xA => {
                if sa < 0 {
                    ceil_a
                } else {
                    floor_a
                }
            }
            0xB => 0,
            0xF => a,
            _ => 0,
        };
        output.drive(
            circuit,
            &CpuV3FpuScalarAluOutputValue {
                result: u64::from(result),
                flag_lt: sa < sb,
                flag_eq: sa == sb,
                flag_gt: sa > sb,
            },
        );
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_scalar_alu.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_scalar_alu_tb.v").to_string())
    }
}

/// Mirrors CpuV3FpuScalarAlu: wrap-only Q16.16 ops.
pub(super) fn evaluate(a: u32, b: u32, op: u8) -> (u32, bool, bool, bool) {
    let sa = a as i32;
    let sb = b as i32;
    let floor_a = a & 0xFFFF_0000;
    let frac = a & 0xFFFF != 0;
    let ceil_a = floor_a.wrapping_add(if frac { 0x1_0000 } else { 0 });
    let result = match op & 0xF {
        0x0 => a.wrapping_add(b),
        0x1 => a.wrapping_sub(b),
        0x3 => {
            if sa < sb {
                a
            } else {
                b
            }
        }
        0x4 => {
            if sa > sb {
                a
            } else {
                b
            }
        }
        0x5 => {
            if sa < 0 {
                a.wrapping_neg()
            } else {
                a
            }
        }
        0x6 => a.wrapping_neg(),
        0x7 => floor_a,
        0x8 => ceil_a,
        0x9 => a.wrapping_add(0x8000) & 0xFFFF_0000,
        0xA => {
            if sa < 0 {
                ceil_a
            } else {
                floor_a
            }
        }
        0xF => a,
        _ => 0,
    };
    (result, sa < sb, sa == sb, sa > sb)
}
