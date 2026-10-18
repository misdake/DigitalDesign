//! Complete selected backend consuming the authoritative VLIW producer pool.
//! No eight-row/13-word ingress adapter and no duplicated source payload banks.
use super::vliw_quad_pool as pool;
use super::{direct_lod, quad_ingress};
use crate::{framebuffer::ports::MaterializedSurface, texture::ports::Slot};
use std::collections::{BTreeMap, BTreeSet};
#[path = "framebuffer_tdp.rs"]
mod framebuffer_tdp;
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
    let native = matches!(backend, quad_ingress::rtl::Backend::Native);
    if surface.width != 400 || surface.height != 240 {
        return Err("VLIW pool NDC/tile contract requires400x240".into());
    }
    let mut source = direct_lod::verilog_selected(slots, surface, backend)?.replace("\r\n", "\n");
    let (a, b) = span(&source, "gpu_v2_pixel_sampling_ingress")?;
    source.replace_range(a..b, SOURCE);
    source.push_str(&pool::verilog(!native));
    let top = "gpu_v2_postquad_direct_lod";
    patch(
        &mut source,
        top,
        "module gpu_v2_postquad_direct_lod(",
        "module gpu_v2_vliw_backend(",
    )?;
    let top = MODULE;
    patch(&mut source,top," input wire[7:0]in_lod,\n input wire in_valid,input wire[22:0]in_begin,\n input wire[31:0]in_basic,input wire[35:0]in_light,input wire[31:0]in_uv,\n output wire in_ready,output wire[2:0]in_row,",PRODUCER_PORTS)?;
    patch(
        &mut source,
        top,
        "wire[1:0]light_used;wire[3:0]sample_used;",
        "wire[5:0]light_used,sample_used;",
    )?;
    patch(
        &mut source,
        top,
        " gpu_v2_pixel_sampling_ingress front(",
        " gpu_v2_vliw_sampling front(",
    )?;
    patch(&mut source,top," .in_lod(in_lod),.in_valid(in_valid),.in_begin(in_begin),.in_basic(in_basic),.in_light(in_light),.in_uv(in_uv),\n .in_ready(in_ready),.in_row(in_row),.advance(advance),.preflight_bad(front_bad),.fault(front_fault),",PRODUCER_CONNECTIONS)?;
    patch(
        &mut source,
        top,
        ".descriptor_head_slot(raw_head_retire?{1'b0,retire_slot[3:0]+4'd1}:retire_slot),",
        "",
    )?;
    patch(&mut source,top,".global_used(global_used),.light_used(light_used),.sample_used(sample_used),",".global_used(global_used),.light_used(light_used),.sample_used(sample_used),.producer_used(producer_used),")?;
    patch(
        &mut source,
        top,
        "wire join_value_read=active&&state==J_READ&&issue_exists",
        "wire join_value_read=active&&(|issue_header[3:0])&&state==J_READ&&issue_exists",
    )?;
    patch(
        &mut source,
        top,
        "assign pipeline_idle=!active&&global_used==0&&in_row==0&&cache_drained&&!final_out_valid;",
        "assign pipeline_idle=!active&&global_used==0&&cache_drained&&!final_out_valid;",
    )?;
    patch(
        &mut source,
        top,
        "case({published,quad_accepted})",
        "case({published,quad_accepted||empty_skip})",
    )?;
    patch(
        &mut source,
        top,
        "if(!active&&published_count!=0)begin",
        "if(!active&&published_count!=0&&!empty_head)begin",
    )?;
    patch(
        &mut source,
        top,
        "issue_slot<={1'b0,issue_slot[3:0]+4'd1};",
        "issue_slot<=issue_slot+5'd1;",
    )?;
    patch(&mut source,top,"if(published_count>1)begin active<=1;header<=head_descriptor;header_tile<=head_tile;end","if(published_count>1)begin active<=|next_descriptor[3:0];header<=next_descriptor;header_tile<=next_tile;end")?;
    patch(
        &mut source,
        top,
        "    if(sink_color_accept)color_paid<=1;",
        "    if(empty_skip)issue_slot<=issue_slot+5'd1;\n    if(sink_color_accept)color_paid<=1;",
    )?;
    patch(
        &mut source,
        top,
        " wire basic_read,basic_valid,basic_collision;",
        EMPTY_CONTROL,
    )?;
    patch(&mut source, top, " reg color_paid;", EMPTY_ASSIGN)?;
    patch(
        &mut source,
        top,
        "wire coalition_allow=compute_ce&&!(front_bad||sink_bad||final_bad);",
        "wire coalition_allow=compute_ce;",
    )?;
    patch(
        &mut source,
        top,
        "assign fault=abort||stopped||front_fault||sink_fault||final_fault;",
        "assign fault=abort||stopped;",
    )?;
    patch(&mut source,top,"if(light_collision||sample_collision||basic_collision||(draw_rebind&&!pipeline_idle))stopped<=1;",
      "if(abort||front_bad||sink_bad||final_bad||front_fault||sink_fault||final_fault||light_collision||sample_collision||basic_collision||(draw_rebind&&!pipeline_idle))stopped<=1;")?;
    patch(
        &mut source,
        top,
        "wire[22:0]issue_descriptor,head_descriptor;wire[8:0]head_tile;",
        "wire[22:0]issue_descriptor,head_descriptor,next_descriptor;wire[8:0]head_tile,next_tile;",
    )?;
    patch(&mut source,top,".descriptor_head(head_descriptor),.descriptor_head_tile(head_tile),",".descriptor_head(head_descriptor),.descriptor_next(next_descriptor),.descriptor_head_tile(head_tile),.descriptor_next_tile(next_tile),")?;
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
    source.replace_range(final_a..final_b, include_str!("vliw_backend_final.v"));
    source = framebuffer_tdp::lower(source, surface, native)?;
    patch(
        &mut source,
        top,
        " wire basic_read,basic_valid,basic_collision;",
        " wire framebuffer_prefetch_valid;wire[8:0]framebuffer_prefetch_tile;\n wire basic_read,basic_valid,basic_collision;",
    )?;
    patch(
        &mut source,
        top,
        " gpu_v2_vliw_sampling front(",
        " gpu_v2_vliw_sampling front(\n .prefetch_valid(framebuffer_prefetch_valid),.prefetch_tile(framebuffer_prefetch_tile),",
    )?;
    patch(
        &mut source,
        top,
        " gpu_v2_pixel_framebuffer_sink sink(",
        " gpu_v2_pixel_framebuffer_sink sink(\n .prefetch_valid(framebuffer_prefetch_valid),.prefetch_tile(framebuffer_prefetch_tile),.prefetch_abort(abort||stopped),",
    )?;
    // The held XY header already contains the complete raw tile identity.
    // Remove its duplicate register and both load paths after old lowering.
    patch(
        &mut source,
        top,
        "reg[8:0]header_tile;",
        "wire[8:0]header_tile={header[22:19],header[15:11]};",
    )?;
    patch(&mut source, top, "header_tile<=0;", "")?;
    patch(&mut source, top, "header_tile<=head_tile;", "")?;
    patch(&mut source, top, "header_tile<=next_tile;", "")?;
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
const PRODUCER_PORTS: &str = r#" input wire front_ce,input wire[1:0]phase,input wire draw_bank,draw_unlit,
 input wire quad_valid,output wire quad_ready,output wire[2:0]quad_id,
 input wire[7:0]quad_x,input wire[6:0]quad_y,input wire quad_rgb,
 input wire quad_end_valid,input wire[2:0]quad_end_id,input wire[3:0]quad_end_mask,
 input wire result_we,input wire[6:0]result_wa,input wire[35:0]result_wd,
 input wire depth_event_valid,input wire[4:0]depth_event_pid,input wire[15:0]depth_event_z16,input wire depth_event_pass,
 input wire lod_valid,input wire[2:0]lod_id,input wire[7:0]lod_value,
 output wire empty_skipped,output wire[5:0]producer_used,"#;
