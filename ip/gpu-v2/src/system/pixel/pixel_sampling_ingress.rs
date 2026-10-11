//! One actual Quad/Lighting/row Runtime Sampling top through real RGB results.
//! Immutable Draw banks and result/join/Final/ROP still belong to the parent.
use super::{pixel_preparation_ingress, quad_ingress};
use crate::texture::{
    ports::Slot,
    rtl::{runtime_cache, runtime_color, runtime_rows::Profile, work},
};

pub const MODULE: &str = "gpu_v2_pixel_sampling_ingress";

pub fn verilog(slots: &[Slot], backend: quad_ingress::rtl::Backend) -> Result<String, String> {
    verilog_work_direct(slots, backend)
}

/// Matched Work experiment: immutable payload SRAM feeds actual Packet E0.
pub fn verilog_work_direct(
    slots: &[Slot],
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    verilog_work_direct_profile(slots, backend, Profile::default())
}

fn verilog_work_direct_profile(
    slots: &[Slot],
    backend: quad_ingress::rtl::Backend,
    preparation: Profile,
) -> Result<String, String> {
    let source = verilog_profile(slots, backend, true, true, true, preparation)?;
    let reference = work::compact_verilog();
    assert_eq!(
        source.matches(&reference).count(),
        1,
        "Work replacement anchor"
    );
    Ok(source.replacen(&reference, &work::compact_direct_verilog(), 1))
}

/// The existing ordered Pool's second head supplies only next TileKey bits;
/// no additional Group FIFO or transaction identifier is allocated.
pub fn verilog_cache_pipeline(
    slots: &[Slot],
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    verilog_cache_pipeline_profile(slots, backend, Profile::default())
}

pub fn verilog_with_preparation(
    slots: &[Slot],
    backend: quad_ingress::rtl::Backend,
    preparation: Profile,
    cache_pipeline: bool,
) -> Result<String, String> {
    if cache_pipeline {
        verilog_cache_pipeline_profile(slots, backend, preparation)
    } else {
        verilog_work_direct_profile(slots, backend, preparation)
    }
}

fn verilog_cache_pipeline_profile(
    slots: &[Slot],
    backend: quad_ingress::rtl::Backend,
    preparation: Profile,
) -> Result<String, String> {
    let native = matches!(backend, quad_ingress::rtl::Backend::Native);
    let mut source = verilog_work_direct_profile(slots, backend, preparation)?;
    let old_cache = runtime_cache::verilog_pool_closed(slots, native)?;
    if source.matches(&old_cache).count() != 1 {
        return Err("Pipeline cache source anchor".into());
    }
    source = source.replacen(
        &old_cache,
        &runtime_cache::verilog_pipelined_pool_closed(slots, native)?,
        1,
    );
    // Three nested wrappers forward the lookahead. The generic emitters and
    // their diagnostic/reference interfaces remain byte-for-byte unchanged.
    for (old, new) in [
        ("output wire producer_ready,front_valid,output wire [71:0] front_payload,", "output wire producer_ready,front_valid,output wire [71:0] front_payload,\n output wire next_valid,output wire[21:0] next_key,"),
        ("assign front_payload=ph?head1:head0;", "assign front_payload=ph?head1:head0;\n assign next_valid=hv[!ph]||(pending&&phead!=ph);\n assign next_key=hv[!ph]?(ph?head0[21:0]:head1[21:0]):data_do[21:0];"),
        ("output wire out_valid,output wire [71:0] out_payload,", "output wire out_valid,output wire [71:0] out_payload,\n output wire next_valid,output wire[21:0] next_key,"),
        (".producer_ready(pool_ready),.front_valid(front_valid),.front_payload(front),", ".producer_ready(pool_ready),.front_valid(front_valid),.front_payload(front),\n  .next_valid(next_valid),.next_key(next_key),"),
        ("output wire out_valid,output wire[71:0] out_payload,", "output wire out_valid,output wire[71:0] out_payload,\n output wire next_valid,output wire[21:0] next_key,"),
        (".out_ready(out_ready),.out_valid(out_valid),.out_payload(out_payload),", ".out_ready(out_ready),.out_valid(out_valid),.out_payload(out_payload),\n .next_valid(next_valid),.next_key(next_key),"),
        (" gpu_v2_texture_runtime_backend backend(", " wire packet_next_valid;wire[21:0]packet_next_key;\n gpu_v2_texture_runtime_backend backend("),
        (".work_reserved(work_reserved),.work_count(work_count),.producer_count(producer_count),.unwritten_count(unwritten_count),", ".next_valid(packet_next_valid),.next_key(packet_next_key),\n .work_reserved(work_reserved),.work_count(work_count),.producer_count(producer_count),.unwritten_count(unwritten_count),"),
        (".in_valid(packet_out_valid),.in_payload(packet_out_payload),.proposal_ready(packet_out_ready),.in_ready(),", ".in_valid(packet_out_valid),.in_payload(packet_out_payload),.next_valid(packet_next_valid),.next_key(packet_next_key),.proposal_ready(packet_out_ready),.in_ready(),"),
    ] {
        if source.matches(old).count() != 1 {
            return Err(format!("Pool lookahead anchor changed: {old}"));
        }
        source = source.replacen(old, new, 1);
    }
    Ok(source)
}

