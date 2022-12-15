use crate::{add_naive, flatten2, input_const, reg_w, Wire, Wires};

pub fn pc(
    jmp_offset_enable: Wire,
    jmp_offset: Wires<4>,
    jmp_long_enable: Wire,
    jmp_long: Wires<4>,
    no_jmp_enable: Wire,
) -> Wires<8> {
    let mut pc = reg_w::<8>();

    let offset_target = add_naive(pc.out, jmp_offset.expand_signed::<8>());
    let offset_target = jmp_offset_enable.expand() & offset_target.sum;

    let zero = input_const(0);
    let long_target: Wires<8> = flatten2(zero.expand::<4>(), jmp_long);
    let long_target = jmp_long_enable.expand() & long_target;

    let one = input_const(1);
    let one_8: Wires<8> = flatten2(one.expand::<1>(), zero.expand::<7>());
    let next_target = add_naive(pc.out, one_8);
    let next_target = no_jmp_enable.expand() & next_target.sum;

    let new_pc = offset_target | (long_target | next_target);
    pc.set_in(new_pc);

    return pc.out;
}

#[test]
fn test_pc() {
    use crate::*;
    clear_all();

    let jmp_offset_enable = input();
    let jmp_offset = input_w::<4>();
    let jmp_long_enable = input();
    let jmp_long = input_w::<4>();
    let no_jmp_enable = input();

    let offset = |v: u8| {
        jmp_offset_enable.set(1);
        jmp_long_enable.set(0);
        no_jmp_enable.set(0);

        jmp_offset.set_u8(v);
    };
    let long = |v: u8| {
        jmp_offset_enable.set(0);
        jmp_long_enable.set(1);
        no_jmp_enable.set(0);

        jmp_long.set_u8(v);
    };
    let next = || {
        jmp_offset_enable.set(0);
        jmp_long_enable.set(0);
        no_jmp_enable.set(1);
    };

    let pc_out = pc(
        jmp_offset_enable,
        jmp_offset,
        jmp_long_enable,
        jmp_long,
        no_jmp_enable,
    );

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
        let result_pc = pc_out.get_u8();
        println!(" => ref {}, result {}", reference_pc, result_pc);

        assert_eq!(result_pc as i32, reference_pc);
    }
}
