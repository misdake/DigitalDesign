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

#[path = "host/gpu_api_bind.rs"]
mod gpu_api_bind;
#[path = "host/gpu_api_capture_source.rs"]
mod gpu_api_capture_source;
#[path = "host/gpu_api_config.rs"]
mod gpu_api_config;
#[path = "host/gpu_api_execute.rs"]
mod gpu_api_execute;
#[path = "host/gpu_api_fault.rs"]
mod gpu_api_fault;
#[path = "host/gpu_api_g0.rs"]
mod gpu_api_g0;
#[path = "host/gpu_api_maintenance.rs"]
mod gpu_api_maintenance;
#[path = "host/gpu_api_mesh.rs"]
mod gpu_api_mesh;
#[path = "host/gpu_api_packet.rs"]
mod gpu_api_packet;
#[path = "host/gpu_api_present.rs"]
mod gpu_api_present;
#[path = "host/gpu_api_projection_boundary.rs"]
mod gpu_api_projection_boundary;
#[path = "host/gpu_api_record.rs"]
mod gpu_api_record;
#[path = "host/gpu_api_resources.rs"]
mod gpu_api_resources;
#[path = "host/gpu_api_ui_packet.rs"]
mod gpu_api_ui_packet;

#[path = "host/gpu_api_integration.rs"]
mod gpu_api_integration;

#[path = "host/gpu_api_pixels.rs"]
mod gpu_api_pixels;
#[path = "host/gpu_functional.rs"]
mod gpu_functional;

#[path = "host/gpu_api_scene.rs"]
mod gpu_api_scene;

#[path = "host/gpu_api_forwarding.rs"]
mod gpu_api_forwarding;

#[path = "../build_support/application_entry.rs"]
mod shared_application_entry;

#[path = "host/gpu_api_imports.rs"]
mod gpu_api_imports;
