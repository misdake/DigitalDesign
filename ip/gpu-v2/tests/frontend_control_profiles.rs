//! Real frontend profile and sticky-control regressions from the public factories.
use gpu_v2::{
    framebuffer::ports::MaterializedSurface, system::pixel::quad_ingress::rtl::Backend,
    texture::ports::Slot,
};
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires Python, Icarus and Gowin simulation primitives"]
fn frontend_control_profiles_and_sticky_stop() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = std::env::var_os("GPU_FRONTEND_CONTROL_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            repo.join("target/frontend-control-profiles")
                .join(std::process::id().to_string())
        });
    let exports = root.join("exports");
    std::fs::create_dir_all(&exports).unwrap();
    for (name, portable) in [("native", false), ("portable", true)] {
        let backend = if portable {
            Backend::Portable
        } else {
            Backend::Native
        };
        let surface = MaterializedSurface {
            color_base_bytes: 0x100000,
            depth_base_bytes: 0x140000,
            width: 400,
            height: 240,
        };
        let slots = [Slot {
            base_address: 0,
            max_size_log2: 0,
            has_full_mip: false,
            valid: true,
        }];
        let frontend = gpu_v2::frontend::vliw::verilog(portable);
        std::fs::write(exports.join(format!("frontend-{name}.v")), &frontend).unwrap();
        std::fs::write(
            exports.join(format!("frontend-command-ports-{name}.v")),
            format!("`define GPU_V2_COMMAND_PORTS\n{frontend}"),
        )
        .unwrap();
        std::fs::write(
            exports.join(format!("generic-{name}.v")),
            gpu_v2::system::vliw::verilog(&slots, surface, backend).unwrap(),
        )
        .unwrap();
        std::fs::write(
            exports.join(format!("command-{name}.v")),
            gpu_v2::system::vliw_command::verilog(surface, backend).unwrap(),
        )
        .unwrap();
    }
    let mut command = Command::new("python");
    command
        .arg(repo.join("ip/gpu-v2/tests/frontend_control_profiles.py"))
        .arg("--exports")
        .arg(&exports)
        .arg("--output")
        .arg(root.join("results"));
    if let Some(baseline) = std::env::var_os("GPU_FRONTEND_CONTROL_BASELINE") {
        command.arg("--baseline").arg(baseline);
    }
    let mut child = command.spawn().unwrap();
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "frontend control contract failed");
            break;
        }
        if started.elapsed() > Duration::from_secs(240) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("frontend control contract exceeded wall watchdog");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
