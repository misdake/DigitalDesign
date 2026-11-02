use super::*;
pub(crate) struct Pin {
    pub name: &'static str,
    pub width: usize,
    pub value: u64,
    pub output: bool,
    pub care: bool,
}
pub(crate) fn fields(e: Edge) -> Vec<Pin> {
    let i = e.input;
    let d = e.output.dma;
    let r = e.output.router;
    let mut p = Vec::new();
    macro_rules! input {
        ($name:literal,$width:expr,$v:expr) => {
            p.push(Pin {
                name: $name,
                width: $width,
                value: u64::from($v),
                output: false,
                care: true,
            });
        };
    }
    macro_rules! output {
        ($name:literal,$width:expr,$v:expr) => {
            p.push(Pin {
                name: $name,
                width: $width,
                value: u64::from($v),
                output: true,
                care: true,
            });
        };
    }
    input!("reset", 1, i.reset);
    input!("ce", 1, i.ce);
    input!("stop", 1, i.stop);
    input!("cache_valid", 1, i.cache.valid);
    input!("cache_source", 22, i.cache.source_halfword);
    input!("cache_bank", 1, i.cache.bank);
    input!("work_valid", 1, i.work.valid);
    input!("work_mesh", 1, i.work.mesh);
    input!("work_source", 22, i.work.source_halfword);
    input!("work_destination", 7, i.work.destination);
    input!("work_size", 2, i.work.size);
    input!("work_blocks", 3, i.work.blocks);
    input!("work_slot", 1, i.work.slot);
    input!("texture_request_valid", 1, i.texture.valid);
    input!("texture_request_address", 32, i.texture.address_bytes);
    input!("framebuffer_request_valid", 1, i.framebuffer.valid);
    input!("framebuffer_request_write", 1, i.framebuffer.writing);
    input!(
        "framebuffer_request_address",
        32,
        i.framebuffer.address_bytes
    );
    input!("framebuffer_write_valid", 1, i.framebuffer.write_valid);
    input!("framebuffer_write_data", 64, i.framebuffer.write_data);
    input!("ro_request_ready", 1, i.physical[0].request_ready);
    input!("ro_response_valid", 1, i.physical[0].response.valid);
    input!("ro_response_last", 1, i.physical[0].response.last);
    input!("ro_response_error", 1, i.physical[0].response.error);
    input!("ro_data", 64, i.physical[0].response.data);
    input!("fb_r_request_ready", 1, i.physical[1].request_ready);
    input!("fb_r_response_valid", 1, i.physical[1].response.valid);
    input!("fb_r_response_last", 1, i.physical[1].response.last);
    input!("fb_r_response_error", 1, i.physical[1].response.error);
    input!("fb_r_data", 64, i.physical[1].response.data);
    input!("fb_w_request_ready", 1, i.physical[2].request_ready);
    input!("fb_w_data_ready", 1, i.physical[2].write_ready);
    input!("fb_w_response_valid", 1, i.physical[2].response.valid);
    input!("fb_w_response_last", 1, i.physical[2].response.last);
    input!("fb_w_response_error", 1, i.physical[2].response.error);
    output!("cache_ready", 1, d.cache_ready);
    output!("cache_done", 1, d.cache_done);
    output!("cache_done_bank", 1, d.cache_done_bank);
    output!("work_ready", 1, d.work_ready);
    output!("work_busy", 1, d.work_busy);
    output!("work_done", 1, d.work_done);
    output!("mesh_begin", 1, d.mesh_begin);
    output!("mesh_done", 1, d.mesh_done);
    output!("mesh_slot", 1, d.mesh_slot);
    output!("mesh_begin_slot", 1, d.mesh_begin_slot);
    output!("mesh_done_slot", 1, d.mesh_done_slot);
    output!("uniform_done", 1, d.uniform_done);
    output!("uniform_bank", 2, d.uniform_bank);
    output!("dma_we", 1, d.ram_write);
    output!("dma_addr", 9, d.ram_address);
    output!("dma_data", 64, d.ram_data);
    output!("dma_request_valid", 1, d.request.valid);
    for (name, width, value) in [
        (
            "dma_request_address",
            22,
            u64::from(d.request.address_halfword),
        ),
        ("dma_request_count", 2, u64::from(d.request.count)),
    ] {
        p.push(Pin {
            name,
            width,
            value,
            output: true,
            care: d.request.valid,
        });
    }
    output!("dma_fault", 1, d.fault);
    output!("dma_idle", 1, d.idle);
    output!("mesh_loading", 1, d.mesh_loading);
    output!("dma_request_ready", 1, r.dma_ready);
    output!("dma_response_valid", 1, r.dma_response.valid);
    output!("dma_response_last", 1, r.dma_response.last);
    output!("dma_response_error", 1, r.dma_response.error);
    output!("dma_response_bad", 1, r.dma_bad);
    output!("dma_response_data", 64, r.dma_response.data);
    output!("texture_request_ready", 1, r.texture_ready);
    output!("texture_read_valid", 1, r.texture_read);
    output!("texture_read_index", 4, r.texture_index);
    output!("texture_read_data", 64, r.texture_data);
    output!("texture_complete", 1, r.texture_complete);
    output!("texture_ok", 1, r.texture_ok);
    output!("framebuffer_request_ready", 1, r.framebuffer_ready);
    output!("framebuffer_write_ready", 1, r.framebuffer_write_ready);
    output!("framebuffer_read_valid", 1, r.framebuffer_read);
    output!("framebuffer_read_data", 64, r.framebuffer_data);
    output!("framebuffer_complete", 1, r.framebuffer_complete);
    output!("framebuffer_ok", 1, r.framebuffer_ok);
    output!("ro_request_valid", 1, r.physical[0].valid);
    output!("ro_address", 22, r.physical[0].address_halfword);
    output!("ro_count", 2, r.physical[0].count);
    output!("fb_r_request_valid", 1, r.physical[1].valid);
    output!("fb_r_address", 22, r.physical[1].address_halfword);
    output!("fb_r_count", 2, r.physical[1].count);
    output!("fb_w_request_valid", 1, r.physical[2].valid);
    output!("fb_w_address", 22, r.physical[2].address_halfword);
    output!("fb_w_count", 2, r.physical[2].count);
    output!("fb_w_data", 64, r.write_data);
    output!("router_busy", 1, r.busy);
    output!("router_fault", 1, r.fault);
    p
}