/// Generic checked construction retained for matched producer experiments.
pub fn verilog_checked(
    slots: &[Slot],
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    verilog_profile(slots, backend, false, false, false, Profile::default())
}

/// Closed producer experiment: the real ordered Pool holds the borrowed head.
pub fn verilog_pool_closed(
    slots: &[Slot],
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    verilog_profile(slots, backend, true, false, false, Profile::default())
}

/// The single UV tail already reserved its public ID on row zero.
pub fn verilog_reserved(
    slots: &[Slot],
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    verilog_profile(slots, backend, true, true, false, Profile::default())
}

/// Factor row-specific proposals before the common validation cone.
pub fn verilog_factored(
    slots: &[Slot],
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    verilog_profile(slots, backend, true, true, true, Profile::default())
}

/// Isolate row proposal factoring from the reserved-publication experiment.
pub fn verilog_factored_checked(
    slots: &[Slot],
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    verilog_profile(slots, backend, true, false, true, Profile::default())
}

fn verilog_profile(
    slots: &[Slot],
    backend: quad_ingress::rtl::Backend,
    pool_closed: bool,
    reserved: bool,
    factored: bool,
    preparation: Profile,
) -> Result<String, String> {
    let native = matches!(backend, quad_ingress::rtl::Backend::Native);
    let mut front = pixel_preparation_ingress::verilog_with_preparation(backend, preparation)?;
    if factored {
        for (old, new) in [
            ("wire raw_proposal=row_proposal&&in_row==0;", "wire raw_proposal=running&&in_valid&&uv_ready&&reserve_ready&&admission_allow&&in_row==0;"),
            ("wire admit_proposal=row_proposal&&in_row==3;", "wire admit_proposal=running&&in_valid&&uv_ready&&in_row==3;"),
            // raw_proposal already contains reserve_ready, so this diagnostic
            // can never reject a known-state proposal from the actual row unit.
            (".reserve_check(raw_proposal)", ".reserve_check(1'b0)"),
        ] {
            assert_eq!(front.matches(old).count(), 1, "row factor anchor");
            front = front.replacen(old, new, 1);
        }
    }
    let end = front.find("endmodule").ok_or("Sampling top missing")?;
    let instances = r#"
 wire cache_valid,color_ready,color_valid,color_last,color_result;
 wire[71:0]cache_payload;wire[15:0]cache_tex0,cache_tex1,cache_tex2,cache_tex3;
 wire[5:0]color_key;wire[23:0]color_rgb;
 wire publish_proposal=ce&&!reset&&!fault&&sample_valid&&sample_ready&&sample_row==3;
 gpu_v2_texture_runtime_cache demand(
 .clk(clk),.reset(reset),.proposal_ce(ce),.commit_allow(allow),
 .abort(abort||quad_fault||sampling_fault||backend_fault||color_fault),
 .in_valid(packet_out_valid),.in_payload(packet_out_payload),.proposal_ready(packet_out_ready),.in_ready(),
 .preflight_bad(cache_bad),.advance(),.out_ready(color_ready),.out_valid(cache_valid),.out_payload(cache_payload),
 .out_tex0(cache_tex0),.out_tex1(cache_tex1),.out_tex2(cache_tex2),.out_tex3(cache_tex3),
 .mem_req_valid(mem_req_valid),.mem_req_addr(mem_req_addr),.mem_req_ready(mem_req_ready),
 .mem_resp_valid(mem_resp_valid),.mem_resp_index(mem_resp_index),.mem_resp_data(mem_resp_data),
 .mem_resp_complete(mem_resp_complete),.mem_resp_ok(mem_resp_ok),.drained(cache_drained),.fault(cache_fault));
 gpu_v2_texture_runtime_color color(
 .clk(clk),.reset(reset),.proposal_ce(ce&&!abort&&!quad_fault&&!sampling_fault&&!backend_fault&&!cache_fault),.commit_allow(allow),
 .check_quad(sample_descriptor[3:0]),.public_free(public_credit),
 .publish_valid(publish_proposal),.publish_quad(sample_descriptor[3:0]),.publish_mask(sample_descriptor[8:5]),
 .publish_ready(),.publish_fire(public_publish),
 .in_valid(cache_valid),.in_payload(cache_payload),.in_texels({cache_tex3,cache_tex2,cache_tex1,cache_tex0}),
 .in_ready(color_ready),.input_fire(),.out_ready(result_ready),.out_valid(color_valid),
 .out_key(color_key),.out_rgb(color_rgb),.quad_last(color_last),.result_fire(color_result),
 .eligible(),.advance(),.preflight_fault(color_bad),.fault(color_fault),.public_live(public_live));
 assign result_valid=!reset&&!fault&&color_valid;
 assign result_key=color_key;assign result_rgb=color_rgb;
 assign result_quad_last=result_valid&&color_last;
 assign result_fire=color_result;
"#;
    let header = " input wire packet_enable,packet_out_ready,";
    assert_eq!(front.matches(header).count(), 1);
    let source = (front[..end].to_owned()+instances+&front[end..])
        .replacen("module gpu_v2_pixel_preparation_ingress(",&format!("module {MODULE}("),1)
        .replacen("input wire public_credit,input wire[26:0] draw_config,","output wire public_credit,input wire[26:0] draw_config,",1)
        .replacen(header,r#" input wire packet_enable,abort,result_ready,
 output wire packet_out_ready,result_valid,result_quad_last,result_fire,public_publish,
 output wire[5:0]result_key,output wire[23:0]result_rgb,output wire[15:0]public_live,
 output wire mem_req_valid,output wire[31:0]mem_req_addr,
 input wire mem_req_ready,mem_resp_valid,input wire[3:0]mem_resp_index,
 input wire[63:0]mem_resp_data,input wire mem_resp_complete,mem_resp_ok,
 output wire cache_drained,"#,1)
        .replacen("assign preflight_bad=", "wire cache_bad,cache_fault,color_bad,color_fault;\n assign preflight_bad=",1)
        .replacen("assign preflight_bad=quad_bad||sampling_bad||backend_bad;","assign preflight_bad=quad_bad||sampling_bad||backend_bad||cache_bad||color_bad;",1)
        .replacen("assign fault=quad_fault||sampling_fault||backend_fault;","assign fault=abort||quad_fault||sampling_fault||backend_fault||cache_fault||color_fault;",1);
    let cache = if pool_closed {
        runtime_cache::verilog_pool_closed(slots, native)?
    } else {
        runtime_cache::verilog(slots, native)?
    };
    let color = if reserved {
        runtime_color::source_reserved(native)
    } else {
        runtime_color::source(native)
    };
    Ok(source + "\n" + &cache + "\n" + &color)
}
