//! FPU instruction field encodings.
/// Word0 opcode field, bits [15:12].
pub(crate) const OPCODE_VECTOR: u8 = 0xC;
pub(crate) const OPCODE_SCALAR: u8 = 0xD;
pub(crate) const OPCODE_AUX: u8 = 0xE;

/// Vector word1 subop field, bits [7:3] (section 6.3).
pub(crate) const VADD: u8 = 0x00;
pub(crate) const VSUB: u8 = 0x01;
pub(crate) const VMUL: u8 = 0x02;
pub(crate) const VMULS: u8 = 0x03;
pub(crate) const VMIN: u8 = 0x04;
pub(crate) const VMAX: u8 = 0x05;
pub(crate) const VABS: u8 = 0x06;
pub(crate) const VNEG: u8 = 0x07;
pub(crate) const VMOV: u8 = 0x0C;
pub(crate) const DOT: u8 = 0x0D;
pub(crate) const DOTADD: u8 = 0x0E;
pub(crate) const DOTSTORE: u8 = 0x0F;

/// Scalar word1 subop field, bits [9:4] (section 7). Only the subops the
/// scalar, multiply and special paths act on are named here.
pub(crate) const MUL: u8 = 0x02;
pub(crate) const CMP: u8 = 0x0B;
pub(crate) const RCP: u8 = 0x0C;
pub(crate) const RSQRT: u8 = 0x0D;
/// SINCOS (Stage 7c): sin -> Fd, cos -> Fd+1. Owned by the special path.
pub(crate) const SINCOS: u8 = 0x0E;

/// AUX word1 subop field, bits [9:4], for kind 00 (section 10.2).
pub(crate) const FLD: u8 = 0x00;
pub(crate) const FST: u8 = 0x01;

/// Word0 opcode, bits [15:12].
pub(crate) fn opcode(word0: u16) -> u8 {
    (word0 >> 12) as u8
}

/// Vector/scalar word0 Fa, bits [11:6].
pub(crate) fn word0_fa(word0: u16) -> u8 {
    ((word0 >> 6) & 0x3F) as u8
}

/// Vector/scalar word0 Fb, bits [5:0].
pub(crate) fn word0_fb(word0: u16) -> u8 {
    (word0 & 0x3F) as u8
}

/// AUX word0 Fa, bits [7:2] (bits [1:0] carry the kind).
pub(crate) fn aux_fa(word0: u16) -> u8 {
    ((word0 >> 2) & 0x3F) as u8
}

/// AUX word0 integer register index X, bits [11:8].
pub(crate) fn aux_x(word0: u16) -> u8 {
    ((word0 >> 8) & 0xF) as u8
}

/// AUX word0 kind, bits [1:0].
pub(crate) fn aux_kind(word0: u16) -> u8 {
    (word0 & 0x3) as u8
}

/// Word1 Fd, bits [15:10].
pub(crate) fn word1_fd(word1: u16) -> u8 {
    ((word1 >> 10) & 0x3F) as u8
}

/// Word1 vector length, bits [9:8].
pub(crate) fn word1_len(word1: u16) -> u8 {
    ((word1 >> 8) & 0x3) as u8
}

/// Word1 vector subop, bits [7:3].
pub(crate) fn vector_subop(word1: u16) -> u8 {
    ((word1 >> 3) & 0x1F) as u8
}

/// Word1 scalar or AUX subop, bits [9:4].
pub(crate) fn scalar_subop(word1: u16) -> u8 {
    ((word1 >> 4) & 0x3F) as u8
}

/// Word1 mode field, bits [3:0].
pub(crate) fn word1_mode(word1: u16) -> u8 {
    (word1 & 0xF) as u8
}

/// Vector len -> last-lane index; len 11 is reserved and clamps to vec4.
pub(crate) fn decoded_last_lane(len_field: u8) -> u8 {
    if len_field == 3 {
        3
    } else {
        len_field + 1
    }
}
