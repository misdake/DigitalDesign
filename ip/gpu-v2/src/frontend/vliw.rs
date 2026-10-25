//! Adopted V96 all-vertex/all-triangle front end, with direct result writes.

/// Current arithmetic source; the result pool belongs to the combined parent.
/// Vertex arithmetic and its control ROM retain the recorded origin.
/// Output transport stores S12F10 normals in one N36 row and exports 16 T items.
pub fn verilog(portable: bool) -> String {
    let mut source = if portable {
        "`define PORTABLE\n".to_owned()
    } else {
        String::new()
    };
    source.push_str(&super::vliw_numeric::verilog());
    for part in [
        include_str!("vliw/primitives/dsp.v"),
        include_str!("vliw/primitives/ram.v"),
        include_str!("vliw/primitives/window.v"),
        include_str!("vliw/vertex/control_rom.v"),
        include_str!("vliw/vertex/mac.v"),
        include_str!("vliw/vertex/rf.v"),
        include_str!("vliw/t32_core.v"),
        include_str!("vliw/vertex/storage.v"),
        include_str!("vliw/raster_storage.v"),
        include_str!("vliw/input_storage.v"),
        include_str!("vliw/input_t_work.v"),
        include_str!("vliw/meshlet.v"),
        include_str!("vliw/raster.v"),
        include_str!("vliw/compute.v"),
        include_str!("vliw/lod.v"),
        include_str!("vliw/prepare.v"),
        include_str!("vliw/prepare_core.v"),
        include_str!("vliw/prepare_program.v"),
        include_str!("vliw/raster_program.v"),
        include_str!("vliw/interpolator_stream.v"),
        include_str!("vliw/pipeline_stream.v"),
    ] {
        source.push_str(part);
        source.push('\n');
    }
    source.push_str(&include_str!("vliw/frontend.v").replace(
        "`include \"control_fields.vh\"",
        include_str!("vliw/vertex/control_fields.vh"),
    ));
    source
}
