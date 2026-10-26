//! Bounded host regressions in one integration executable.
#[path = "host/boot.rs"]
mod boot;
#[path = "host/cache_maintenance_wiring.rs"]
mod cache_maintenance_wiring;
#[path = "host/dualshock2_ui.rs"]
mod dualshock2_ui;
#[path = "host/fetch_pause.rs"]
mod fetch_pause;
#[path = "host/gamepad.rs"]
mod gamepad;
#[path = "host/gpu_demo_first_person.rs"]
mod gpu_demo_first_person;
#[path = "host/gpu_demo_retained_controls.rs"]
mod gpu_demo_retained_controls;
#[path = "host/gpu_pipeline.rs"]
mod gpu_pipeline;
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
#[path = "host/stick.rs"]
mod stick;
#[path = "host/system_display_board.rs"]
mod system_display_board;
#[path = "host/time_device.rs"]
mod time_device;
