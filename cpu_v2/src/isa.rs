#![allow(unused)]

use cpu_macro::define_isa;

pub type InstBinaryType = u16;
pub type Reg = u8;
pub type Imm4 = u8;

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

    (halt  0x00 OOOO "halt")
    (mov   0x01 OORR "r{0} = r{1}")
    (inv   0x02 OORR "r{0} = !r{1}")
    (neg   0x03 OORR "r{0} = -r{1}")
    (not0  0x04 OORR "r{0} = !!r{1}")
    (cnt1  0x05 OORR "r{0} = cnt1(r{1})")
    (log2  0x06 OORR "r{0} = log2(r{1})")
    (cmp_r 0x07 OORR "flags = flags(r{0} - r{1})") // unsigned? signed?
    (lsl   0x08 OOIR "r{0} = r{0} << {1}")
    (lsr   0x09 OOIR "r{0} = r{0} >> {1}")
    (asr   0x0a OOIR "r{0} = r{0} >>> {1}")
    // 0x0b
    // 0x0c
    // 0x0d
    (pc    0x0e OOIR "r{0} = pc + i4(0x{1:x})")
    (cmp_i 0x0f OOIR "flags = flags(r{0} - {1})") // unsigned? signed?

    (and  0x8 ORRR "r{0} = r{2} & r{1}")
    (or   0x9 ORRR "r{0} = r{2} | r{1}")
    (xor  0xa ORRR "r{0} = r{2} ^ r{1}")
    (add  0xb ORRR "r{0} = r{2} + r{1}")
    (sub  0xc ORRR "r{0} = r{2} - r{1}")
    (addi 0xd ORIR "r{0} = r{2} + {1}")
    (subi 0xe ORIR "r{0} = r{2} - {1}")
    // 0xf

    (load_hi 0x1 OIIR "r{0} hi = 0x{2:x}{1:x}") // hi, lo, reg
    (load_lo 0x2 OIIR "r{0} = 0x{2:x}{1:x}")    // hi, lo, reg, clears high 8 bits

    (store_mem 0x3 ORIR "mem[r{2} + {1}] = r{0}")
    (load_mem  0x4 ORIR "r{0} = mem[r{2} + {1}]")

    // 0x50~0x53 is 0x54~0x57 inverted
    (j_offset_g  0x51 OOII "jg  pc + 0x{0:x}{1:x}") // lo, hi
    (j_offset_e  0x52 OOII "je  pc + 0x{0:x}{1:x}") // lo, hi
    (j_offset_l  0x53 OOII "jl  pc + 0x{0:x}{1:x}") // lo, hi
    (j_offset    0x54 OOII "jmp pc + 0x{0:x}{1:x}") // lo, hi
    (j_offset_le 0x55 OOII "jle pc + 0x{0:x}{1:x}") // lo, hi
    (j_offset_ne 0x56 OOII "jne pc + 0x{0:x}{1:x}") // lo, hi
    (j_offset_ge 0x57 OOII "jge pc + 0x{0:x}{1:x}") // lo, hi
    (jmp_reg     0x5e OORX "jmp r{0}")
    (call_reg    0x5f OORR "call r{1} (pc save r{0})")

    (dev_recv 0x6 OIIR "r{0} <- device[{2}].out[{1}]")
    (dev_send 0x7 OIIR "device[{2}].in[{1}] <- r{0}")
}

pub const FLAGS_GREATER: u8 = 1 << 0;
pub const FLAGS_EQUAL: u8 = 1 << 1;
pub const FLAGS_LESS: u8 = 1 << 2;
#[repr(u8)]
#[derive(Copy, Clone, Debug)]
pub enum Cond {
    Never = 0,
    Greater = FLAGS_GREATER,
    Equal = FLAGS_EQUAL,
    Less = FLAGS_LESS,
    GreaterEqual = FLAGS_GREATER | FLAGS_EQUAL,
    LessEqual = FLAGS_LESS | FLAGS_EQUAL,
    NotEqual = FLAGS_GREATER | FLAGS_LESS,
    Always = FLAGS_GREATER | FLAGS_EQUAL | FLAGS_LESS,
}

#[test]
fn test_print() {
    let inst = load_hi(0x2, 0x1, 0);
    println!("inst: {}\nbinary: {:4x}", inst, inst.encode());
}
