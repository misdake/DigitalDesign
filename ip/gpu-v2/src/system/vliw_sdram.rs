//! Full VLIW GPU connected to the main halfword22/64-bit three-master boundary.
use crate::{
    framebuffer::ports::MaterializedSurface, system::pixel::quad_ingress, texture::ports::Slot,
};

pub const MODULE: &str = "gpu_v2_vliw_sdram";

const OLD_PORTS: &str = " output mem_req_valid,mem_req_write,output[31:0]mem_req_addr,input mem_req_ready,\n input[63:0]mem_read_data,input mem_read_valid,\n output mem_write_valid,output[63:0]mem_write_data,input mem_write_ready,mem_complete_valid,mem_complete_ok";
const NEW_PORTS: &str = " output texture_request_valid,output[31:0]texture_request_address,\n input texture_request_ready,texture_read_valid,input[3:0]texture_read_index,\n input[63:0]texture_read_data,input texture_complete,texture_ok,\n output framebuffer_request_valid,framebuffer_request_write,output[31:0]framebuffer_request_address,\n input framebuffer_request_ready,framebuffer_read_valid,input[63:0]framebuffer_read_data,\n output framebuffer_write_valid,output[63:0]framebuffer_write_data,\n input framebuffer_write_ready,framebuffer_complete,framebuffer_ok";
const OLD_CONNECTIONS: &str = " .mem_req_valid(mem_req_valid),.mem_req_write(mem_req_write),.mem_req_addr(mem_req_addr),.mem_req_ready(mem_req_ready),\n .mem_read_data(mem_read_data),.mem_read_valid(mem_read_valid),.mem_write_valid(mem_write_valid),.mem_write_data(mem_write_data),\n .mem_write_ready(mem_write_ready),.mem_complete_valid(mem_complete_valid),.mem_complete_ok(mem_complete_ok)";
const NEW_CONNECTIONS: &str = " .texture_request_valid(texture_request_valid),.texture_request_address(texture_request_address),\n .texture_request_ready(texture_request_ready),.texture_read_valid(texture_read_valid),.texture_read_index(texture_read_index),\n .texture_read_data(texture_read_data),.texture_complete(texture_complete),.texture_ok(texture_ok),\n .framebuffer_request_valid(framebuffer_request_valid),.framebuffer_request_write(framebuffer_request_write),.framebuffer_request_address(framebuffer_request_address),\n .framebuffer_request_ready(framebuffer_request_ready),.framebuffer_read_valid(framebuffer_read_valid),.framebuffer_read_data(framebuffer_read_data),\n .framebuffer_write_valid(framebuffer_write_valid),.framebuffer_write_data(framebuffer_write_data),\n .framebuffer_write_ready(framebuffer_write_ready),.framebuffer_complete(framebuffer_complete),.framebuffer_ok(framebuffer_ok)";

pub fn verilog(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    let source = compute_verilog(slots, surface, backend)?;
    Ok(format!(
        "{}\n{source}\n{}",
        include_str!("vliw_sdram.v"),
        super::vliw_memory::verilog()
    ))
}

pub(super) fn compute_verilog(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    let source = super::vliw::verilog(slots, surface, backend)?;
    let mut source = super::vliw_memory::split_backend(&source)?;
    // Bind only the enclosing geometry/backend module. A private cache may
    // legitimately expose the same memory-port spelling as this wrapper.
    let marker = "module gpu_v2_vliw(";
    if source.matches(marker).count() != 1 {
        return Err("VLIW full memory top declaration changed".into());
    }
    let start = source.find(marker).unwrap();
    let end = start
        + source[start..]
            .find("endmodule")
            .ok_or("VLIW full memory top endmodule")?
        + 9;
    let mut top = source[start..end].to_owned();
    for (a, b) in [
        ("module gpu_v2_vliw(", "module gpu_v2_vliw_three_port("),
        (
            " gpu_v2_vliw_backend back(",
            " gpu_v2_vliw_backend_three_port back(",
        ),
        (OLD_PORTS, NEW_PORTS),
        (OLD_CONNECTIONS, NEW_CONNECTIONS),
    ] {
        if top.matches(a).count() != 1 {
            return Err(format!("VLIW full memory binding changed: {a:?}"));
        }
        top = top.replacen(a, b, 1);
    }
    source.replace_range(start..end, &top);
    Ok(source)
}
