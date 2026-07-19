#![allow(unused)]

use cpu_macro::define_isa;

pub type InstBinaryType = u16;
pub type Reg = u8;
pub type Imm4 = u8;

/// hardwired link register (call_rel / call_abs / call_reg write pc+1)
pub const RA_REG: u8 = 13;
/// stack pointer (sp_add / sp_sub read and write it implicitly)
pub const SP_REG: u8 = 14;

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

// ISA v2.6, see isa.html (next to this file)
// i8/u8 across n1:n0 is hi:lo (n1 = high nibble).
// addi uses zero-less i4: raw 0..7 -> +1..+8, raw 8..f -> -8..-1.
define_isa! {
    Instruction

    (halt   0x00 OORX "halt r{0}")
    (mov    0x01 OORR "r{0} = r{1}")
    (inv    0x02 OORR "r{0} = !r{1}")
    (neg    0x03 OORR "r{0} = -r{1}")
    (not0   0x04 OORR "r{0} = !!r{1}")
    (cnt1   0x05 OORR "r{0} = cnt1(r{1})")
    (log2   0x06 OORR "r{0} = log2(r{1})")
    // 0x07 reserved
    (lsl    0x08 OOIR "r{0} = r{0} << {1}")
    (lsr    0x09 OOIR "r{0} = r{0} >> {1}")
    (asr    0x0a OOIR "r{0} = r{0} >>> {1}")
    // 0x0b reserved
    (sp_add 0x0c OOII "r14 += 0x{1:x}{0:x}") // hi, lo
    (sp_sub 0x0d OOII "r14 -= 0x{1:x}{0:x}") // hi, lo
    // 0x0e, 0x0f reserved

    (and  0x8 ORRR "r{0} = r{2} & r{1}")
    (or   0x9 ORRR "r{0} = r{2} | r{1}")
    (xor  0xa ORRR "r{0} = r{2} ^ r{1}")
    (add  0xb ORRR "r{0} = r{2} + r{1}")
    (sub  0xc ORRR "r{0} = r{2} - r{1}")
    (addi 0xd ORIR "r{0} = r{2} + i4nz(0x{1:x})")

    (load_hi 0x2 OIIR "r{0} hi = 0x{2:x}{1:x}") // hi, lo, reg
    (load_lo 0x3 OIIR "r{0} = 0x{2:x}{1:x}")    // hi, lo, reg, clears high 8 bits

    (store_mem 0x4 ORRI "mem[r{2} + {0}] = r{1}") // base, src, i4 offset
    (load_mem  0x5 ORIR "r{0} = mem[r{2} + {1}]") // base, i4 offset, dst

    (store_sp  0x6 OIIR "mem[r14 + 0x{2:x}{1:x}] = r{0}") // hi, lo, src; frame slot 0..255
    (load_sp   0x7 OIIR "r{0} = mem[r14 + 0x{2:x}{1:x}]") // hi, lo, dst; frame slot 0..255

    (pc       0x10 OOIR "r{0} = pc + i4(0x{1:x})")
    // j_cc: n2 = cond mask (G=1, E=2, L=4); mask=7 (jmp) always taken; i8 = hi:lo
    (jg       0x11 OOII "jg  pc + 0x{1:x}{0:x}") // hi, lo
    (je       0x12 OOII "je  pc + 0x{1:x}{0:x}")
    (jge      0x13 OOII "jge pc + 0x{1:x}{0:x}")
    (jl       0x14 OOII "jl  pc + 0x{1:x}{0:x}")
    (jne      0x15 OOII "jne pc + 0x{1:x}{0:x}")
    (jle      0x16 OOII "jle pc + 0x{1:x}{0:x}")
    (jmp      0x17 OOII "jmp pc + 0x{1:x}{0:x}")
    (cmp_r    0x18 OORR "flags = ucmp(r{0}, r{1})")
    (cmp_i    0x19 OOIR "flags = ucmp(r{0}, u4(0x{1:x}))")
    (cmp_s    0x1a OORR "flags = scmp(r{0}, r{1})")
    (cmp_si   0x1b OOIR "flags = scmp(r{0}, i4(0x{1:x}))")
    (call_rel 0x1c OOII "call_rel pc + 0x{1:x}{0:x} (r13)") // hi, lo
    (call_abs 0x1d OOII "call_abs 0x{1:x}{0:x} (r13)") // hi, lo
    (jmp_reg  0x1e OORX "jmp r{0}")
    (call_reg 0x1f OORX "call r{0} (r13)")

    (dev_recv 0xe OIIR "r{0} <- device[{2}].out[{1}]")
    (dev_send 0xf OIIR "device[{2}].in[{1}] <- r{0}")
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
    NotEqual = FLAGS_GREATER | FLAGS_LESS,
    LessEqual = FLAGS_LESS | FLAGS_EQUAL,
    Always = FLAGS_GREATER | FLAGS_EQUAL | FLAGS_LESS,
}
impl Cond {
    pub fn invert(self) -> Self {
        match self {
            Cond::Never => Cond::Always,
            Cond::Greater => Cond::LessEqual,
            Cond::Equal => Cond::NotEqual,
            Cond::Less => Cond::GreaterEqual,
            Cond::GreaterEqual => Cond::Less,
            Cond::NotEqual => Cond::Equal,
            Cond::LessEqual => Cond::Greater,
            Cond::Always => Cond::Never,
        }
    }
}

/// decode addi's zero-less i4 immediate: raw 0..7 -> +1..+8, raw 8..f -> -8..-1
pub fn imm4_nz(raw: Imm4) -> i16 {
    if raw < 8 {
        raw as i16 + 1
    } else {
        raw as i16 - 16
    }
}

/// encode a delta in -8..=-1 / 1..=8 into addi's raw zero-less i4 field
pub fn imm4_nz_encode(delta: i8) -> Imm4 {
    debug_assert!((-8..=8).contains(&delta) && delta != 0);
    if delta > 0 {
        (delta - 1) as u8
    } else {
        (16 + delta) as u8
    }
}

/// decode i4 as sign-extended i16
pub fn imm_as_i16(i4: Imm4) -> u16 {
    let sign_bit = (i4 & 0b1000) != 0;
    i4 as u16 | (0b1111_1111_1111_0000 * sign_bit as u16)
}

/// combine hi:lo nibbles into a u8, sign-extended to i16 bits
pub fn imm8_as_i16(hi: Imm4, lo: Imm4) -> u16 {
    (((hi << 4) | lo) as i8) as i16 as u16
}

/// combine hi:lo nibbles into a u16 (zero-extended u8)
pub fn hilo_as_u16(hi: Imm4, lo: Imm4) -> u16 {
    ((hi as u16) << 4) | (lo as u16)
}

#[test]
fn test_print() {
    let inst = load_hi(0x2, 0x1, 0);
    println!("inst: {}\nbinary: {:4x}", inst, inst.encode());
}
