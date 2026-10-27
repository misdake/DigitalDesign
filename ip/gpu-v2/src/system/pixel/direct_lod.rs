//! Complete selected postquad chain with the helper/D/L preparation removed.
//! The old eight-row Basic/Light boundary is temporary; the direct texture leaf
//! is independently callable by the subsequent producer-pool integration.
use super::{postquad_ii4, quad_ingress};
use crate::{
    framebuffer::ports::MaterializedSurface,
    texture::{ports::Slot, rtl},
};
use std::collections::{BTreeMap, BTreeSet};

pub const MODULE: &str = "gpu_v2_postquad_direct_lod";

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

/// Same selected C, coefficient/weight, cache, Color, Lighting, Final and ROP.
/// LOD is U5F3 and is captured with row0; UV32 is consumed on odd Basic rows.
/// Draw remains immutable until pipeline_idle. RGB bypass does not bypass light.
pub fn verilog_selected(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    let native = matches!(backend, quad_ingress::rtl::Backend::Native);
    let mut source = postquad_ii4::verilog(slots, surface, backend, 9)?.replace("\r\n", "\n");
    for name in ["gpu_v2_postquad", "gpu_v2_pixel_sampling_ingress"] {
        patch(
            &mut source,
            name,
            "input wire in_valid,input wire[22:0]",
            "input wire[7:0]in_lod,\n input wire in_valid,input wire[22:0]",
        )?;
        let (a, b) = span(&source, name)?;
        let body = source[a..b]
            .replace(
                "input wire[35:0]in_light,in_uv,",
                "input wire[35:0]in_light,input wire[31:0]in_uv,",
            )
            .replace(
                "input wire[35:0] in_light,in_uv,",
                "input wire[35:0] in_light,input wire[31:0]in_uv,",
            );
        source.replace_range(a..b, &body);
        patch(
            &mut source,
            name,
            ".in_valid(in_valid),.in_begin(in_begin)",
            ".in_lod(in_lod),.in_valid(in_valid),.in_begin(in_begin)",
        )?;
        let (a, b) = span(&source, name)?;
        let body = source[a..b]
            .replace("wire[10:0]sample_descriptor", "wire[18:0]sample_descriptor")
            .replace(
                "wire[10:0] sample_descriptor",
                "wire[18:0] sample_descriptor",
            );
        source.replace_range(a..b, &body);
    }
    patch(
        &mut source,
        "gpu_v2_pixel_sampling_ingress",
        ".in_uv(in_uv)",
        ".in_uv({2'b00,in_uv[31:16],2'b00,in_uv[15:0]})",
    )?;
    patch(
        &mut source,
        "gpu_v2_pixel_sampling_ingress",
        "sample_descriptor[10],draw_config[15:0]",
        "1'b0,8'd0,sample_descriptor[18:11]",
    )?;
    // The queued quad is already complete. Reserve public ownership at its
    // first C input so an early cache hit cannot precede publication.
    patch(
        &mut source,
        "gpu_v2_pixel_sampling_ingress",
        "sample_ready&&sample_row==3",
        "sample_ready&&sample_row==0",
    )?;
    // Prepare immutable framebuffer tile metadata at the producer's row0.
    // The head then selects an address only; retirement never drives stride math.
    patch(
        &mut source,
        "gpu_v2_postquad",
        "wire[22:0]issue_descriptor,head_descriptor;",
        "wire[22:0]issue_descriptor,head_descriptor;wire[8:0]head_tile;",
    )?;
    for name in ["gpu_v2_postquad", "gpu_v2_pixel_sampling_ingress"] {
        patch(
            &mut source,
            name,
            ".descriptor_head(head_descriptor)",
            ".descriptor_head(head_descriptor),.descriptor_head_tile(head_tile)",
        )
        .or_else(|_| {
            patch(
                &mut source,
                name,
                ".descriptor_head(descriptor_head)",
                ".descriptor_head(descriptor_head),.descriptor_head_tile(descriptor_head_tile)",
            )
        })?;
    }
    patch(
        &mut source,
        "gpu_v2_pixel_sampling_ingress",
        "output wire[22:0]descriptor_issue,descriptor_head,",
        "output wire[22:0]descriptor_issue,descriptor_head,output wire[8:0]descriptor_head_tile,",
    )?;
    if !(surface.width / 16).is_power_of_two() {
        let (a, b) = span(&source, "gpu_v2_postquad")?;
        let body = source[a..b]
            .lines()
            .filter(|line| {
                !line.trim_start().starts_with("wire[8:0]next_tile_y=")
                    && !line.trim_start().starts_with("wire[8:0]next_tile=")
            })
            .collect::<Vec<_>>()
            .join("\n");
        if body.matches("header_tile<=next_tile;").count() != 2 {
            return Err("direct LOD tile capture anchors".into());
        }
        source.replace_range(
            a..b,
            &body.replace("header_tile<=next_tile;", "header_tile<=head_tile;"),
        );
    }
    let q = "gpu_v2_quad_ingress_closed_basic";
    patch(
        &mut source,
        q,
        "output wire[22:0]descriptor_issue,descriptor_head,",
        "output wire[22:0]descriptor_issue,descriptor_head,output wire[8:0]descriptor_head_tile,",
    )?;
    let terms: Vec<_> = (0..6)
        .filter(|bit| (surface.width / 16) & (1 << bit) != 0)
        .map(|bit| format!("(write_tile_y << {bit})"))
        .collect();
    patch(&mut source,q,"reg[22:0]descriptors[0:15];",&format!("reg[22:0]descriptors[0:15];\n (* syn_ramstyle=\"distributed_ram\" *)reg[8:0]descriptor_tiles[0:15];\n wire[8:0]write_tile_y={{5'd0,in_begin[22:19]}};\n wire[8:0]write_tile={}+{{4'd0,in_begin[15:11]}};",terms.join("+")))?;
    patch(&mut source,q,"assign descriptor_head=descriptors[descriptor_head_slot[3:0]];","assign descriptor_head=descriptors[descriptor_head_slot[3:0]];\n assign descriptor_head_tile=descriptor_tiles[descriptor_head_slot[3:0]];")?;
    patch(&mut source,q,"descriptors[{1'b0,alloc_ptr[3:0]}]<=in_begin;","descriptors[{1'b0,alloc_ptr[3:0]}]<=in_begin;\n        descriptor_tiles[alloc_ptr[3:0]]<=write_tile;")?;
    patch(
        &mut source,
        q,
        "input wire in_valid,",
        "input wire in_valid,input wire[7:0]in_lod,",
    )?;
    patch(
        &mut source,
        q,
        "output wire [10:0] sample_descriptor",
        "output wire [18:0] sample_descriptor",
    )?;
    patch(
        &mut source,
        q,
        "reg [10:0] sample_desc",
        "reg [18:0] sample_desc",
    )?;
    patch(
        &mut source,
        q,
        "sample_desc[sample_alloc[2:0]]<={current_begin[7],",
        "sample_desc[sample_alloc[2:0]]<={in_lod,current_begin[7],",
    )?;
    let (a, b) = span(&source, "gpu_v2_texture_runtime_rows")?;
    let header = source[a..b].split_once(");").ok_or("direct rows header")?.0;
    let wrapper = format!(
        "{});\n{}\nendmodule",
        header,
        include_str!("direct_lod_rows.v")
    );
    source.replace_range(a..b, &wrapper);
    // The old C definition is shared by both emitters; append only new modules.
    let direct = rtl::direct_lod::verilog(native)?;
    let (a, b) = span(&direct, "gpu_v2_texture_serial_coord")?;
    let direct = format!("{}{}", &direct[..a], &direct[b..]);
    source.push_str(&direct);
    patch(
        &mut source,
        "gpu_v2_postquad",
        "module gpu_v2_postquad(",
        &format!("module {MODULE}("),
    )?;
    let result = reachable(&source, MODULE)?;
    for forbidden in [
        "module gpu_v2_texture_runtime_d(",
        "module gpu_v2_texture_serial_lod ",
        "module gpu_v2_uv_rows",
        "module gpu_v2_context_store_shared",
        "module gpu_v2_texture_runtime_frontend",
    ] {
        if result.contains(forbidden) {
            return Err(format!(
                "direct LOD retained old active preparation: {forbidden}"
            ));
        }
    }
    Ok(if native {
        "`define GPU_V2_GOWIN_DSP\n".to_owned() + &result
    } else {
        result
    })
}
