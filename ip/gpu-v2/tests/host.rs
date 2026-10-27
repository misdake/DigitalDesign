//! Bounded host regressions in one integration executable.
#[path = "host/basecolor_uv12.rs"]
mod basecolor_uv12;
#[path = "host/direct_lod.rs"]
mod direct_lod;
#[path = "host/framebuffer.rs"]
mod framebuffer;
#[path = "host/framebuffer_cache_emu.rs"]
mod framebuffer_cache_emu;
#[path = "host/framebuffer_leaf_actual.rs"]
mod framebuffer_leaf_actual;
#[path = "host/geometry_record_transport.rs"]
mod geometry_record_transport;
#[path = "host/gw2ar_write_contract.rs"]
mod gw2ar_write_contract;
#[path = "host/lighting.rs"]
mod lighting;
#[path = "host/lighting_architecture.rs"]
mod lighting_architecture;
#[path = "host/lighting_compact.rs"]
mod lighting_compact;
#[path = "host/lighting_compact_schedule.rs"]
mod lighting_compact_schedule;
#[path = "host/lighting_compensation.rs"]
mod lighting_compensation;
#[path = "host/lighting_four_cycle.rs"]
mod lighting_four_cycle;
#[path = "host/lighting_pipeline.rs"]
mod lighting_pipeline;
#[path = "host/lighting_rsqrt.rs"]
mod lighting_rsqrt;
#[path = "host/lighting_scalar.rs"]
mod lighting_scalar;
#[path = "host/lighting_square_boundary.rs"]
mod lighting_square_boundary;
#[path = "host/lighting_system.rs"]
mod lighting_system;
#[path = "host/lighting_two_cycle.rs"]
mod lighting_two_cycle;
#[path = "host/logic_cones.rs"]
mod logic_cones;
#[path = "host/memory_burst.rs"]
mod memory_burst;
#[path = "host/oracle_chain.rs"]
mod oracle_chain;
#[path = "host/oracle_parallel.rs"]
mod oracle_parallel;
#[path = "host/physical.rs"]
mod physical;
#[path = "host/pixel_backend_shared_mc.rs"]
mod pixel_backend_shared_mc;
#[path = "host/pixel_branches.rs"]
mod pixel_branches;
#[path = "host/pixel_dispatch_overnight.rs"]
mod pixel_dispatch_overnight;
#[path = "host/pixel_dispatch_shared_clock.rs"]
mod pixel_dispatch_shared_clock;
#[path = "host/pixel_engines_overnight.rs"]
mod pixel_engines_overnight;
#[path = "host/pixel_final_stage.rs"]
mod pixel_final_stage;
#[path = "host/pixel_lighting_live.rs"]
mod pixel_lighting_live;
#[path = "host/pixel_raster_ingress.rs"]
mod pixel_raster_ingress;
#[path = "host/pixel_registered_backend.rs"]
mod pixel_registered_backend;
#[path = "host/pixel_sdram.rs"]
mod pixel_sdram;
#[path = "host/pixel_shared_sdram.rs"]
mod pixel_shared_sdram;
#[path = "host/pixel_system.rs"]
mod pixel_system;
#[path = "host/quad_authority_model.rs"]
mod quad_authority_model;
#[path = "host/record_raster_pixel.rs"]
mod record_raster_pixel;
#[path = "host/render_precision.rs"]
mod render_precision;
#[path = "host/scheduler.rs"]
mod scheduler;
#[path = "host/sdram_display_load.rs"]
mod sdram_display_load;
#[path = "common/basecolor_uv12_fixture.rs"]
#[allow(dead_code)]
mod shared_common_basecolor_uv12_fixture;
#[path = "support/mod.rs"]
#[allow(dead_code)]
mod shared_support_mod;
#[path = "support/pixel.rs"]
#[allow(dead_code)]
mod shared_support_pixel;
#[path = "support/record_raster.rs"]
#[allow(dead_code)]
mod shared_support_record_raster;
#[path = "support/sdram/burst.rs"]
#[allow(dead_code)]
mod shared_support_sdram_burst;
#[path = "support/sdram/cycle.rs"]
#[allow(dead_code)]
mod shared_support_sdram_cycle;
#[path = "support/sdram/mod.rs"]
#[allow(dead_code)]
mod shared_support_sdram_mod;
#[path = "support/sdram/physical_texture.rs"]
#[allow(dead_code)]
mod shared_support_sdram_physical_texture;
#[path = "support/sdram/shared_pixel.rs"]
#[allow(dead_code)]
mod shared_support_sdram_shared_pixel;
#[path = "support/source_record_lease.rs"]
#[allow(dead_code)]
mod shared_support_source_record_lease;
#[path = "support/source_record_raster_producer.rs"]
#[allow(dead_code)]
mod shared_support_source_record_raster_producer;
#[path = "support/texture.rs"]
#[allow(dead_code)]
mod shared_support_texture;
#[path = "support/texture_capture.rs"]
#[allow(dead_code)]
mod shared_support_texture_capture;
#[path = "support/triangle.rs"]
mod shared_support_triangle;
#[path = "host/source_record_raster_pixel.rs"]
mod source_record_raster_pixel;
#[path = "host/source_record_transport.rs"]
mod source_record_transport;
#[path = "host/texture.rs"]
mod texture;
#[path = "host/texture_bound.rs"]
mod texture_bound;
#[path = "host/texture_cache_actual.rs"]
mod texture_cache_actual;
#[path = "host/texture_coefficient.rs"]
mod texture_coefficient;
#[path = "host/texture_coefficient_fixed.rs"]
mod texture_coefficient_fixed;
#[path = "host/texture_color_emu.rs"]
mod texture_color_emu;
#[path = "host/texture_compact_coefficient.rs"]
mod texture_compact_coefficient;
#[path = "host/texture_coordinate.rs"]
mod texture_coordinate;
#[path = "host/texture_counted.rs"]
mod texture_counted;
#[path = "host/texture_packet_pool.rs"]
mod texture_packet_pool;
#[path = "host/texture_preparation_handoff.rs"]
mod texture_preparation_handoff;
#[path = "host/texture_runtime.rs"]
mod texture_runtime;
#[path = "host/texture_runtime_ingress.rs"]
mod texture_runtime_ingress;
#[path = "host/texture_sampler_emu.rs"]
mod texture_sampler_emu;
#[path = "host/texture_sampling_step.rs"]
mod texture_sampling_step;
#[path = "host/texture_sdram.rs"]
mod texture_sdram;
#[path = "host/texture_serial_coefficient.rs"]
mod texture_serial_coefficient;
#[path = "host/texture_serial_preparation.rs"]
mod texture_serial_preparation;
#[path = "host/texture_staged.rs"]
mod texture_staged;
#[path = "host/texture_timed.rs"]
mod texture_timed;
#[path = "host/timed.rs"]
mod timed;
#[path = "host/triangle.rs"]
mod triangle;
#[path = "host/vertex_selected_binding.rs"]
mod vertex_selected_binding;
#[path = "host/vliw_backend.rs"]
mod vliw_backend;
#[path = "host/vliw_sdram.rs"]
mod vliw_sdram;
#[path = "host/weight_lifetime.rs"]
mod weight_lifetime;

use shared_support_record_raster as rr;
