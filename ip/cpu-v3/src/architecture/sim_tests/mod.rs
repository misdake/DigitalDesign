use super::*;
use crate::{
    alu, branch, compare_signed, compare_unsigned, conditional_move, device_receive, device_send,
    fpu_aux, fpu_aux_raw, fpu_scalar, fpu_vector, fpu_vector_raw, halt, immediate_signed,
    immediate_unsigned, jump_and_link_register, jump_and_link_relative, jump_register,
    jump_relative, jump_segment, line_copy, load, load_immediate16, move_register, multiply,
    multiply_immediate, nop, population_count, prefix12, prefixed, prefixed_branch, read_special,
    set_less_than_signed, set_less_than_unsigned, shift_immediate, shift_register, signal, store,
    write_data_segment, AluOp, FpuAuxKind, FpuAuxSubop, FpuDotStride, FpuScalarSubop,
    FpuVectorLength, FpuVectorSubop, ImmediateOp, MultiplyWindow, ShiftOp, SpecialRegister,
    TestCondition,
};

/// Runs `RCP`, `RSQRT` and dual `SINCOS` on one raw Q16.16 input and
/// returns `(rcp, rsqrt, sin, cos)`.
fn run_special(input: i32) -> (i32, i32, i32, i32) {
    let raw = input as u32;
    let mut program = vec![];
    program.extend(load_immediate16(1, 0x0100));
    program.extend(fpu_aux(
        FpuAuxKind::IntegerRegister,
        1,
        0,
        0,
        FpuAuxSubop::Fld,
        0,
    )); // f0 = input
    program.extend(fpu_scalar(0, 0, 1, FpuScalarSubop::Rcp, 0));
    program.extend(fpu_scalar(0, 0, 2, FpuScalarSubop::Rsqrt, 0));
    program.extend(fpu_scalar(0, 0, 3, FpuScalarSubop::SinCos, 0));
    program.push(halt());
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();
    machine
        .load_program(0x0100, &[raw as u16, (raw >> 16) as u16])
        .unwrap();
    assert!(matches!(
        machine.run(64).unwrap(),
        RunOutcome::Halted { .. }
    ));
    (
        machine.fpu_register(1).unwrap(),
        machine.fpu_register(2).unwrap(),
        machine.fpu_register(3).unwrap(),
        machine.fpu_register(4).unwrap(),
    )
}

struct EchoDevice {
    channels: [Word; 16],
}

impl Device for EchoDevice {
    fn read(&mut self, _memory: &mut [Word], channel: u8) -> Word {
        self.channels[usize::from(channel)]
    }

    fn write(&mut self, _memory: &mut [Word], channel: u8, value: Word) {
        self.channels[usize::from(channel)] = value;
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn device_round_trip_program() -> Vec<Word> {
    let mut program = vec![];
    program.extend(load_immediate16(1, 0x1234));
    program.extend([device_send(1, 2, 3), device_receive(0, 2, 3), halt()]);
    program
}

mod addressing;

mod arithmetic;

mod control;

mod devices;

mod fpu;
