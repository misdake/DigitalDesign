use super::{ExecOp, EXEC_OP_WIDTH, FLAGS_WIDTH, WORD_WIDTH};
use crate::semantics::{calc_flags, calc_flags_signed};
use digital_design_circuit::{
    add_naive, input_const, input_w, input_w_const, mux2_w, CircuitComponent, CircuitComponentEmu,
    CircuitWires, Wire, Wires, WiresU16, WiresU8,
};

#[derive(Clone)]
pub struct ExecuteInput {
    pub pc: Wires<WORD_WIDTH>,
    pub source_a: Wires<WORD_WIDTH>,
    pub source_b: Wires<WORD_WIDTH>,
    pub immediate: Wires<WORD_WIDTH>,
    pub operation: Wires<EXEC_OP_WIDTH>,
}

#[derive(Clone)]
pub struct ExecuteOutput {
    pub result: Wires<WORD_WIDTH>,
    pub flags: Wires<FLAGS_WIDTH>,
    pub memory_address: Wires<WORD_WIDTH>,
    pub memory_write: Wires<WORD_WIDTH>,
    pub pc_target: Wires<WORD_WIDTH>,
    pub device_write: Wires<WORD_WIDTH>,
    pub halt_signal: Wires<WORD_WIDTH>,
}

pub struct CpuExecute;

fn any(values: &[Wire]) -> Wire {
    values
        .iter()
        .copied()
        .fold(input_const(0), |output, value| output | value)
}

fn equal_const<const W: usize>(value: Wires<W>, constant: u16) -> Wire {
    value
        .wires
        .iter()
        .enumerate()
        .fold(input_const(1), |equal, (bit, wire)| {
            equal & wire.eq_const(((constant >> bit) & 1) as u8)
        })
}

fn select<const W: usize>(cases: &[(Wire, Wires<W>)]) -> Wires<W> {
    cases
        .iter()
        .fold(input_w_const(0), |output, (selected, value)| {
            output | selected.expand() & *value
        })
}

fn one_bit_value(bit: Wire) -> Wires<WORD_WIDTH> {
    Wires {
        wires: std::array::from_fn(|index| if index == 0 { bit } else { input_const(0) }),
    }
}

fn shift_left(mut value: Wires<WORD_WIDTH>, amount: Wires<4>) -> Wires<WORD_WIDTH> {
    for stage in 0..4 {
        let distance = 1 << stage;
        let shifted = Wires {
            wires: std::array::from_fn(|bit| {
                if bit >= distance {
                    value.wires[bit - distance]
                } else {
                    input_const(0)
                }
            }),
        };
        value = mux2_w(value, shifted, amount.wires[stage]);
    }
    value
}

fn shift_right(
    mut value: Wires<WORD_WIDTH>,
    amount: Wires<4>,
    arithmetic: bool,
) -> Wires<WORD_WIDTH> {
    for stage in 0..4 {
        let distance = 1 << stage;
        let fill = if arithmetic {
            value.wires[WORD_WIDTH - 1]
        } else {
            input_const(0)
        };
        let shifted = Wires {
            wires: std::array::from_fn(|bit| {
                if bit + distance < WORD_WIDTH {
                    value.wires[bit + distance]
                } else {
                    fill
                }
            }),
        };
        value = mux2_w(value, shifted, amount.wires[stage]);
    }
    value
}

fn count_ones(value: Wires<WORD_WIDTH>) -> Wires<WORD_WIDTH> {
    value
        .wires
        .into_iter()
        .fold(input_w_const(0), |count, bit| {
            add_naive(count, one_bit_value(bit)).sum
        })
}

