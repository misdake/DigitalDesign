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
    // The closed VLIW path accepts every completed Color pixel in its final bank.
    // Keep the generic buffered Color factory unchanged.
    let (color_a, color_b) = span(&source, "gpu_v2_texture_runtime_color_checked")?;
    source.replace_range(
        color_a..color_b,
        include_str!("vliw_color_direct.v").trim_end(),
    );
    // The VLIW pool owns Normal/NDC Q through the complete pixel handshake.
    // Replace the selected consumer as one unit; the independent Light2 reference remains.
    let (light_a, light_b) = span(&source, "gpu_v2_pixel_lighting_consumer")?;
    source.replace_range(light_a..light_b, include_str!("vliw_lighting_tuple.v"));
    lower_pool_layout(&mut source)?;
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

/// Replace the selected join calendar as a unit. Z is owned by the producer
/// bank until the paired ROP depth acceptance; it never enters a staging ring.
fn lower_pool_layout(source: &mut String) -> Result<(), String> {
    let top = MODULE;
    patch(
        source,
        top,
        "wire basic_read,basic_valid,basic_collision;",
        "wire basic_read,basic_collision;",
    )?;
    patch(source, top, ".basic_return_valid(basic_valid),", "")?;
    patch(source, top, "wire[7:0]basic_address;wire[23:0]basic_data;",
        "wire[6:0]basic_address;wire[23:0]basic_data;wire[15:0]depth_data;\n wire sink_color_accept;reg[1:0]out_lane;")?;
    patch(source, top, ".basic_return_data(basic_data),.basic_collision(basic_collision),",
        ".basic_return_data(basic_data),.basic_collision(basic_collision),\n .depth_read(sink_color_accept),.depth_read_address({retire_slot,out_lane}),.depth_data(depth_data),")?;
    patch(source, top, "reg[2:0]state;reg[1:0]issue_lane,out_lane;\n reg paid;reg[3:0]depth_valid,depth_reserved;\n reg[15:0]depth_rows[0:3];\n reg depth_pending;reg[1:0]depth_tag,input_bypass;\n localparam J_READ=0,J_WAIT=1,J_ISSUE=2,J_DEPTH=3,J_DEPTH_WAIT=4,J_DRAIN=5;",
        "reg state;reg[1:0]issue_lane;\n reg[1:0]input_bypass;\n localparam J_READ=0,J_WAIT=1;")?;
    patch(
        source,
        top,
        "wire lane_ready,lane_accept,sink_color_accept,sink_color_ready;",
        "wire lane_ready,lane_accept,sink_color_ready;",
    )?;
    patch(source, top, "wire join_value_read=active&&(|issue_header[3:0])&&state==J_READ&&issue_exists&&issue_light_ready&&issue_sample_ready&&!depth_reserved[issue_lane];\n // Only old free rows pay reads. A retirement or publication on this edge\n // cannot fund a reservation. The producer never crosses a second quad.\n wire join_depth_read=active&&state==J_WAIT&&(paid||basic_valid)&&(!covered||final_in_ready)&&(!issue_ahead||issue_lane!=3);\n wire read_values=join_value_read;\n assign basic_read=join_value_read||join_depth_read;\n assign basic_address={issue_slot,issue_lane,state==J_WAIT};",
        r#"wire source_exists=active&&(|issue_header[3:0])&&issue_exists;
 wire issue_room=!issue_ahead||issue_lane!=3;
 wire join_value_read=source_exists&&state==J_READ&&covered&&issue_light_ready&&issue_sample_ready;
 // READ_MODE0 updates all three Qs on the J_READ edge. J_WAIT itself
 // owns complete source Q until Final accepts; CE0 freezes state and Q.
 // The cursor may enter the next quad, but cannot cross its final lane.
 wire join_issue=active&&state==J_WAIT&&final_in_ready&&issue_room;
 // A masked lane owns no arithmetic operands: advance its ordered prefix
 // directly, with no source RAM read, paid return or J_WAIT transition.
 wire skip_issue=source_exists&&state==J_READ&&!covered&&issue_room;
 wire source_advance=join_issue||skip_issue;
 wire read_values=join_value_read;
 assign basic_read=join_value_read;
 assign basic_address=read_key;
 // This ordered prefix also covers lanes which produce no Final token.
 // A lane cannot reach ROP until its real source/issue cursor has passed it.
 wire output_issued=issue_ahead||(issue_lane>out_lane);"#)?;
    patch(source, top, "issue_slot==retire_slot&&state==J_READ&&issue_lane==0&&!paid&&!depth_pending&&\n  depth_reserved==0&&depth_valid==0&&!color_paid&&!final_out_valid;",
        "issue_slot==retire_slot&&state==J_READ&&issue_lane==0&&!color_paid&&!final_out_valid;")?;
    patch(source, top, "wire lane_valid=active&&depth_valid[out_lane]&&(!out_covered||final_out_valid||color_paid);",
        "wire lane_valid=active&&output_issued&&(!out_covered||final_out_valid||color_paid);")?;
    patch(
        source,
        top,
        ".depth(depth_rows[out_lane])",
        ".depth(depth_data)",
    )?;
    patch(source, top, "stopped<=0;active<=0;state<=J_READ;paid<=0;depth_valid<=0;color_paid<=0;issue_slot<=0;depth_reserved<=0;depth_pending<=0;depth_tag<=0;input_bypass<=0;",
        "stopped<=0;active<=0;state<=J_READ;color_paid<=0;issue_slot<=0;input_bypass<=0;")?;
    patch(source, top, "// A paid depth return owns its tagged ring row even on CE0.\n   depth_pending<=advance&&join_depth_read;\n   if(advance&&join_depth_read)depth_tag<=issue_lane;\n   if(depth_pending&&basic_valid)begin\n    depth_rows[depth_tag]<=basic_data[15:0];depth_valid[depth_tag]<=1;\n   end\n   if(active&&state==J_WAIT&&basic_valid&&!depth_pending)paid<=1;",
        "// J_WAIT owns synchronous RGB/GH/texture Q through CE0 and backpressure.")?;
    patch(
        source,
        top,
        "state<=J_WAIT;paid<=0;depth_reserved[issue_lane]<=1;",
        "state<=J_WAIT;",
    )?;
    patch(source, top, "wire final_in_valid=active&&state==J_WAIT&&(paid||basic_valid)&&covered&&(!issue_ahead||issue_lane!=3);",
        "wire final_in_valid=active&&state==J_WAIT&&issue_room;")?;
    patch(
        source,
        top,
        "    if(active)case(state)\n     J_READ:if(join_value_read)begin",
        "    if(join_value_read)begin",
    )?;
    patch(source, top, "     end\n     J_WAIT:if(join_depth_read)begin\n      state<=J_READ;paid<=0;\n      if(issue_lane==3)begin issue_lane<=0;issue_slot<=issue_slot+5'd1;end\n      else issue_lane<=issue_lane+1'b1;\n     end\n    endcase\n    if(empty_skip)issue_slot<=issue_slot+5'd1;",
        r#"    end
    if(source_advance)state<=J_READ;
    // Every source prefix step, including a whole empty quad, has one owner.
    if(source_advance||empty_skip)begin
     if(empty_skip||issue_lane==3)begin issue_lane<=0;issue_slot<=issue_slot+5'd1;end
     else issue_lane<=issue_lane+1'b1;
    end"#)?;
    patch(source, top, "if(lane_accept)begin out_lane<=out_lane+1'b1;color_paid<=0;depth_valid[out_lane]<=0;depth_reserved[out_lane]<=0;end",
        "if(lane_accept)begin out_lane<=out_lane+1'b1;color_paid<=0;end")?;
    patch(
        source,
        top,
        "wire light_return,sample_return,light_collision,sample_collision;",
        "wire light_collision,sample_collision;",
    )?;
    patch(source, top, ".return_valid(light_return),", "")?;
    patch(source, top, ".return_valid(sample_return),", "")?;
    // These selected banks have exactly the same synchronous Q ownership.
    // Remove their unused validity pipelines too, in both physical backends.
    for bank in ["gpu_v2_postquad_light_bank", "gpu_v2_postquad_sample_bank"] {
        patch(source, bank, " output reg return_valid,\n", "")?;
        patch(
            source,
            bank,
            "if(reset) return_valid<=1'b0;",
            "if(reset)begin end",
        )?;
        patch(
            source,
            bank,
            "   return_valid<=read_en && !collision;\n",
            "",
        )?;
    }
    // Occupancy below32 makes the existing publish cursor unambiguous. The
    // parent does not need another increment/decrement copy of that interval.
    patch(source, top, "reg[5:0]published_count;", "")?;
    patch(
        source,
        top,
        "wire issue_exists=published_count>{5'd0,issue_ahead};",
        "wire issue_exists=issue_slot!=published_slot;",
    )?;
    patch(source, top, "published_count!=0&&empty_head", "empty_head")?;
    patch(source, top, "published_count<=0;", "")?;
    patch(source, top, "    case({published,quad_accepted||empty_skip})\n     2'b10:published_count<=published_count+1'b1;\n     2'b01:published_count<=published_count-1'b1;\n    endcase\n", "")?;
    patch(
        source,
        top,
        "if(!active&&published_count!=0&&!empty_head)begin",
        "if(!active&&retire_slot!=published_slot&&!empty_head)begin",
    )?;
    patch(
        source,
        top,
        "if(published_count>1)begin",
        "if((retire_slot+5'd1)!=published_slot)begin",
    )?;
    // The sink's registered C/D phase already is the C-paid fact. On a sink
    // fault it resets while the global sticky stop suppresses every consumer.
    patch(source, top, "reg color_paid;", "wire color_paid;")?;
    patch(
        source,
        top,
        "state<=J_READ;color_paid<=0;issue_slot",
        "state<=J_READ;issue_slot",
    )?;
    patch(source, top, "    if(sink_color_accept)color_paid<=1;\n    if(lane_accept)begin out_lane<=out_lane+1'b1;color_paid<=0;end",
        "    if(lane_accept)out_lane<=out_lane+1'b1;")?;
    patch(
        source,
        top,
        " gpu_v2_pixel_framebuffer_sink sink(",
        " gpu_v2_pixel_framebuffer_sink sink(\n .depth_phase(color_paid),",
    )?;
    let sink = "gpu_v2_pixel_framebuffer_sink";
    patch(
        source,
        sink,
        " output wire raw_head_retire,",
        " output wire depth_phase,raw_head_retire,",
    )?;
    patch(
        source,
        sink,
        " reg phase;",
        " reg phase;\n assign depth_phase=phase;",
    )?;
    // The same paired-D acceptance advances both former lane registers.
    // Use the sink's canonical lane directly; no independent lane can mismatch.
    patch(source, top, "reg[1:0]out_lane;", "wire[1:0]out_lane;")?;
    patch(
        source,
        top,
        "issue_lane<=0;out_lane<=0;light_complete",
        "issue_lane<=0;light_complete",
    )?;
    patch(
        source,
        top,
        "active<=1;header<=head_descriptor;out_lane<=0;",
        "active<=1;header<=head_descriptor;",
    )?;
    patch(
        source,
        top,
        "    if(lane_accept)out_lane<=out_lane+1'b1;\n",
        "",
    )?;
    patch(source, top, ".lane(out_lane),", ".active_lane(out_lane),")?;
    patch(
        source,
        sink,
        " input wire lane_valid,input wire[1:0]lane,",
        " input wire lane_valid,output wire[1:0]active_lane,",
    )?;
    patch(
        source,
        sink,
        " wire lane_matches=(lane==expected_lane);",
        " assign active_lane=expected_lane;",
    )?;
    patch(
        source,
        sink,
        "wire[2:0]row_index={lane,phase};",
        "wire[2:0]row_index={expected_lane,phase};",
    )?;
    let (a, b) = span(source, sink)?;
    let body = &source[a..b];
    if body.matches("&&lane_matches").count() != 6 {
        return Err("selected sink lane-match ownership anchors changed".into());
    }
    let body = body.replace("&&lane_matches", "");
    source.replace_range(a..b, &body);
    Ok(())
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
