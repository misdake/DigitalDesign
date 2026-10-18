//! Complete selected framebuffer replacement: fixed scalar ROP and independent TDP maintenance.
//! Historical factories remain available; the live graph contains only this cache/leaf.
use crate::framebuffer::ports::MaterializedSurface;

pub const CACHE_MODULE: &str = "gpu_v2_framebuffer_tdp";
pub const CACHE: &str = include_str!("framebuffer_tdp_cache.v");
pub const BANK: &str = include_str!("framebuffer_tdp_bank.v");
pub const LEAF: &str = include_str!("framebuffer_tdp_leaf.v");

fn module_span(source: &str, name: &str) -> Result<(usize, usize), String> {
    let marker = format!("module {name}");
    let starts: Vec<_> = source
        .match_indices(&marker)
        .filter_map(|(i, _)| {
            matches!(
                source.as_bytes().get(i + marker.len()),
                Some(b'(' | b' ' | b'\n' | b'\r' | b'#')
            )
            .then_some(i)
        })
        .collect();
    if starts.len() != 1 {
        return Err(format!("TDP replacement {name}: {} modules", starts.len()));
    }
    let a = starts[0];
    let b = a + source[a..].find("endmodule").ok_or("TDP module end")? + 9;
    Ok((a, b))
}

/// Whole-module substitution after historical source lowering has finished.
/// No operand, state-machine or arithmetic textual anchors are patched here.
pub fn lower(
    source: String,
    surface: MaterializedSurface,
    _native: bool,
) -> Result<String, String> {
    lower_with_prefetch(source, surface, _native, true, true)
}

/// Optional dirty speculative replacement reuses the same single maintenance FSM.
/// Disabling hints preserves the step-1/2 scalar TDP architecture.
pub fn lower_with_prefetch(
    mut source: String,
    surface: MaterializedSurface,
    _native: bool,
    enable: bool,
    dirty: bool,
) -> Result<String, String> {
    surface.validate()?;
    if source.contains(&format!("module {CACHE_MODULE}")) {
        return Err("TDP framebuffer replacement already applied".into());
    }
    for name in [
        "gpu_v2_pixel_framebuffer_sink",
        "framebuffer_cache",
        "rop_leaf",
    ] {
        let (a, b) = module_span(&source, name)?;
        source.replace_range(a..b, "");
    }
    let sink = include_str!("framebuffer_tdp.v")
        .replace("@COLOR@", &surface.color_base_bytes.to_string())
        .replace("@DEPTH@", &surface.depth_base_bytes.to_string())
        .replace("@WIDTH@", &surface.width.to_string())
        .replace("@HEIGHT@", &surface.height.to_string())
        .replace("@PREFETCH@", if enable { "1" } else { "0" })
        .replace("@PREFETCH_DIRTY@", if dirty { "1" } else { "0" });
    source.push_str(&format!("\n{sink}\n{CACHE}\n{LEAF}\n{BANK}\n"));
    Ok(source)
}