fn log2(value: Wires<WORD_WIDTH>) -> Wires<WORD_WIDTH> {
    let mut higher_set = input_const(0);
    let mut selected = [Wire(0); WORD_WIDTH];
    for bit in (0..WORD_WIDTH).rev() {
        selected[bit] = value.wires[bit] & !higher_set;
        higher_set = higher_set | value.wires[bit];
    }

    Wires {
        wires: std::array::from_fn(|output_bit| {
            if output_bit < 4 {
                any(&(0..WORD_WIDTH)
                    .filter(|value_bit| value_bit & (1 << output_bit) != 0)
                    .map(|value_bit| selected[value_bit])
                    .collect::<Vec<_>>())
            } else {
                input_const(0)
            }
        }),
    }
}

fn compare(a: Wires<WORD_WIDTH>, b: Wires<WORD_WIDTH>) -> Wires<FLAGS_WIDTH> {
    let mut equal = input_const(1);
    let mut less = input_const(0);

    for bit in 0..WORD_WIDTH {
        let bits_equal = !(a.wires[bit] ^ b.wires[bit]);
        less = (!a.wires[bit] & b.wires[bit]) | (bits_equal & less);
        equal = equal & bits_equal;
    }

    Wires {
        wires: [!less & !equal, equal, less],
    }
}

fn compare_signed(mut a: Wires<WORD_WIDTH>, mut b: Wires<WORD_WIDTH>) -> Wires<FLAGS_WIDTH> {
    a.wires[WORD_WIDTH - 1] = !a.wires[WORD_WIDTH - 1];
    b.wires[WORD_WIDTH - 1] = !b.wires[WORD_WIDTH - 1];
    compare(a, b)
}

impl CircuitComponent for CpuExecute {
    type Input = ExecuteInput;
    type Output = ExecuteOutput;

    fn build(input: &Self::Input) -> Self::Output {
        let operation = |value: ExecOp| equal_const(input.operation, value as u16);
        let pass_a = operation(ExecOp::PassA);
        let inv = operation(ExecOp::Inv);
        let neg = operation(ExecOp::Neg);
        let not_zero = operation(ExecOp::NotZero);
        let count_ones_op = operation(ExecOp::CountOnes);
        let log2_op = operation(ExecOp::Log2);
        let lsl = operation(ExecOp::Lsl);
        let lsr = operation(ExecOp::Lsr);
        let asr = operation(ExecOp::Asr);
        let and = operation(ExecOp::And);
        let or = operation(ExecOp::Or);
        let xor = operation(ExecOp::Xor);
        let add = operation(ExecOp::Add);
        let sub = operation(ExecOp::Sub);
        let add_immediate = operation(ExecOp::AddImmediate);
        let load_hi = operation(ExecOp::LoadHi);
        let load_lo = operation(ExecOp::LoadLo);
        let pc_add = operation(ExecOp::PcAdd);
        let compare_unsigned = operation(ExecOp::CompareUnsigned);
        let compare_unsigned_immediate = operation(ExecOp::CompareUnsignedImmediate);
        let compare_signed_op = operation(ExecOp::CompareSigned);
        let compare_signed_immediate = operation(ExecOp::CompareSignedImmediate);
        let call_relative = operation(ExecOp::CallRelative);
        let call_absolute = operation(ExecOp::CallAbsolute);
        let call_register = operation(ExecOp::CallRegister);

        let one = Wires::<WORD_WIDTH>::parse_u16(1);
        let pc_next = add_naive(input.pc, one).sum;
        let add_result = add_naive(input.source_a, input.source_b).sum;
        let sub_result = add_naive(input.source_a, add_naive(!input.source_b, one).sum).sum;
        let immediate_result = add_naive(input.source_a, input.immediate).sum;
        let pc_add_result = add_naive(input.pc, input.immediate).sum;
        let neg_result = add_naive(!input.source_a, one).sum;
        let not_zero_result = one_bit_value(any(&input.source_a.wires));
        let shift_amount = Wires {
            wires: input.immediate.wires[0..4].try_into().unwrap(),
        };
        let load_hi_result =
            input.immediate | (input.source_a & Wires::<WORD_WIDTH>::parse_u16(0x00ff));

        let result = select(&[
            (pass_a, input.source_a),
            (inv, !input.source_a),
            (neg, neg_result),
            (not_zero, not_zero_result),
            (count_ones_op, count_ones(input.source_a)),
            (log2_op, log2(input.source_a)),
            (lsl, shift_left(input.source_a, shift_amount)),
            (lsr, shift_right(input.source_a, shift_amount, false)),
            (asr, shift_right(input.source_a, shift_amount, true)),
            (and, input.source_a & input.source_b),
            (or, input.source_a | input.source_b),
            (xor, input.source_a ^ input.source_b),
            (add, add_result),
            (sub, sub_result),
            (add_immediate, immediate_result),
            (load_hi, load_hi_result),
            (load_lo, input.immediate),
            (pc_add, pc_add_result),
            (call_relative | call_absolute | call_register, pc_next),
        ]);
        let flags = select(&[
            (compare_unsigned, compare(input.source_a, input.source_b)),
            (
                compare_unsigned_immediate,
                compare(input.source_a, input.immediate),
            ),
            (
                compare_signed_op,
                compare_signed(input.source_a, input.source_b),
            ),
            (
                compare_signed_immediate,
                compare_signed(input.source_a, input.immediate),
            ),
        ]);

        ExecuteOutput {
            result,
            flags,
            memory_address: select(&[
                (add_immediate, immediate_result),
                (call_absolute, input.immediate),
            ]),
            memory_write: input.source_b,
            pc_target: select(&[
                (pass_a | call_register, input.source_a),
                (pc_add, pc_add_result),
                (call_relative, pc_add_result),
            ]),
            device_write: input.source_a,
            halt_signal: input.source_a,
        }
    }
}

