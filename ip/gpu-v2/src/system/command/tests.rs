use super::verilog;
use crate::{framebuffer::ports::MaterializedSurface, system::pixel::quad_ingress::rtl::Backend};

#[test]
fn dependency_closed_no_extra_store() {
    let s = verilog(
        MaterializedSurface {
            color_base_bytes: 0x100000,
            depth_base_bytes: 0x140000,
            width: 400,
            height: 240,
        },
        Backend::Portable,
    )
    .unwrap();
    assert!(!s.contains("module gpu_command_store"));
    assert!(!s.contains("module gpu_command_mesh("));
    // Selected-profile exclusions are checked after preprocessing by
    // tests/transport_equivalence.py; raw source keeps generic alternatives.
    assert!(!s.contains("module gpu_vliw_dma"));
    assert!(s.contains("module gpu_command_dma("));
    for legacy in [
        "slot_base_f",
        "slot_full_f",
        "slot_max_f",
        "slot_valid_f",
        "draw_config",
    ] {
        assert!(
            !s.contains(legacy),
            "old texture configuration survived: {legacy}"
        );
    }
    assert!(s.contains("if(reset||texture_invalidate)begin state_initialized<=0;"));
    assert!(s.contains("cfg_max_mip=texture_mip?{1'b0,texture_n}:5'd0"));
    assert!(s.contains("output[34:0]out_packet"));
    let cache = s
        .split("module gpu_v2_texture_runtime_cache_unique #")
        .nth(1)
        .unwrap()
        .split("endmodule")
        .next()
        .unwrap();
    assert!(cache.contains("head_tag={6'd0,head_sector[14:3]}"));
    assert!(!cache.contains("hk_row"));
    assert!(!cache.contains("layer_off_f"));
    assert!(!cache.contains("wire msx="));
    let light = s
        .split("module gpu_v2_lighting(")
        .nth(1)
        .unwrap()
        .split("endmodule")
        .next()
        .unwrap();
    assert!(!light.contains("configured"));
    assert!(!light.contains("context_valid"));
    assert!(!light.contains("context_ready"));
    // Generic alternatives remain in the source. The bounded maintenance
    // profile gate checks these identifiers after actual preprocessing.
    assert_eq!(s.matches("`define GPU_V2_COMMAND_MAINTENANCE").count(), 1);
    assert_eq!(s.matches("`undef GPU_V2_COMMAND_MAINTENANCE").count(), 1);
}
#[test]
#[ignore = "requires Icarus preprocessor; validates active native profile in simulation and synthesis"]
fn dependency_closed_native_profile() {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };
    let source = verilog(
        MaterializedSurface {
            color_base_bytes: 0x100000,
            depth_base_bytes: 0x140000,
            width: 400,
            height: 240,
        },
        Backend::Portable,
    )
    .unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!(
            "gpu-command-closure-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&root).unwrap();
    let input = root.join("command.v");
    std::fs::write(&input, source).unwrap();
    for synthesis in [false, true] {
        let output = root.join(if synthesis {
            "synthesis.pp"
        } else {
            "simulation.pp"
        });
        let log = std::fs::File::create(output.with_extension("log")).unwrap();
        let mut command =
            Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
        command.args(["-g2012", "-E", "-D__ICARUS__"]);
        if synthesis {
            command.arg("-DSYNTHESIS");
        }
        let mut child = command
            .arg("-o")
            .arg(&output)
            .arg(&input)
            .stdout(Stdio::null())
            .stderr(Stdio::from(log))
            .spawn()
            .unwrap();
        let started = Instant::now();
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(
                    status.success(),
                    "preprocessor failed: {}",
                    output.display()
                );
                break;
            }
            if started.elapsed() > Duration::from_secs(10) {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("native profile preprocessor exceeded10 seconds");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let s = std::fs::read_to_string(output).unwrap();
        for legacy in [
            "draw_bank",
            "loaded_bank",
            "COLOR_BASE",
            "DEPTH_BASE",
            "draw_rebind",
            "reg[31:0]far_z",
        ] {
            assert!(
                !s.contains(legacy),
                "old texture configuration survived: {legacy}"
            );
        }
        assert!(!s.contains("L_CONTEXT"));
    }
}
