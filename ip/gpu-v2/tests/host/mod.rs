//! Bounded host regressions in one integration executable.
#[path = "texture/basecolor_uv12.rs"]
mod basecolor_uv12;
#[path = "integration/component_oracles.rs"]
mod component_oracles;
#[path = "geometry/direct_lod.rs"]
mod direct_lod;
#[path = "framebuffer/framebuffer.rs"]
mod framebuffer;
#[path = "framebuffer/framebuffer_cache_emu.rs"]
mod framebuffer_cache_emu;
#[path = "framebuffer/framebuffer_leaf_actual.rs"]
mod framebuffer_leaf_actual;
#[path = "geometry/geometry_record_transport.rs"]
mod geometry_record_transport;
#[path = "memory/gw2ar_write_contract.rs"]
mod gw2ar_write_contract;
#[path = "lighting/lighting.rs"]
mod lighting;
#[path = "lighting/lighting_architecture.rs"]
mod lighting_architecture;
#[path = "lighting/lighting_compact.rs"]
mod lighting_compact;
#[path = "lighting/lighting_compact_schedule.rs"]
mod lighting_compact_schedule;
#[path = "lighting/lighting_compensation.rs"]
mod lighting_compensation;
#[path = "lighting/lighting_four_cycle.rs"]
mod lighting_four_cycle;
#[path = "lighting/lighting_pipeline.rs"]
mod lighting_pipeline;
#[path = "lighting/lighting_rsqrt.rs"]
mod lighting_rsqrt;
#[path = "lighting/lighting_scalar.rs"]
mod lighting_scalar;
#[path = "lighting/lighting_square_boundary.rs"]
mod lighting_square_boundary;
#[path = "lighting/lighting_system.rs"]
mod lighting_system;
#[path = "lighting/lighting_two_cycle.rs"]
mod lighting_two_cycle;
#[path = "scheduling/logic_cones.rs"]
mod logic_cones;
#[path = "memory/memory_burst.rs"]
mod memory_burst;
#[path = "memory/physical.rs"]
mod physical;
#[path = "pixel/pixel_backend_shared_mc.rs"]
mod pixel_backend_shared_mc;
#[path = "pixel/pixel_branches.rs"]
mod pixel_branches;
#[path = "pixel/pixel_dispatch_overnight.rs"]
mod pixel_dispatch_overnight;
#[path = "pixel/pixel_dispatch_shared_clock.rs"]
mod pixel_dispatch_shared_clock;
#[path = "pixel/pixel_engines_overnight.rs"]
mod pixel_engines_overnight;
#[path = "pixel/pixel_final_stage.rs"]
mod pixel_final_stage;
#[path = "pixel/pixel_lighting_live.rs"]
mod pixel_lighting_live;
#[path = "pixel/pixel_raster_ingress.rs"]
mod pixel_raster_ingress;
#[path = "pixel/pixel_registered_backend.rs"]
mod pixel_registered_backend;
#[path = "pixel/pixel_sdram.rs"]
mod pixel_sdram;
#[path = "pixel/pixel_shared_sdram.rs"]
mod pixel_shared_sdram;
#[path = "pixel/pixel_system.rs"]
mod pixel_system;
#[path = "pixel/quad_authority_model.rs"]
mod quad_authority_model;
#[path = "pixel/record_raster_pixel.rs"]
mod record_raster_pixel;
#[path = "geometry/render_precision.rs"]
mod render_precision;
#[path = "scheduling/scheduler.rs"]
mod scheduler;
#[path = "memory/sdram_display_load.rs"]
mod sdram_display_load;
#[path = "../fixtures/basecolor_uv12_fixture.rs"]
#[allow(dead_code)]
mod shared_common_basecolor_uv12_fixture;
#[path = "../support/mod.rs"]
#[allow(dead_code)]
mod shared_support_mod;
#[path = "../support/pixel.rs"]
#[allow(dead_code)]
mod shared_support_pixel;
#[path = "../support/record_raster.rs"]
#[allow(dead_code)]
mod shared_support_record_raster;
#[path = "../support/sdram/burst.rs"]
#[allow(dead_code)]
mod shared_support_sdram_burst;
#[path = "../support/sdram/cycle.rs"]
#[allow(dead_code)]
mod shared_support_sdram_cycle;
#[path = "../support/sdram/mod.rs"]
#[allow(dead_code)]
mod shared_support_sdram_mod;
#[path = "../support/sdram/physical_texture.rs"]
#[allow(dead_code)]
mod shared_support_sdram_physical_texture;
#[path = "../support/sdram/shared_pixel.rs"]
#[allow(dead_code)]
mod shared_support_sdram_shared_pixel;
#[path = "../support/source_record_lease.rs"]
#[allow(dead_code)]
mod shared_support_source_record_lease;
#[path = "../support/source_record_raster_producer.rs"]
#[allow(dead_code)]
mod shared_support_source_record_raster_producer;
#[path = "../support/texture.rs"]
#[allow(dead_code)]
mod shared_support_texture;
#[path = "../support/texture_capture.rs"]
#[allow(dead_code)]
mod shared_support_texture_capture;
#[path = "../support/triangle.rs"]
mod shared_support_triangle;
#[path = "pixel/source_record_raster_pixel.rs"]
mod source_record_raster_pixel;
#[path = "integration/source_record_transport.rs"]
mod source_record_transport;
#[path = "integration/texture.rs"]
mod texture;
#[path = "texture/texture_bound.rs"]
mod texture_bound;
#[path = "texture/texture_cache_actual.rs"]
mod texture_cache_actual;
#[path = "texture/texture_coefficient.rs"]
mod texture_coefficient;
#[path = "texture/texture_coefficient_fixed.rs"]
mod texture_coefficient_fixed;
#[path = "texture/texture_color_emu.rs"]
mod texture_color_emu;
#[path = "texture/texture_compact_coefficient.rs"]
mod texture_compact_coefficient;
#[path = "texture/texture_coordinate.rs"]
mod texture_coordinate;
#[path = "texture/texture_counted.rs"]
mod texture_counted;
#[path = "texture/texture_packet_pool.rs"]
mod texture_packet_pool;
#[path = "texture/texture_preparation_handoff.rs"]
mod texture_preparation_handoff;
#[path = "texture/texture_runtime.rs"]
mod texture_runtime;
#[path = "texture/texture_runtime_ingress.rs"]
mod texture_runtime_ingress;
#[path = "texture/texture_sampler_emu.rs"]
mod texture_sampler_emu;
#[path = "texture/texture_sampling_step.rs"]
mod texture_sampling_step;
#[path = "texture/texture_sdram.rs"]
mod texture_sdram;
#[path = "texture/texture_serial_coefficient.rs"]
mod texture_serial_coefficient;
#[path = "texture/texture_serial_preparation.rs"]
mod texture_serial_preparation;
#[path = "texture/texture_staged.rs"]
mod texture_staged;
#[path = "texture/texture_timed.rs"]
mod texture_timed;
#[path = "integration/timed.rs"]
mod timed;
#[path = "geometry/triangle.rs"]
mod triangle;
#[path = "geometry/vertex_selected_binding.rs"]
mod vertex_selected_binding;
#[path = "pixel/vliw_backend.rs"]
mod vliw_backend;
#[path = "integration/vliw_sdram.rs"]
mod vliw_sdram;
#[path = "texture/weight_lifetime.rs"]
mod weight_lifetime;

use shared_support_record_raster as rr;

mod support_contracts;

#[path = "../command/command_payload_binding.rs"]
mod command_payload_binding;

#[path = "../command/source_capture.rs"]
mod source_capture;

#[path = "../command/ui_grid_layout.rs"]
mod ui_grid_layout;

#[path = "../geometry/frontend.rs"]
mod frontend;

#[path = "../geometry/frontend_runtime.rs"]
mod frontend_runtime;

#[path = "../integration/functional.rs"]
mod functional;
