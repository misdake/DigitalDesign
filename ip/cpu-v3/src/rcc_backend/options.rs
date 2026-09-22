use rcc::{Opts, RccConfig};

/// CpuV3 target and ABI options.
#[derive(Clone, Debug)]
pub struct CompilerOptions {
    pub opt: Opts,
    pub stack_init: u16,
    pub data_base: u16,
    pub code_base: u16,
    /// Code and ordinary data use different physical segments. This permits
    /// their 16-bit offset ranges to overlap.
    pub separate_code_data_segments: bool,
    pub heap_begin: u16,
    pub heap_size: u16,
    pub vec_init_cap: u16,
}

impl Default for CompilerOptions {
    fn default() -> Self {
        Self {
            opt: Opts::default(),
            stack_init: crate::DEFAULT_STACK_TOP,
            data_base: crate::DEFAULT_DATA_BASE,
            code_base: 0,
            separate_code_data_segments: false,
            heap_begin: 0x8000,
            heap_size: 0x6000,
            vec_init_cap: 4,
        }
    }
}

impl CompilerOptions {
    /// Layout for an application entered with distinct CSEG and DSEG values.
    ///
    /// The first three 32-KiB data pages form one 96-KiB logical arena: 16 KiB
    /// is reserved for statics, almost 64 KiB for the current boundary-tag
    /// heap, and about 16 KiB for a stack growing down from `0xc000`.
    pub fn for_separate_code_and_data_segments(code_base: u16) -> Self {
        Self {
            stack_init: 0xc000,
            data_base: 0,
            code_base,
            separate_code_data_segments: true,
            heap_begin: 0x2000,
            heap_size: 0x7fff,
            ..Self::default()
        }
    }
}

impl RccConfig for CompilerOptions {
    fn optimizations(&self) -> &Opts {
        &self.opt
    }
    fn data_base(&self) -> u16 {
        self.data_base
    }
    fn heap_begin(&self) -> u16 {
        self.heap_begin
    }
    fn heap_size(&self) -> u16 {
        self.heap_size
    }
    fn vec_init_cap(&self) -> u16 {
        self.vec_init_cap
    }
}
