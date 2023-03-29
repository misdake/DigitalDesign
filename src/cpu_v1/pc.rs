use super::CpuComponent;
use crate::cpu_v1::CpuComponentEmu;
use crate::{add_naive, flatten2, input_const, input_w, Wire, Wires};

#[derive(Clone)]
pub struct CpuPcInput {
    pub curr_pc: Wires<8>,
    pub pc_offset_enable: Wire,
    pub pc_offset: Wires<4>,
    pub jmp_long_enable: Wire,
    pub jmp_long: Wires<4>,
}
#[derive(Clone)]
pub struct CpuPcOutput {
    pub next_pc: Wires<8>,
}

pub struct CpuPcEmu;
impl CpuComponentEmu<CpuPc> for CpuPcEmu {
    fn init_output() -> CpuPcOutput {
        CpuPcOutput { next_pc: input_w() }
    }
    fn execute(input: &CpuPcInput, output: &CpuPcOutput) {
        assert_eq!(
            1,
            input.jmp_long_enable.get() + input.pc_offset_enable.get()
        );

        let curr_pc = input.curr_pc.get_u8();
        let offset = input.pc_offset.get_u8();
        let long = input.jmp_long.get_u8();
        let next_pc = if input.pc_offset_enable.is_one() {
            curr_pc + offset
        } else if input.jmp_long_enable.is_one() {
            long * 16
        } else {
            curr_pc + 1
        };
        output.next_pc.set_u8(next_pc);
    }
}

pub struct CpuPc;
impl CpuComponent for CpuPc {
    type Input = CpuPcInput;
    type Output = CpuPcOutput;

    fn build(input: &CpuPcInput) -> CpuPcOutput {
        let next_pc = next_pc(
            input.curr_pc,
            input.pc_offset_enable,
            input.pc_offset,
            input.jmp_long_enable,
            input.jmp_long,
        );
        CpuPcOutput { next_pc }
    }
}

fn next_pc(
    curr_pc: Wires<8>,
    pc_offset_enable: Wire,
    pc_offset: Wires<4>,
    jmp_long_enable: Wire,
    jmp_long: Wires<4>,
) -> Wires<8> {
    let offset_target = add_naive(curr_pc, pc_offset.expand_signed::<8>());
    let offset_target = pc_offset_enable.expand() & offset_target.sum;

    let zero = input_const(0);
    let long_target: Wires<8> = flatten2(zero.expand::<4>(), jmp_long);
    let long_target = jmp_long_enable.expand() & long_target;

    let next_pc = offset_target | long_target;
    return next_pc;
}

#[test]
fn test_next_pc() {
    use crate::*;
    clear_all();

    let pc_offset_enable = input();
    let pc_offset = input_w::<4>();
    let jmp_long_enable = input();
    let jmp_long = input_w::<4>();

    let offset = |v: u8| {
        pc_offset_enable.set(1);
        pc_offset.set_u8(v);
        jmp_long_enable.set(0);
    };
    let long = |v: u8| {
        pc_offset_enable.set(0);
        jmp_long_enable.set(1);
        jmp_long.set_u8(v);
    };
    let next = || {
        pc_offset_enable.set(1);
        pc_offset.set_u8(1);
        jmp_long_enable.set(0);
    };

    let mut pc = reg_w::<8>();
    let next_pc = next_pc(
        pc.out,
        pc_offset_enable,
        pc_offset,
        jmp_long_enable,
        jmp_long,
    );
    pc.set_in(next_pc);

    let mut reference_pc: i32 = 0;
    let testcases = shuffled_list(1 << 6, 123.4);
    for testcase in testcases {
        let enable = (testcase) % 4;
        let value = ((testcase >> 2) % 16) as u8;
        match enable {
            0 => {
                let i = select(value >= 8, value as i32 - 16, value as i32);
                reference_pc = reference_pc + i;
                offset(value);
                print!("offset {}", i);
            }
            1 => {
                reference_pc = value as i32 * 16;
                long(value);
                print!("long {}", value);
            }
            _ => {
                reference_pc = reference_pc + 1;
                next();
                print!("next",);
            }
        }

        simulate();
        reference_pc = reference_pc % 256;
        let result_pc = pc.out.get_u8();
        println!(" => ref {}, result {}", reference_pc, result_pc);

        assert_eq!(result_pc as i32, reference_pc);
    }
}
