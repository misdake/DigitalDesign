//! Complete selected backend consuming the authoritative VLIW producer pool.
//! No eight-row/13-word ingress adapter and no duplicated source payload banks.
use super::framebuffer_tdp;
use super::vliw_quad_pool as pool;
use super::{direct_lod, quad_ingress};
use crate::{framebuffer::ports::MaterializedSurface, texture::ports::Slot};
use std::collections::{BTreeMap, BTreeSet};
pub const MODULE: &str = "gpu_v2_vliw_backend";
pub const SOURCE: &str = include_str!("vliw_backend.v");
fn span(source: &str, name: &str) -> Result<(usize, usize), String> {
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
        return Err(format!(
            "direct LOD module {name}: {} declarations",
            starts.len()
        ));
    }
    let a = starts[0];
    let b = a
        + source[a..]
            .find("endmodule")
            .ok_or("direct LOD endmodule")?
        + 9;
    Ok((a, b))
}
fn patch(source: &mut String, name: &str, old: &str, new: &str) -> Result<(), String> {
    let (a, b) = span(source, name)?;
    let body = &source[a..b];
    if body.matches(old).count() != 1 {
        return Err(format!("direct LOD {name} anchor {old:?}"));
    }
    let body = body.replacen(old, new, 1);
    source.replace_range(a..b, &body);
    Ok(())
}

/// Remove all unreachable old preparation/reference definitions, not just their
/// instances. This keeps one active implementation and a reviewable source set.
fn reachable(source: &str, top: &str) -> Result<String, String> {
    let mut modules = BTreeMap::new();
    for line in source.split_inclusive('\n') {
        let s = line.trim_start();
        if let Some(rest) = s.strip_prefix("module ") {
            let name = rest
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .next()
                .unwrap();
            let (a, b) = span(source, name)?;
            modules.insert(name.to_owned(), source[a..b].to_owned());
        }
    }
    let mut live = BTreeSet::from([top.to_owned()]);
    loop {
        let mut add = BTreeSet::new();
        for name in &live {
            let body = modules
                .get(name)
                .ok_or_else(|| format!("direct LOD missing {name}"))?;
            let tokens: BTreeSet<_> = body
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .collect();
            for candidate in modules.keys() {
                if tokens.contains(candidate.as_str()) {
                    add.insert(candidate.clone());
                }
            }
        }
        let n = live.len();
        live.extend(add);
        if live.len() == n {
            break;
        }
    }
    let mut result = format!("{}\n", modules[top]);
    for name in live.iter().filter(|n| n.as_str() != top) {
        result.push_str(&modules[name]);
        result.push('\n');
    }
    Ok(result)
}

/// Front production never waits on backend CE. Draw changes require the
/// combined frontend/backend owner to prove global drain.
pub fn verilog_selected(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    render_globals(slots, surface, backend)
}

fn final_source() -> String {
    include_str!("vliw_backend_final.v").replace("// @FINAL_POLICY@", "")
}

fn top_source() -> String {
    include_str!("vliw_backend_top.v").replace("// @JOIN_POLICY@", "")
}

fn render_globals(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    let native = matches!(backend, quad_ingress::rtl::Backend::Native);
    if surface.width != 400 || surface.height != 240 {
        return Err("VLIW pool NDC/tile contract requires400x240".into());
    }
    let mut source = direct_lod::verilog_source_q(slots, surface, backend)?.replace("\r\n", "\n");
    let (a, b) = span(&source, "gpu_v2_pixel_sampling_ingress")?;
    source.replace_range(a..b, SOURCE);
    source.push_str(&pool::verilog(!native));
    // The root owns its complete final ports/calendar. Intermediate historical
    // factories still supply independently selected numerical children.
    let (a, b) = span(&source, direct_lod::MODULE)?;
    source.replace_range(a..b, &top_source());
    // Whole-GPU configuration drain includes the entire flush sequence, even
    // between two line transactions where no maintenance request is active.
    patch(
        &mut source,
        "framebuffer_cache",
        "assign idle = !rop_active && !mt_active",
        "assign idle = !flushing && !rop_active && !mt_active",
    )?;
    // Replace the complete selected leaf, not just its ready expression. The
    // original shared Final reference remains unchanged for independent replay.
    let (final_a, final_b) = span(&source, "gpu_v2_final_ordered_two_cycle")?;
    source.replace_range(final_a..final_b, &final_source());
    source = framebuffer_tdp::lower(source, surface, native)?;
    // The closed VLIW path accepts every completed Color pixel in its final bank.
    // Keep the generic buffered Color factory unchanged.
    let (color_a, color_b) = span(&source, "gpu_v2_texture_runtime_color_checked")?;
    source.replace_range(
        color_a..color_b,
        crate::texture::rtl::runtime_color::direct_source(false).trim_end(),
    );
    // The VLIW pool owns Normal/NDC Q through the complete pixel handshake.
    // Replace the selected consumer as one unit; the independent Light2 reference remains.
    let (light_a, light_b) = span(&source, "gpu_v2_pixel_lighting_consumer")?;
    source.replace_range(light_a..light_b, include_str!("vliw_lighting_tuple.v"));
    // These banks lend their held Q directly; the join calendar owns the loan.
    for (name, width) in [("light", 18), ("sample", 24)] {
        let module = format!("gpu_v2_postquad_{name}_bank");
        let bank = super::row_store::verilog_owned_q(128, width, native)?
            .replace("gpu_v2_pixel_row_store", &module);
        let (a, b) = span(&source, &module)?;
        source.replace_range(a..b, &bank);
    }
    let result = reachable(&source, MODULE)?;
    Ok(format!(
        "{}{}{}",
        if native {
            "`define GPU_V2_GOWIN_DSP\n"
        } else {
            ""
        },
        if !native {
            "`define GPU_VLIW_POOL_PORTABLE\n"
        } else {
            ""
        },
        result
    ))
}
