//! Export the actual shared arbiter/bridge/controller/pin peer for Stage3 RTL.
use digital_design_hardware_gowin::{
    sdram_memory_controller::{
        arbiter::{CpuV3MemoryArbiterInput, CpuV3MemoryArbiterOutput},
        combination,
    },
    ModuleIo,
};
use std::{fs, path::PathBuf};

#[test]
fn export_real_shared_sdram_peer() {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-vliw-integration-20261018/stage3/mc-peer");
    fs::create_dir_all(&out).unwrap();
    let i = combination::idle_inputs();
    let o = combination::idle_outputs();
    let mut declarations = String::new();
    for (values, input) in [
        (CpuV3MemoryArbiterInput::verilog_values(&i), true),
        (CpuV3MemoryArbiterOutput::verilog_values(&o), false),
    ] {
        for v in values {
            if v.name == "reset" || v.name == "lookahead_enable" || v.name.starts_with("memory_") {
                continue;
            }
            let width = format!("[{}:0]", v.width - 1);
            if input {
                let alias = if let Some(suffix) = v.name.strip_prefix("gpu_") {
                    let (prefix, suffix) = if let Some(s) = suffix.strip_prefix("ro_") {
                        ("ro", s)
                    } else if let Some(s) = suffix.strip_prefix("fb_r_") {
                        ("fb_r", s)
                    } else {
                        ("fb_w", suffix.strip_prefix("fb_w_").unwrap())
                    };
                    match suffix {
                        "write" => {
                            if prefix == "fb_w" {
                                "1'b1".into()
                            } else {
                                "1'b0".into()
                            }
                        }
                        "write_data" => {
                            if prefix == "fb_w" {
                                "fb_w_data".into()
                            } else {
                                "64'd0".into()
                            }
                        }
                        "line_count_minus_1" => format!("{prefix}_count"),
                        _ => format!("{prefix}_{suffix}"),
                    }
                } else {
                    format!("{}'h{:x}", v.width, v.value)
                };
                declarations.push_str(&format!("wire{width} {}={alias};\n", v.name));
            } else {
                declarations.push_str(&format!("wire{width} {};\n", v.name));
            }
        }
    }
    for (gpu, local) in [("ro", "ro"), ("fb_r", "fb_r"), ("fb_w", "fb_w")] {
        declarations.push_str(&format!(
            "assign {local}_request_ready=gpu_{gpu}_request_ready;\n"
        ));
        declarations.push_str(&format!(
            "assign {local}_response_valid=gpu_{gpu}_response_valid;\n"
        ));
        declarations.push_str(&format!(
            "assign {local}_response_last=gpu_{gpu}_response_last;\n"
        ));
        declarations.push_str(&format!("assign {local}_response_error=gpu_{gpu}_error;\n"));
        if gpu != "fb_w" {
            declarations.push_str(&format!("assign {local}_data=gpu_{gpu}_read_data;\n"));
        }
    }
    declarations.push_str("assign fb_w_data_ready=gpu_fb_w_write_data_ready;\n");
    let files =
        combination::rtl_sources_with_includes(21600, [("ports.vh".into(), declarations)]).unwrap();
    for (path, text) in files {
        fs::write(out.join(path), text).unwrap();
    }
    println!("Exported actual shared SDRAM RTL peer to {}", out.display());
}
