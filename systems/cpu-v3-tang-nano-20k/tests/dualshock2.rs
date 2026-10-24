//! Independent serial peers exercise the production engine and actual CPU.
use cpu_v3_tang_nano_20k::DualShock2;
use digital_design_hardware::{GeneratedVerilogProject, HardwareIdentity, VerilogProject};
use std::{collections::BTreeMap, path::PathBuf, process::Command};

fn run_rtl(name: &str, projects: Vec<GeneratedVerilogProject>, tb: String) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/dualshock2-validation")
        .join(name);
    std::fs::create_dir_all(&root).unwrap();
    let mut files = BTreeMap::new();
    for project in projects {
        for (path, source) in project.files {
            if !source.contains("module tb") {
                if let Some(previous) = files.insert(path, source.clone()) {
                    assert_eq!(previous, source);
                }
            }
        }
    }
    let mut paths = Vec::new();
    for (i, source) in files.values().enumerate() {
        let path = root.join(format!("source_{i}.v"));
        std::fs::write(&path, source).unwrap();
        paths.push(path);
    }
    let path = root.join("tb.v");
    std::fs::write(&path, tb).unwrap();
    let image = root.join("tb.vvp");
    let output =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()))
            .args(["-g2012", "-s", "tb", "-o"])
            .arg(&image)
            .args(paths)
            .arg(path)
            .output()
            .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()))
        .arg(image)
        .current_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .output()
        .unwrap();
    std::fs::write(root.join("result.log"), &output.stdout).unwrap();
    assert!(
        output.status.success() && String::from_utf8_lossy(&output.stdout).contains("DS2_PASS"),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "bounded production RTL with independent LSB serial controller"]
fn byte_engine_serial_peer() {
    run_rtl(
        "engine",
        vec![VerilogProject::generate::<DualShock2>().unwrap()],
        include_str!("dualshock2_engine_tb.v")
            .replace("__DS2__", &DualShock2::verilog_identity().module_name()),
    );
}

#[test]
#[ignore = "bounded actual CPU, D-cache, Device0 clock/watch and serial Device5"]
fn actual_cpu_configures_and_polls_serial_peer() {
    use cpu_v3::{rcc_backend, CpuV3Core, CpuV3DataCache};
    use cpu_v3_tang_nano_20k::SystemControlDevice;
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let parsed = rcc::frontend::compile_program_named(
        "DS2 CPU fixture",
        include_str!("dualshock2_cpu.rcc"),
        &options,
        &mut |name| {
            std::fs::read_to_string(root.join("rcc").join(format!("{name}.rs")))
                .map_err(|e| e.to_string())
        },
    )
    .unwrap();
    let code = rcc_backend::try_compile(parsed, &options, "main").unwrap();
    let init = code
        .words
        .iter()
        .enumerate()
        .map(|(i, w)| format!("rom[{i}]=16'h{w:04x};\n"))
        .collect::<String>();
    let prefix = include_str!("cache_maintenance_protocol_tb.v")
        .split_once("integer commands=")
        .unwrap()
        .0;
    let mut tb = prefix
        .replace("__CASE__", "0")
        .replace("__RESULT__", "0")
        .replace(
            "integer cycles=0;",
            "integer cycles=0;integer pause_left=0;wire [15:0] ds2_read_data;",
        )
        .replace("rom[0:255]", "rom[0:65535]")
        .replace("rom[iaddr[7:0]]", "rom[iaddr[15:0]]")
        .replace("cycles>5000", "cycles>30000000")
        .replace(
            "wire finite_pause = CASE==1 && dresp && pause_count<5;",
            "wire finite_pause = pause_left>0;",
        );
    let start = tb.find("__SYSCTL__ sysctl(").unwrap();
    let end = tb[start..].find("__CACHE__ cache(").unwrap() + start;
    let instance=tb[start..end].replace(".device_index(device_index)",".device_index(query_index)")
        .replace(".device_channel(device_channel)",".device_channel(query_channel)")
        .replace(".device_read_enable(device_read_enable)",".device_read_enable(device_read_enable||watch_enable)")
        .replace(".device_write_enable(device_write_enable)",".device_write_enable(device_write_enable&&!watch_enable)")
        .replace(".device_read_data(device_read_data)",".device_read_data(sysctl_data)")
        .replace(".watch_read_data(16'd0),",".watch_read_data(device_read_data),.watch_device_index(watch_index),.watch_device_channel(watch_channel),.watch_read_enable(watch_enable),");
    tb.replace_range(start..end,&format!("wire [2:0] watch_index;wire [3:0] watch_channel;wire watch_enable;wire [15:0] sysctl_data;\nwire [2:0] query_index=watch_enable?watch_index:device_index;\nwire [3:0] query_channel=watch_enable?watch_channel:device_channel;\nassign device_read_data=sysctl_data|ds2_read_data;\n{instance}"));
    tb.push_str(include_str!("dualshock2_cpu_peer.vh"));
    tb = tb
        .replace("__PROGRAM__", &init)
        .replace("__CORE__", &CpuV3Core::verilog_identity().module_name())
        .replace(
            "__CACHE__",
            &CpuV3DataCache::verilog_identity().module_name(),
        )
        .replace(
            "__SYSCTL__",
            &SystemControlDevice::<4, 54000>::verilog_identity().module_name(),
        )
        .replace("__DS2__", &DualShock2::verilog_identity().module_name());
    run_rtl(
        "cpu",
        vec![
            VerilogProject::generate::<CpuV3Core>().unwrap(),
            VerilogProject::generate::<CpuV3DataCache>().unwrap(),
            VerilogProject::generate::<SystemControlDevice<4, 54000>>().unwrap(),
            VerilogProject::generate::<DualShock2>().unwrap(),
        ],
        tb,
    );
}
