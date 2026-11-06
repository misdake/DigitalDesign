//! Selected command backend assembled from its actual physical components.
use crate::{
    framebuffer::ports::MaterializedSurface,
    lighting::{dsp_sharing, rtl as lighting},
    system::{
        pixel::{final_stage, framebuffer_tdp, quad_ingress, row_store, vliw_quad_pool},
        source::{module, Modules},
    },
    texture::rtl,
};

pub(crate) fn verilog(
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    if surface.width != 400 || surface.height != 240 {
        return Err("VLIW pool NDC/tile contract requires400x240".into());
    }
    let native = matches!(backend, quad_ingress::rtl::Backend::Native);
    let mut components = Modules::default();
    for source in [include_str!("backend.v"), include_str!("sampling.v")] {
        components.add(source)?;
    }
    components.add(&vliw_quad_pool::verilog_command(!native))?;
    components.add(&framebuffer_tdp::sources(surface, true, true)?)?;

    let (options, plans) = dsp_sharing::four_cycle_shared_roms(9)?;
    components.add(&lighting::generate_ii4_with_schedule_plans(options, &plans, true)?.source)?;
    components.add(include_str!("../pixel/vliw_lighting_tuple.v"))?;
    for (name, width) in [("light", 18), ("sample", 24)] {
        components.add(&row_store::verilog_owned_q(128, width, native)?.replace(
            "gpu_v2_pixel_row_store",
            &format!("gpu_v2_postquad_{name}_bank"),
        ))?;
    }

    components.add(
        &include_str!("../pixel/vliw_backend_final.v")
            .replace("// @FINAL_POLICY@", "`define GPU_V2_COMMAND_FINAL"),
    )?;
    components.add(include_str!(
        "../pixel/final_stage/rtl/two_phase_dispatch.v"
    ))?;
    components.add(if native {
        include_str!("../pixel/final_stage/rtl/shared_rop_native.v")
    } else {
        include_str!("../pixel/final_stage/rtl/shared_rop_portable.v")
    })?;
    components.add(&final_stage::rtl::ordered_shared::two_cycle_mul9_source(
        native,
    ))?;

    components.add_complete(&rtl::direct_lod::command_body())?;
    components.add_complete(rtl::compact_coordinate::verilog_command())?;
    components.add_complete(&rtl::coordinate_store::command_six_source(native))?;
    components.add_complete(rtl::compact_coefficient::command_direct_weight().trim_end())?;
    components.add_module(
        rtl::compact_coefficient::source(),
        "gpu_v2_texture_coefficient_mul9",
    )?;
    components.add(&rtl::runtime_color::public_source_reserved())?;
    components.add(&rtl::runtime_color::direct_source(true))?;
    let color_helpers = include_str!("../pixel/postquad_ii4_color.v");
    for name in ["color_serial_registered_mul9", "color_packed_accumulator"] {
        components.add(module(color_helpers, name)?)?;
    }
    components.add_complete(
        &include_str!("../texture_cache16x8.v")
            .trim_end()
            .replace("@NATIVE@", if native { "1" } else { "0" }),
    )?;
    for source in [
        include_str!("../texture_membership16x8.v"),
        include_str!("../texture_work16.v"),
        include_str!("../texture_packet16x8.v"),
        include_str!("../texture_group_q.v"),
        include_str!("../texture_packet_backend.v"),
        include_str!("../texture_runtime_backend.v"),
    ] {
        components.add_complete(source.trim_end())?;
    }
    let source = components.render("gpu_v2_vliw_backend_three_port")?;
    Ok(format!(
        "{native_scope}{pool_scope}`define GPU_V2_LIVE_GLOBALS\n\
         `define GPU_V2_VLIW_COLOR_COMMAND\n{source}\n\
         `undef GPU_V2_VLIW_COLOR_COMMAND\n\n`undef GPU_V2_LIVE_GLOBALS\n",
        native_scope = if native {
            "`define GPU_V2_GOWIN_DSP\n"
        } else {
            ""
        },
        pool_scope = if native {
            ""
        } else {
            "`define GPU_VLIW_POOL_PORTABLE\n"
        },
    ))
}
