use crate::{add_naive, flatten2, input_const, reg_w, Wire, Wires};

fn pc(new_pc: Wires<8>) -> Wires<8> {
    let mut pc = reg_w::<8>();
    pc.set_in(new_pc);
    pc.out
}

fn new_pc(
    jmp_offset_enable: Wire,
    jmp_offset: Wires<4>,
    jmp_long_enable: Wire,
    jmp_long_target: Wires<4>,
    no_jmp_enable: Wire,
    old_pc: Wires<8>,
) -> Wires<8> {
    let offset_target = add_naive(old_pc, jmp_offset.expand_signed::<8>());
    let offset_target = jmp_offset_enable.expand() & offset_target.sum;

    let zero = input_const(0);
    let long_target: Wires<8> = flatten2(jmp_long_target, zero.expand::<4>());
    let long_target = jmp_long_enable.expand() & long_target;

    let one = input_const(1);
    let one_8: Wires<8> = flatten2(zero.expand::<7>(), one.expand::<1>());
    let next_target = add_naive(old_pc, one_8);
    let next_target = no_jmp_enable.expand() & next_target.sum;

    return offset_target | (long_target | next_target);
}
