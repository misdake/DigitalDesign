#![allow(incomplete_features)]
#![feature(generic_const_exprs)]

pub mod dsl_rt;
mod fixed_constants;
mod fixed_math;
pub mod frontend;
pub mod rcc_std;

mod compiler;

pub use compiler::*;

pub type FuncName = &'static str;

/// Target-independent rcc frontend and runtime configuration.
pub trait RccConfig {
    /// Explicit target check profile, independent of optimization and host cfg.
    fn gpu_debug(&self) -> bool {
        false
    }
    fn optimizations(&self) -> &Opts;
    fn data_base(&self) -> u16;
    fn heap_begin(&self) -> u16;
    fn heap_size(&self) -> u16;
    fn vec_init_cap(&self) -> u16;
    fn asset_banks(&self) -> &[frontend::static_assets::AssetBank] {
        &[]
    }
    fn static_data_limit(&self) -> usize {
        1 << 16
    }
    fn static_data_limit_name(&self) -> &'static str {
        "target memory"
    }
}

#[derive(Clone, Debug)]
pub struct RccOptions {
    pub gpu_debug: bool,
    pub opt: Opts,
    pub data_base: u16,
    pub heap_begin: u16,
    pub heap_size: u16,
    pub vec_init_cap: u16,
}

impl Default for RccOptions {
    fn default() -> Self {
        Self {
            gpu_debug: false,
            opt: Opts::default(),
            data_base: 0,
            heap_begin: 0x1000,
            heap_size: 20,
            vec_init_cap: 4,
        }
    }
}

impl RccConfig for RccOptions {
    fn gpu_debug(&self) -> bool {
        self.gpu_debug
    }
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