pub struct CpuExecuteEmu;

impl CircuitComponentEmu<CpuExecute> for CpuExecuteEmu {
    fn create(input: &ExecuteInput) -> (Self, ExecuteOutput) {
        let output = ExecuteOutput {
            result: input_w(),
            flags: input_w(),
            memory_address: input_w(),
            memory_write: input_w(),
            pc_target: input_w(),
            device_write: input_w(),
            halt_signal: input_w(),
        };
        let latency = input
            .pc
            .get_max_latency_external()
            .max(input.source_a.get_max_latency_external())
            .max(input.source_b.get_max_latency_external())
            .max(input.immediate.get_max_latency_external())
            .max(input.operation.get_max_latency_external())
            + 1;
        output.result.set_latency_external(latency);
        output.flags.set_latency_external(latency);
        output.memory_address.set_latency_external(latency);
        output.memory_write.set_latency_external(latency);
        output.pc_target.set_latency_external(latency);
        output.device_write.set_latency_external(latency);
        output.halt_signal.set_latency_external(latency);
        (Self, output)
    }

    fn execute(
        &mut self,
        circuit: &mut CircuitWires,
        input: &ExecuteInput,
        output: &ExecuteOutput,
    ) {
        let pc = input.pc.get_u16(circuit);
        let source_a = input.source_a.get_u16(circuit);
        let source_b = input.source_b.get_u16(circuit);
        let immediate = input.immediate.get_u16(circuit);
        let operation = ExecOp::from_raw(input.operation.get_u8(circuit));

        let mut result = 0;
        let mut flags = 0;
        let mut memory_address = 0;
        let mut pc_target = 0;

        match operation {
            ExecOp::Idle => {}
            ExecOp::PassA => {
                result = source_a;
                pc_target = source_a;
            }
            ExecOp::Inv => result = !source_a,
            ExecOp::Neg => result = (source_a as i16).wrapping_neg() as u16,
            ExecOp::NotZero => result = u16::from(source_a != 0),
            ExecOp::CountOnes => result = source_a.count_ones() as u16,
            ExecOp::Log2 => {
                result = if source_a == 0 {
                    0
                } else {
                    source_a.ilog2() as u16
                }
            }
            ExecOp::Lsl => result = source_a << immediate,
            ExecOp::Lsr => result = source_a >> immediate,
            ExecOp::Asr => result = ((source_a as i16) >> immediate) as u16,
            ExecOp::And => result = source_a & source_b,
            ExecOp::Or => result = source_a | source_b,
            ExecOp::Xor => result = source_a ^ source_b,
            ExecOp::Add => result = source_a.wrapping_add(source_b),
            ExecOp::Sub => result = source_a.wrapping_sub(source_b),
            ExecOp::AddImmediate => {
                result = source_a.wrapping_add(immediate);
                memory_address = result;
            }
            ExecOp::LoadHi => result = immediate | (source_a & 0x00ff),
            ExecOp::LoadLo => result = immediate,
            ExecOp::PcAdd => {
                result = pc.wrapping_add(immediate);
                pc_target = result;
            }
            ExecOp::CompareUnsigned => flags = calc_flags(source_a, source_b),
            ExecOp::CompareUnsignedImmediate => flags = calc_flags(source_a, immediate),
            ExecOp::CompareSigned => flags = calc_flags_signed(source_a, source_b),
            ExecOp::CompareSignedImmediate => flags = calc_flags_signed(source_a, immediate),
            ExecOp::CallRelative => {
                result = pc.wrapping_add(1);
                pc_target = pc.wrapping_add(immediate);
            }
            ExecOp::CallAbsolute => {
                result = pc.wrapping_add(1);
                memory_address = immediate;
            }
            ExecOp::CallRegister => {
                result = pc.wrapping_add(1);
                pc_target = source_a;
            }
            ExecOp::Max => unreachable!(),
        }

        output.result.set_u16(circuit, result);
        output.flags.set_u8(circuit, flags);
        output.memory_address.set_u16(circuit, memory_address);
        output.memory_write.set_u16(circuit, source_b);
        output.pc_target.set_u16(circuit, pc_target);
        output.device_write.set_u16(circuit, source_a);
        output.halt_signal.set_u16(circuit, source_a);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_circuit::{build_circuit, input_w, CircuitWires};

    fn snapshot(circuit: &CircuitWires, output: &ExecuteOutput) -> (u16, u8, u16, u16, u16) {
        (
            output.result.get_u16(circuit),
            output.flags.get_u8(circuit),
            output.memory_address.get_u16(circuit),
            output.pc_target.get_u16(circuit),
            output.halt_signal.get_u16(circuit),
        )
    }

    #[test]
    fn gate_logic_matches_emu_for_basic_operations() {
        let (mut circuit, (input, gates, emu)) = build_circuit(|| {
            let input = ExecuteInput {
                pc: input_w(),
                source_a: input_w(),
                source_b: input_w(),
                immediate: input_w(),
                operation: input_w(),
            };
            let gates = CpuExecute::build(&input);
            let emu = CpuExecuteEmu::build(&input);
            (input, gates, emu)
        });

        let cases = [
            (ExecOp::Add, 0x1000, 0x1234, 0x4321, 0),
            (ExecOp::Sub, 0, 3, 5, 0),
            (ExecOp::CountOnes, 0, 0xa55a, 0, 0),
            (ExecOp::Log2, 0, 0, 0, 0),
            (ExecOp::Log2, 0, 0x8001, 0, 0),
            (ExecOp::Asr, 0, 0x8000, 0, 3),
            (ExecOp::CompareSigned, 0, 0xffff, 1, 0),
            (ExecOp::AddImmediate, 0, 0xfffe, 0, 5),
            (ExecOp::CallRelative, 0x1000, 0, 0, 0xfffc),
        ];

        for (operation, pc, source_a, source_b, immediate) in cases {
            input.pc.set_u16(&mut circuit, pc);
            input.source_a.set_u16(&mut circuit, source_a);
            input.source_b.set_u16(&mut circuit, source_b);
            input.immediate.set_u16(&mut circuit, immediate);
            input.operation.set_u8(&mut circuit, operation as u8);
            circuit.simulate();
            assert_eq!(
                snapshot(&circuit, &gates),
                snapshot(&circuit, &emu),
                "{operation:?}"
            );
        }
    }
}