pub(crate) fn wrapper() -> String {
    let mut source = String::from("module p1a(input clk");
    for f in fields(Edge {
        input: Input::default(),
        output: Output::default(),
    }) {
        write!(
            source,
            ",{} [{}:0] {}",
            if f.output { "output" } else { "input" },
            f.width - 1,
            f.name
        )
        .unwrap();
    }
    source.push_str(");\ngpu_command_dma dma(.clk(clk)");
    for name in [
        "reset",
        "ce",
        "stop",
        "cache_valid",
        "cache_ready",
        "cache_source",
        "cache_bank",
        "cache_done",
        "cache_done_bank",
        "work_valid",
        "work_ready",
        "work_mesh",
        "work_source",
        "work_destination",
        "work_size",
        "work_blocks",
        "work_slot",
        "work_busy",
        "work_done",
        "mesh_begin",
        "mesh_done",
        "mesh_slot",
        "mesh_begin_slot",
        "mesh_done_slot",
        "uniform_done",
        "uniform_bank",
        "dma_we",
        "dma_addr",
        "dma_data",
        "mesh_loading",
    ] {
        write!(source, ",.{name}({name})").unwrap();
    }
    for (name, pin) in [
        ("request_valid", "dma_request_valid"),
        ("request_ready", "dma_request_ready"),
        ("request_address", "dma_request_address"),
        ("request_count", "dma_request_count"),
        ("response_valid", "dma_response_valid"),
        ("response_last", "dma_response_last"),
        ("response_error", "dma_response_error"),
        ("response_bad", "dma_response_bad"),
        ("response_data", "dma_response_data"),
        ("idle", "dma_idle"),
        ("fault", "dma_fault"),
    ] {
        write!(source, ",.{name}({pin})").unwrap();
    }
    source.push_str(");\ngpu_vliw_memory_router #(.CONTROL(0)) router(.clk(clk),.reset(reset),.control_request_valid(1'b0),.control_request_address(22'd0),.control_request_count(2'd0)");
    for (name, pin) in [
        ("geometry_request_valid", "dma_request_valid"),
        ("geometry_request_ready", "dma_request_ready"),
        ("geometry_request_address", "dma_request_address"),
        ("geometry_request_count", "dma_request_count"),
        ("geometry_response_valid", "dma_response_valid"),
        ("geometry_response_last", "dma_response_last"),
        ("geometry_response_error", "dma_response_error"),
        ("geometry_response_bad", "dma_response_bad"),
        ("geometry_response_data", "dma_response_data"),
        ("busy", "router_busy"),
        ("fault", "router_fault"),
    ] {
        write!(source, ",.{name}({pin})").unwrap();
    }
    for name in [
        "texture_request_valid",
        "texture_request_address",
        "texture_request_ready",
        "texture_read_valid",
        "texture_read_index",
        "texture_read_data",
        "texture_complete",
        "texture_ok",
        "framebuffer_request_valid",
        "framebuffer_request_write",
        "framebuffer_request_address",
        "framebuffer_request_ready",
        "framebuffer_write_valid",
        "framebuffer_write_data",
        "framebuffer_write_ready",
        "framebuffer_read_valid",
        "framebuffer_read_data",
        "framebuffer_complete",
        "framebuffer_ok",
        "ro_request_valid",
        "ro_request_ready",
        "ro_address",
        "ro_count",
        "ro_response_valid",
        "ro_response_last",
        "ro_response_error",
        "ro_data",
        "fb_r_request_valid",
        "fb_r_request_ready",
        "fb_r_address",
        "fb_r_count",
        "fb_r_response_valid",
        "fb_r_response_last",
        "fb_r_response_error",
        "fb_r_data",
        "fb_w_request_valid",
        "fb_w_request_ready",
        "fb_w_address",
        "fb_w_count",
        "fb_w_data",
        "fb_w_data_ready",
        "fb_w_response_valid",
        "fb_w_response_last",
        "fb_w_response_error",
    ] {
        write!(source, ",.{name}({name})").unwrap();
    }
    source.push_str(");\nendmodule\n");
    source
}
