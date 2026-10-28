//! Standalone connected RTL export. Board PLL, phase and pads stay outside.
use super::{arbiter, arbiter::*, streaming_port::*, RtlSources};
use digital_design_hardware::{HardwareIdentity, ModuleIo, VerilogProject};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The original module identities are retained in both the system and this
/// wrapper; exported arbiter logic comes from the same NAND implementation.
pub fn rtl_sources(init_cycles: u32) -> Result<BTreeMap<PathBuf, String>, String> {
    if !(1..=65535).contains(&init_cycles) {
        return Err("init cycles must be 1..65535".into());
    }
    let project = VerilogProject::generate::<CpuV3MemoryArbiter>().map_err(|e| e.to_string())?;
    let mut files = BTreeMap::new();
    for (path, source) in project.files {
        let name = PathBuf::from(path.file_name().ok_or("missing module file name")?);
        if files.insert(name, source).is_some() {
            return Err("duplicate standalone module file name".into());
        }
    }
    let input = idle_inputs();
    let output = super::arbiter::compute_output(&Default::default(), &input);
    let ins = CpuV3MemoryArbiterInput::verilog_values(&input);
    let outs = CpuV3MemoryArbiterOutput::verilog_values(&output);
    let mut ports = vec![
        "input wire logic_clk".to_string(),
        "input wire controller_clk".into(),
        "input wire sdram_clk".into(),
    ];
    let mut declarations = String::new();
    let mut links = vec![".clk(logic_clk)".to_string()];
    for (values, dir) in [(&ins, "input"), (&outs, "output")] {
        for v in values.iter() {
            let width = if v.width == 1 {
                String::new()
            } else {
                format!("[{}:0] ", v.width - 1)
            };
            if v.name.starts_with("memory_") || v.name == "lookahead_enable" {
                declarations.push_str(&format!("wire {width}{};\n", v.name));
            } else {
                ports.push(format!("{dir} wire {width}{}", v.name));
            }
            links.push(format!(".{}({})", v.name, v.name));
        }
    }
    ports.extend(
        [
            "output wire O_sdram_clk",
            "output wire O_sdram_cke",
            "output wire O_sdram_cs_n",
            "output wire O_sdram_cas_n",
            "output wire O_sdram_ras_n",
            "output wire O_sdram_wen_n",
            "output wire [3:0] O_sdram_dqm",
            "output wire [10:0] O_sdram_addr",
            "output wire [1:0] O_sdram_ba",
            "inout wire [31:0] IO_sdram_dq",
        ]
        .map(str::to_string),
    );
    let mut text = format!(
        "module GowinSdramCombination (\n{}\n);\n{}\n{} arbiter (\n{}\n);\n",
        ports.join(",\n"),
        declarations,
        CpuV3MemoryArbiter::verilog_identity().module_name(),
        links.join(",\n")
    );
    text.push_str(&include_str!("rtl/stream_connections.vh").replace(
        "__SHARED_PORT__",
        &StreamingSdramPort::verilog_identity().module_name(),
    ));
    text.push_str("\nendmodule\n");
    files.insert("combination.v".into(), text);
    files.insert(
        "shared_port.v".into(),
        RtlSources::STREAM_SHARED_PORT.replace(
            "module SharedSdramPort",
            &format!(
                "module {}",
                StreamingSdramPort::verilog_identity().module_name()
            ),
        ),
    );
    files.insert(
        "bridge.v".into(),
        RtlSources::STREAM_GEARBOX.replace(
            "parameter COLD_START_ONLY=0, INIT_CYCLES=21600",
            &format!("parameter COLD_START_ONLY=0, INIT_CYCLES={init_cycles}"),
        ),
    );
    files.insert("controller.v".into(), RtlSources::CONTROLLER.into());
    files.insert(
        "pins.v".into(),
        format!(
            "{}\n{}",
            RtlSources::PIN_MODEL,
            RtlSources::INPUT_DELAY_MODEL
        ),
    );
    let contract = super::profile::manifest(&files, init_cycles);
    files.insert("stream-contract.json".into(), contract);
    Ok(files)
}

/// All clients idle; feedback belongs to the selected test transport.
pub fn idle_outputs() -> arbiter::CpuV3MemoryArbiterOutputValue {
    arbiter::compute_output(&Default::default(), &idle_inputs())
}

/// All clients idle; feedback belongs to the selected test transport.
pub fn idle_inputs() -> arbiter::CpuV3MemoryArbiterInputValue {
    arbiter::CpuV3MemoryArbiterInputValue {
        reset: false,
        lookahead_enable: false,
        instruction_request_valid: false,
        instruction_address: 0,
        instruction_response_ready: false,
        data_request_valid: false,
        data_write: false,
        data_line: false,
        data_address: 0,
        data_write_data: 0,
        data_response_ready: false,
        dma_request_valid: false,
        dma_write: false,
        dma_address: 0,
        dma_write_data: 0,
        dma_response_ready: false,
        display_request_valid: false,
        display_address: 0,
        display_response_ready: false,
        gpu_ro_request_valid: false,
        gpu_ro_write: false,
        gpu_ro_address: 0,
        gpu_ro_line_count_minus_1: 0,
        gpu_ro_write_data: 0,
        gpu_fb_r_request_valid: false,
        gpu_fb_r_write: false,
        gpu_fb_r_address: 0,
        gpu_fb_r_line_count_minus_1: 0,
        gpu_fb_r_write_data: 0,
        gpu_fb_w_request_valid: false,
        gpu_fb_w_write: false,
        gpu_fb_w_address: 0,
        gpu_fb_w_line_count_minus_1: 0,
        gpu_fb_w_write_data: 0,
        memory_request_ready: false,
        memory_write_data_ready: false,
        memory_response_valid: false,
        memory_read_data: 0,
        memory_response_last: false,
        memory_error: false,
    }
}

/// Add caller-specific Verilog includes and regenerate the peer identity.
/// Reserved source and manifest names cannot be replaced by the caller.
pub fn rtl_sources_with_includes(
    init_cycles: u32,
    includes: impl IntoIterator<Item = (PathBuf, String)>,
) -> Result<BTreeMap<PathBuf, String>, String> {
    let mut files = rtl_sources(init_cycles)?;
    files.remove(std::path::Path::new("stream-contract.json"));
    for (path, source) in includes {
        if path
            .file_name()
            .is_none_or(|name| path != std::path::Path::new(name))
            || path.extension().is_none_or(|ext| ext != "vh")
            || files.contains_key(&path)
        {
            return Err("peer include must be a unique flat Verilog header".into());
        }
        files.insert(path, source);
    }
    let contract = super::profile::manifest(&files, init_cycles);
    files.insert("stream-contract.json".into(), contract);
    Ok(files)
}
