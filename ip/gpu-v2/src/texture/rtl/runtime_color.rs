//! Closed ordered Runtime Color and public-result ownership.
//! Context8 lifetime is separate from these sixteen public quad IDs. The
//! ordered producer/cache emits covered lanes in increasing order, so consuming
//! the highest covered lane proves every group of that quad has passed Color.
//! Only valid1 and last-lane2 are retained per public ID. This selected contract
//! deliberately does not validate arbitrary duplicate/out-of-order lane streams;
//! the independent wide diagnostic owner remains the qualification reference.
use super::color;

pub const MODULE: &str = "gpu_v2_texture_runtime_color";
pub const PUBLIC_OWNER_BITS: usize = 16 * 3;

/// A sole parent UV tail reserves a free public ID at accepted row0 and
/// exclusively publishes it at row3. No other writer can allocate that ID
/// during the partial tail. Under that closed contract, the row3 publication
/// preflight is redundant. The old-state publish_ready/fire guard remains;
/// all other checked Color/public guards and the default emitter are unchanged.
pub fn source_reserved(native: bool) -> String {
    let mut value = source(native);
    let old = "wire publish_bad=publish_valid && !publish_ready;";
    assert_eq!(value.matches(old).count(), 1, "reserved publication anchor");
    value = value.replace(old, "wire publish_bad=1'b0;");
    value
}

/// The checked result16 Color source with independent proposal preflight.
/// Static source adaptation changes only proposal/control wiring; all checked
/// arithmetic, packet, stream, accumulator and credit guards remain intact.
pub fn source(native: bool) -> String {
    let mut leaf = color::source().replace(
        "gpu_v2_texture_color",
        "gpu_v2_texture_runtime_color_checked",
    );
    for (old, new) in [
        (
            "  input  wire        ce,",
            "  input  wire        ce,\n  input wire proposal_ce,\n  output wire preflight_bad,",
        ),
        (
            "assign in_ready = ce && !fault",
            "assign in_ready = proposal_ce && !fault",
        ),
        (
            "  wire result_write=!reset && ce",
            "  assign preflight_bad=fault_now;\n  wire result_write=!reset && ce",
        ),
    ] {
        assert_eq!(leaf.matches(old).count(), 1, "Color proposal anchor {old}");
        leaf = leaf.replace(old, new);
    }
    let mut result = String::new();
    if native {
        result.push_str("`define GPU_V2_GOWIN_DSP\n");
    }
    result.push_str(&leaf);
    result.push_str(include_str!("runtime_color.v"));
    result
}
