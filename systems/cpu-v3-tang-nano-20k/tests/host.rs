//! Bounded host regressions in one integration executable.
#[path = "host/boot.rs"]
mod boot;
#[path = "host/cache_maintenance_wiring.rs"]
mod cache_maintenance_wiring;
#[path = "host/gpu_rcc_builder.rs"]
mod gpu_rcc_builder;
#[path = "host/gpu_rcc_driver.rs"]
mod gpu_rcc_driver;
#[path = "host/gpu_rcc_present_control.rs"]
mod gpu_rcc_present_control;
#[path = "host/gpu_rcc_ui.rs"]
mod gpu_rcc_ui;
#[path = "host/rcc_modules.rs"]
mod rcc_modules;
#[path = "../build_support/rcc_modules.rs"]
#[allow(dead_code)]
mod shared_build_support_rcc_modules;
#[path = "support/gpu_rcc_d2.rs"]
#[allow(dead_code)]
mod shared_support_gpu_rcc_d2;
#[path = "host/time_device.rs"]
mod time_device;