const PRODUCER_CONNECTIONS: &str = r#" .front_ce(front_ce&&!fault),.producer_phase(phase),.draw_bank(draw_bank),.draw_unlit(draw_unlit),
 .quad_valid(quad_valid),.quad_ready(quad_ready),.quad_id(quad_id),.quad_x(quad_x),.quad_y(quad_y),.quad_rgb(quad_rgb),
 .quad_end_valid(quad_end_valid),.quad_end_id(quad_end_id),.quad_end_mask(quad_end_mask),
 .result_we(result_we),.result_wa(result_wa),.result_wd(result_wd),
 .depth_event_valid(depth_event_valid),.depth_event_pid(depth_event_pid),.depth_event_z16(depth_event_z16),.depth_event_pass(depth_event_pass),
 .lod_valid(lod_valid),.lod_id(lod_id),.lod_value(lod_value),
 .empty_skip(empty_skip),.empty_head(empty_head),.empty_skip_ready(empty_skip_ready),
 .advance(advance),.preflight_bad(front_bad),.fault(front_fault),"#;
const EMPTY_CONTROL: &str = r#" wire basic_read,basic_valid,basic_collision;
 wire empty_head,empty_skip_ready;
 // No lookahead Basic/Light/Sample read is allowed for a zero-mask slot.
 // The empty head retires only after all previous head tokens have drained.
 wire empty_skip;
 assign empty_skipped=empty_skip;"#;
const EMPTY_ASSIGN: &str = r#" reg color_paid;
 assign empty_skip=advance&&!active&&published_count!=0&&empty_head&&empty_skip_ready&&
  issue_slot==retire_slot&&state==J_READ&&issue_lane==0&&!paid&&!depth_pending&&
  depth_reserved==0&&depth_valid==0&&!color_paid&&!final_out_valid;"#;
