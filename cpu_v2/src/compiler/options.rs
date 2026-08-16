//! compile-time options: optimization flags, memory layout, library params.

use crate::compiler::Opts;

/// Selection policy for the ISA's 256-entry `call_abs` table at
/// `0xff00..=0xffff`. The compiler emits a startup initializer for selected
/// entries, so compiled images remain self-contained.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FunctionTableConfig {
    Disabled,
    /// Select profitable repeated calls plus recursive/loop call targets.
    Auto,
    /// Put every directly-called reachable function in the table (up to 256).
    All,
    /// Put the named directly-called reachable functions in the table.
    Functions(Vec<String>),
}

/// everything a compile can be tuned with; `Compiler::default()` uses these
#[derive(Clone, Debug)]
pub struct CompilerOptions {
    /// optimization passes (const-prop / cse / dce / coalesce)
    pub opt: Opts,
    /// initial stack pointer value for the entry function (0 = keep the
    /// simulator's default of 0, i.e. the stack wraps to the top of memory).
    /// stack direction is ISA-fixed: frames grow downward from sp.
    pub stack_init: u16,
    /// data section base address for static data (spec §9)
    pub data_base: u16,
    /// linked code offset inside a G16 code segment; ignored by v2.6
    pub code_base: u16,
    /// heap region start (used by the rcc_std heap library and its auto-init)
    pub heap_begin: u16,
    /// heap region size in words
    pub heap_size: u16,
    /// initial capacity used by rcc_std Vec's vec_new
    pub vec_init_cap: u16,
    /// direct-call function table policy
    pub function_table: FunctionTableConfig,
}

impl Default for CompilerOptions {
    fn default() -> Self {
        Self {
            opt: Opts::default(),
            stack_init: 0,
            data_base: 0,
            code_base: 0,
            heap_begin: 0x1000,
            heap_size: 20,
            vec_init_cap: 4,
            function_table: FunctionTableConfig::Auto,
        }
    }
}

impl CompilerOptions {
    /// Safe baseline for G16's unified address space.
    ///
    /// Code grows upward from zero, static data begins at 0x4000, and the
    /// downward-growing stack stops below the 0xff00 MMIO page.
    pub fn g16() -> Self {
        Self {
            stack_init: crate::g16::DEFAULT_STACK_TOP,
            data_base: crate::g16::DEFAULT_DATA_BASE,
            heap_begin: 0x8000,
            function_table: FunctionTableConfig::Disabled,
            ..Self::default()
        }
    }
}
