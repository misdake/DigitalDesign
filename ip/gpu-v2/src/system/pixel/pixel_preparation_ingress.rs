//! One real Quad/Lighting/Runtime preparation top through actual Group72 output.
//! Immutable Draw/public credit, cache/Color and result/join/ROP remain parents.
use super::{pixel_runtime_ingress, quad_ingress};
use crate::texture::rtl::{runtime_backend, runtime_rows::Profile};

pub const MODULE: &str = "gpu_v2_pixel_preparation_ingress";

pub fn verilog(backend: quad_ingress::rtl::Backend) -> Result<String, String> {
    verilog_profile(backend, false, Profile::default())
}

/// Matched static experiment for the proved canonical C producer.
pub fn verilog_canonical(backend: quad_ingress::rtl::Backend) -> Result<String, String> {
    verilog_profile(backend, true, Profile::default())
}

pub fn verilog_with_preparation(
    backend: quad_ingress::rtl::Backend,
    preparation: Profile,
) -> Result<String, String> {
    verilog_profile(backend, false, preparation)
}

fn verilog_profile(
    backend: quad_ingress::rtl::Backend,
    canonical_cost: bool,
    preparation: Profile,
) -> Result<String, String> {
    let native = matches!(backend, quad_ingress::rtl::Backend::Native);
    let front = pixel_runtime_ingress::verilog_profile(backend, preparation)?;
    let port = "output wire sample_out_valid,input wire sample_out_ready,";
    let end = front
        .find("endmodule")
        .ok_or("Pixel preparation top missing")?;
    let instance = r#"
 gpu_v2_texture_runtime_backend backend(
 .clk(clk),.reset(reset),.ce(ce),.commit_allow(allow),.abort(1'b0),
 .in_valid(sample_out_valid),.in_owner(sample_out_owner),.in_result(sample_out_result),
 .in_ready(sample_out_ready),.in_accept(coefficient_accept),.advance(),.preflight_bad(backend_bad),.fault(backend_fault),
 .packet_enable(packet_enable),.out_ready(packet_out_ready),.out_valid(packet_out_valid),.out_payload(packet_out_payload),
 .work_reserved(work_reserved),.work_count(work_count),.producer_count(producer_count),.unwritten_count(unwritten_count),
 .group_count(group_count),.row_count(row_count),.pool_strobes(),
 .coefficient_valid(),.coefficient_consume(),.member_issue(),.member_return(),.reserved_cost(),
 .coefficient_weights(),.coefficient_meta(),.member_payload(),.packet_capture(),.work_ack());
"#;
    let newports = "output wire sample_out_valid,sample_out_ready,\n input wire packet_enable,packet_out_ready,\n output wire packet_out_valid,coefficient_accept,output wire[71:0]packet_out_payload,\n output wire[4:0]work_reserved,work_count,producer_count,unwritten_count,\n output wire[5:0]group_count,row_count,";
    assert!(front.contains(port));
    let front = format!("{}\n{}{}", &front[..end], instance, &front[end..])
        .replacen(
            "module gpu_v2_pixel_runtime_ingress(",
            &format!("module {MODULE}("),
            1,
        )
        .replacen(port, newports, 1)
        .replacen(
            "wire quad_bad,sampling_bad,quad_fault,sampling_fault;",
            "wire quad_bad,sampling_bad,quad_fault,sampling_fault,backend_bad,backend_fault;",
            1,
        )
        .replacen(
            "assign preflight_bad=quad_bad||sampling_bad;",
            "assign preflight_bad=quad_bad||sampling_bad||backend_bad;",
            1,
        )
        .replacen(
            "assign fault=quad_fault||sampling_fault;",
            "assign fault=quad_fault||sampling_fault||backend_fault;",
            1,
        );
    if canonical_cost {
        Ok(front.replacen(
            runtime_backend::MODULE,
            runtime_backend::CANONICAL_MODULE,
            1,
        ) + "\n"
            + &runtime_backend::verilog_canonical(native)?)
    } else {
        Ok(front + "\n" + &runtime_backend::verilog(native)?)
    }
}
