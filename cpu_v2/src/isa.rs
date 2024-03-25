#![allow(unused)]

use cpu_macro::define_isa;

pub type InstBinaryType = u16;
pub type Reg = u8;
pub type Imm4 = u8;
pub type Flag4 = u8;

fn part3(binary: InstBinaryType) -> u8 {
    ((binary >> 12) & 0b1111) as u8
}
fn part2(binary: InstBinaryType) -> u8 {
    ((binary >> 8) & 0b1111) as u8
}
fn part1(binary: InstBinaryType) -> u8 {
    ((binary >> 4) & 0b1111) as u8
}
fn part0(binary: InstBinaryType) -> u8 {
    (binary & 0b1111) as u8
}

define_isa! {
    Instruction

    // 0x00 triggers halt?
    (mov  0x01 OORR "r{0} = r{1}")
    (inv  0x02 OORR "r{0} = !r{1}")
    (neg  0x03 OORR "r{0} = -r{1}")
    (not0 0x04 OORR "r{0} = !!r{1}")
    (cnt1 0x05 OORR "r{0} = cnt1(r{1})")
    (log2 0x06 OORR "r{0} = log2(r{1})")
    (cmp_i 0x07 OOIR "flags = flags(r{0} - {1})")
    (cmp_r 0x08 OORR "flags = flags(r{0} - r{1})")

    (and  0x8 ORRR "r{0} = r{1} & r{2}")
    (or   0x9 ORRR "r{0} = r{1} | r{2}")
    (xor  0xa ORRR "r{0} = r{1} ^ r{2}")
    (add  0xb ORRR "r{0} = r{1} + r{2}")
    (sub  0xc ORRR "r{0} = r{1} - r{2}")
    (addi 0xd OIRR "r{0} = r{1} + i4(0x{2:x})")
    (lsl  0xe OIRR "r{0} = r{1} << {2}")
    (lsr  0xf OIRR "r{0} = r{1} >> {2}")

    (load_hi 0x1 OIIR "r{0}_hi = 0x{2:x}{1:x}")
    (load_lo 0x2 OIIR "r{0}_lo = 0x{2:x}{1:x}")

    (store_mem     0x30 OORR "mem[r{1}] = r{0}")
    (load_mem      0x31 OORR "r{0} = mem[r{1}]")
    (stack_push    0x32 OOXR "mem[--sp] = r{0}")
    (stack_pop     0x33 OOXR "r{0} = mem[sp++]")
    (stack_write   0x34 OOIR "mem[sp + {1}] = r{0}")
    (stack_read    0x35 OOIR "r{0} = mem[sp + {1}]")
    (stack_push_pc 0x36 OOXX "mem[--sp] = pc")
    (stack_pop_pc  0x37 OOXX "pc = mem[sp++]")

    (sp_set_r  0x38 OOIR "sp  = r{0} + i4(0x{1:x})")
    (sp_add_r  0x39 OOXR "sp += r{0}")
    (sp_inc_i  0x3a OOIX "sp += {0}")
    (sp_dec_i  0x3b OOIX "sp -= {0}")
    (sp_get_i  0x3c OOIR "r{0} = sp + {1}")
    (sp_get_r  0x3d OOIR "r{0} = sp + r{1}")

    (j_add_i 0x4  OIIF "if jmp({0:x}) pc += 0x{2:x}{1:x}")
    (j_add_r 0x50 OORF "if jmp({0:x}) pc += r{1:x}")
    (j_set_r 0x51 OORF "if jmp({0:x}) pc = r{1:x}")

    (dev_recv 0x6 OIIR "r{0} <- device[{2}].out[{1}]")
    (dev_send 0x7 OIIR "device[{2}].in[{1}] <- r{0}")
}

#[test]
fn test_print() {
    let inst = load_hi(0x2, 0x1, 0);
    println!("inst: {}\nbinary: {:4x}", inst, inst.encode());
}
