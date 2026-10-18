//! Independent error-retention oracle: no expected result comes from the cache model.
use super::*;
use crate::hardware::fetch::{CpuV3InstructionFetchQueue, CPU_V3_BTC_ENTRIES};

#[test]
#[ignore = "bounded I-cache held error plus hit/miss successor, portable cache RAM"]
fn icache_held_error_direct_portable() {
    held_error_replay(false, false);
}

#[test]
#[ignore = "bounded Fetch plus I-cache full-FIFO error retention, portable cache RAM"]
fn icache_held_error_integrated_portable() {
    held_error_replay(true, false);
}

#[test]
#[ignore = "bounded I-cache held error through official DPB; requires GOWIN_HOME"]
fn icache_held_error_direct_vendor() {
    held_error_replay(false, true);
}

#[test]
#[ignore = "bounded Fetch plus I-cache full-FIFO error through official DPB; requires GOWIN_HOME"]
fn icache_held_error_integrated_vendor() {
    held_error_replay(true, true);
}

fn held_error_replay(integrated: bool, vendor: bool) {
    let primitive = vendor.then(|| {
        let home = std::path::PathBuf::from(
            std::env::var_os("GOWIN_HOME")
                .expect("held-error official DPB test requires GOWIN_HOME"),
        );
        let p = [
            home.join("IDE/simlib/gw2a/prim_sim.v"),
            home.join("simlib/gw2a/prim_sim.v"),
        ]
        .into_iter()
        .find(|p| p.is_file())
        .expect("held-error official DPB library not found under GOWIN_HOME");
        p.canonicalize().unwrap()
    });
    let mut data = CpuV3DualPortCacheData::<ZeroBsramImage>::verilog_source().unwrap();
    if vendor {
        let (ports, _) = data
            .split_once(");\n")
            .expect("cache data module header drift");
        data = format!(
            "{ports});\n{}",
            include_str!("cpu_v3_data_cache_vendor_data_body.vh")
        );
    }
    let sources = [
        data,
        CpuV3DataCacheMetadata::verilog_source().unwrap(),
        CpuV3TwoWayCache::verilog_source().unwrap(),
        CpuV3InstructionCache::verilog_source().unwrap(),
        CpuV3InstructionFetchQueue::verilog_source().unwrap(),
    ]
    .join("\n");
    let tb = if integrated {
        include_str!("icache_held_error_integrated_tb.v")
    } else {
        include_str!("icache_held_error_direct_tb.v")
    };
    let component = if integrated { "integrated" } else { "direct" };
    let mode = if vendor { "vendor" } else { "portable" };
    let directory = std::env::temp_dir().join(format!(
        "icache-held-error-{}-{CPU_V3_BTC_ENTRIES}-{component}-{mode}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("modules.v"), sources).unwrap();
    std::fs::write(directory.join("tb.v"), tb).unwrap();
    if let Some(p) = &primitive {
        std::fs::write(
            directory.join("primitive-path.txt"),
            p.display().to_string(),
        )
        .unwrap();
    }
    let mut command = std::process::Command::new(
        std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()),
    );
    command
        .current_dir(&directory)
        .args(["-g2005", "-s", "tb", "-o", "sim.vvp"]);
    if primitive.is_some() {
        command.args(["-s", "GSR", "-DCPU_V3_CACHE_METADATA_VENDOR"]);
    }
    command.args(["modules.v", "tb.v"]);
    if let Some(p) = &primitive {
        command.arg(p);
    }
    let (status, stdout, stderr) = held_error_command(&mut command, &directory, "compile");
    assert!(status.success(), "held-error compile: {stdout}\n{stderr}");
    let mut command =
        std::process::Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
    command.current_dir(&directory).arg("sim.vvp");
    let (status, stdout, stderr) = held_error_command(&mut command, &directory, "run");
    if let Some(root) = std::env::var_os("CPU_V3_ICACHE_ERROR_EVIDENCE") {
        let evidence = std::path::PathBuf::from(root)
            .join(format!("entries-{CPU_V3_BTC_ENTRIES}/{component}-{mode}"));
        std::fs::create_dir_all(&evidence).unwrap();
        for name in [
            "modules.v",
            "tb.v",
            "compile.stdout.log",
            "compile.stderr.log",
            "run.stdout.log",
            "run.stderr.log",
        ] {
            std::fs::copy(directory.join(name), evidence.join(name)).unwrap();
        }
        if vendor {
            std::fs::copy(
                directory.join("primitive-path.txt"),
                evidence.join("primitive-path.txt"),
            )
            .unwrap();
        }
    }
    let pass = if integrated {
        "HELD_ERROR_INTEGRATED_PASS"
    } else {
        "HELD_ERROR_DIRECT_PASS"
    };
    assert!(
        status.success() && stdout.contains(pass),
        "held-error {component}-{mode}: {stdout}\n{stderr}"
    );
    println!("ICACHE_HELD_ERROR entries={CPU_V3_BTC_ENTRIES} {component}-{mode}\n{stdout}");
    let resolved = directory.canonicalize().unwrap();
    let root = std::env::temp_dir().canonicalize().unwrap();
    assert!(resolved.starts_with(&root) && resolved != root);
    std::fs::remove_dir_all(resolved).unwrap();
}

fn held_error_command(
    command: &mut std::process::Command,
    directory: &std::path::Path,
    label: &str,
) -> (std::process::ExitStatus, String, String) {
    let out = directory.join(format!("{label}.stdout.log"));
    let err = directory.join(format!("{label}.stderr.log"));
    let mut child = command
        .stdout(std::fs::File::create(&out).unwrap())
        .stderr(std::fs::File::create(&err).unwrap())
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if start.elapsed() > std::time::Duration::from_secs(60) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!(
                "held-error {label} exceeded 60 seconds; logs in {}",
                directory.display()
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    (
        status,
        std::fs::read_to_string(out).unwrap(),
        std::fs::read_to_string(err).unwrap(),
    )
}
