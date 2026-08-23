use rcc::{Opts, RccConfig};

/// CpuV3 target and ABI options.
#[derive(Clone, Debug)]
pub struct CompilerOptions {
    pub opt: Opts,
    pub stack_init: u16,
    pub data_base: u16,
    pub code_base: u16,
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
            heap_begin: 0x8000,
            heap_size: 20,
            vec_init_cap: 4,
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
