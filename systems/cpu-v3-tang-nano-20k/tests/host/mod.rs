//! Bounded host regressions in one integration executable.
#[path = "platform/boot.rs"]
mod boot;
#[path = "platform/cache_maintenance_wiring.rs"]
mod cache_maintenance_wiring;
#[path = "../support/checked_flat_fixture.rs"]
mod checked_flat_fixture;
#[path = "input/dualshock2_ui.rs"]
mod dualshock2_ui;
#[path = "platform/fetch_pause.rs"]
mod fetch_pause;
#[path = "input/gamepad.rs"]
mod gamepad;
#[path = "apps/gpu_demo_first_person.rs"]
mod gpu_demo_first_person;
#[path = "apps/gpu_demo_retained_controls.rs"]
mod gpu_demo_retained_controls;
#[path = "gpu/gpu_pipeline.rs"]
mod gpu_pipeline;
#[path = "gpu/gpu_rcc_builder.rs"]
mod gpu_rcc_builder;
#[path = "gpu/gpu_rcc_driver.rs"]
mod gpu_rcc_driver;
#[path = "gpu/gpu_rcc_present_control.rs"]
mod gpu_rcc_present_control;
#[path = "gpu/gpu_rcc_ui.rs"]
mod gpu_rcc_ui;
#[path = "platform/host_io.rs"]
mod host_io;
#[path = "apps/public_handheld.rs"]
mod public_handheld;
#[path = "compiler/rcc_modules.rs"]
mod rcc_modules;
#[path = "../../build_support/rcc_modules.rs"]
#[allow(dead_code)]
mod shared_build_support_rcc_modules;
#[path = "../support/gpu_control_peer.rs"]
mod shared_support_gpu_control_peer;
#[path = "../support/gpu_rcc_d2.rs"]
#[allow(dead_code)]
mod shared_support_gpu_rcc_d2;
#[path = "input/stick.rs"]
mod stick;
#[path = "gpu/system_display_board.rs"]
mod system_display_board;
#[path = "apps/tankracer_terrain.rs"]
mod tankracer_terrain;
#[path = "gpu/time_device.rs"]
mod time_device;

#[path = "compiler/ui_text.rs"]
mod ui_text;

#[path = "compiler/gpu_api_bind.rs"]
mod gpu_api_bind;
#[path = "compiler/gpu_api_config.rs"]
mod gpu_api_config;
#[path = "compiler/gpu_api_execute.rs"]
mod gpu_api_execute;
#[path = "compiler/gpu_api_facts.rs"]
mod gpu_api_facts;
#[path = "compiler/gpu_api_fault.rs"]
mod gpu_api_fault;
#[path = "compiler/gpu_api_g0.rs"]
mod gpu_api_g0;
#[path = "compiler/gpu_api_maintenance.rs"]
mod gpu_api_maintenance;
#[path = "compiler/gpu_api_mesh.rs"]
mod gpu_api_mesh;
#[path = "compiler/gpu_api_packet.rs"]
mod gpu_api_packet;
#[path = "compiler/gpu_api_present.rs"]
mod gpu_api_present;
#[path = "compiler/gpu_api_projection_boundary.rs"]
mod gpu_api_projection_boundary;
#[path = "compiler/gpu_api_record.rs"]
mod gpu_api_record;
#[path = "compiler/gpu_api_resources.rs"]
mod gpu_api_resources;
#[path = "compiler/gpu_api_ui_packet.rs"]
mod gpu_api_ui_packet;

#[path = "compiler/gpu_api_integration.rs"]
mod gpu_api_integration;

#[path = "compiler/gpu_api_pixels.rs"]
mod gpu_api_pixels;
#[path = "gpu/gpu_functional.rs"]
mod gpu_functional;

#[path = "compiler/gpu_api_scene.rs"]
mod gpu_api_scene;

#[path = "compiler/gpu_api_forwarding.rs"]
mod gpu_api_forwarding;

#[path = "../../build_support/application_entry.rs"]
mod shared_application_entry;

#[path = "compiler/gpu_api_imports.rs"]
mod gpu_api_imports;

#[path = "input/xinput_inverse.rs"]
mod xinput_inverse;

#[path = "compiler/gpu_api_model_cache.rs"]
mod gpu_api_model_cache;

#[path = "compiler/gpu_api_replay.rs"]
mod gpu_api_replay;

#[path = "platform/fixed_depth.rs"]
mod fixed_depth;

#[path = "../support/cpu_memory/mod.rs"]
mod shared_cpu_memory;
mod support_contracts;

#[path = "../boot/boot_address_spaces.rs"]
mod boot_address_spaces;

#[path = "compiler/static_assets.rs"]
mod static_assets;

#[path = "compiler/const_math.rs"]
mod const_math;

#[path = "compiler/workflow_diagnostics.rs"]
mod workflow_diagnostics;
