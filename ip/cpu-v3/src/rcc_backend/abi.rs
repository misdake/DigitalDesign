//! Target register ABI shared by lowering and allocation.

pub(super) const REG_TMP: u8 = 12;
pub(super) const REG_SP: u8 = 13;
pub(super) const REG_LINK: u8 = 14;
/// Reserved FPU register for breaking parallel-move cycles (never allocated).
/// The FPU v2 convention reserves `F63` as the parallel-move scratch.
pub(super) const FPU_SCRATCH: u8 = 63;

/// FPU v2 argument registers: `F4..F27`, i.e. six `vec4` values placed
/// compactly after the `F0..F3` return area (design `fpu-design-v2` todo C0).
pub(super) const FPU_ARGUMENT_REGISTERS: &[u8] = &[
    4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27,
];

/// FPU v2 ordinary allocation area: `F28..F62`. `F0..F3` are reserved for
/// returns, `F4..F27` for arguments, and `F63` for parallel-move cycles.
pub(super) const FPU_ALLOCATABLE_REGISTERS: &[u8] = &[
    28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51,
    52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62,
];

pub(super) const CPU_V3_REGISTER_CONVENTION: rcc::RegisterConvention = rcc::RegisterConvention {
    return_registers: &[0, 1],
    argument_registers: &[2, 3, 4, 5, 6, 7],
    allocatable_registers: &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 15],
    caller_saved: &[0, 1, 2, 3, 4, 5, 6, 7, 15],
    callee_saved: &[8, 9, 10, 11],
    link_register: REG_LINK,
    stack_register: REG_SP,
    temporary_register: REG_TMP,
    maximum_frame_words: i16::MAX as usize,
    // FPU v2 ABI (Q16.16, contiguous scalar ranges): F0..F3 returns, F4..F27
    // arguments placed compactly, F28..F62 ordinary allocation, F63 the
    // parallel-move scratch. All FPU registers are caller-saved and ACC is
    // caller-clobbered. C0 freezes this layout; range-aware allocation for
    // vec2/vec3/vec4 values lands with C2.
    fpu: Some(rcc::FpuRegisterConvention {
        return_registers: &[0, 1, 2, 3],
        argument_registers: FPU_ARGUMENT_REGISTERS,
        allocatable_registers: FPU_ALLOCATABLE_REGISTERS,
        scratch_register: FPU_SCRATCH,
    }),
};
