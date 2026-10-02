//! Standalone connected RTL export. Board PLL, phase and pads stay outside.
use super::{arbiter::*, emu, shared_port::*, RtlSources};
use digital_design_hardware::{HardwareIdentity, ModuleIo, VerilogProject};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The original module identities are retained in both the system and this
/// wrapper; exported arbiter logic comes from the same NAND implementation.
pub fn rtl_sources(init_cycles: u32) -> Result<BTreeMap<PathBuf, String>, String> {
    rtl_sources_with_early_grant(init_cycles, false)
}

/// One irrevocable successor slot and native closed-bank preparation. Physical
/// READ/WRITE chaining remains disabled; final responses stay per transaction.
pub fn rtl_sources_with_early_grant(
    init_cycles: u32,
    early_grant: bool,
) -> Result<BTreeMap<PathBuf, String>, String> {
    rtl_sources_with_options(init_cycles, early_grant, false)
}

pub fn rtl_sources_with_options(
    init_cycles: u32,
    early_grant: bool,
    chained_groups: bool,
) -> Result<BTreeMap<PathBuf, String>, String> {
    if !(1..=65535).contains(&init_cycles) {
        return Err("init cycles must be 1..65535".into());
    }
    let project = VerilogProject::generate::<CpuV3MemoryArbiter>().map_err(|e| e.to_string())?;
    let mut files = project.files;
    let input = emu::idle_inputs();
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
    text.push_str(
        &include_str!("rtl/combination_connections.vh")
            .replace("__EARLY_GRANT__", if early_grant { "1'b1" } else { "1'b0" })
            .replace(
                "__CHAIN_GROUP_FOUR__",
                if chained_groups { "1'b1" } else { "1'b0" },
            ),
    );
    text = text.replace(
        "__SHARED_PORT__",
        &SharedSdramPort::verilog_identity().module_name(),
    );
    text.push_str("\nendmodule\n");
    files.insert("combination.v".into(), text);
    files.insert(
        "shared_port.v".into(),
        RtlSources::SHARED_PORT.replace(
            "module SharedSdramPort",
            &format!(
                "module {}",
                SharedSdramPort::verilog_identity().module_name()
            ),
        ),
    );
    files.insert(
        "bridge.v".into(),
        RtlSources::GEARBOX.replace(
            ".BANK_BIT(BANK_BIT)",
            &format!(".BANK_BIT(BANK_BIT), .INIT_CYCLES({init_cycles})"),
        ),
    );
    files.insert("controller.v".into(), RtlSources::CONTROLLER.into());
    files.insert("pins.v".into(), RtlSources::PIN_MODEL.into());
    Ok(files)
}
