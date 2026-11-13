//! Application build and preview share the sole image authority and artifact reader.
use crate::{applications::Definition, process};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
pub type Settings = BTreeMap<&'static str, String>;
fn identity(path: &Path) -> Result<serde_json::Value, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))}))
}
pub fn show_path(path: &Path) -> String {
    path.display()
        .to_string()
        .trim_start_matches(r"\\?\")
        .to_owned()
}
pub fn cargo() -> Command {
    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}
pub fn write_receipt(output: &Path, mut value: serde_json::Value) -> Result<(), String> {
    fn visit(
        root: &Path,
        path: &Path,
        files: &mut serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), String> {
        for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_symlink() {
                return Err("run outputs cannot contain symbolic links".into());
            }
            if kind.is_dir() {
                visit(root, &path, files)?;
            } else if kind.is_file() && path != root.join("run.json") {
                files.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                    identity(&path)?,
                );
            }
        }
        Ok(())
    }
    let mut files = serde_json::Map::new();
    visit(output, output, &mut files)?;
    value["files"] = serde_json::Value::Object(files);
    value["run_id"] = serde_json::json!(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&value).unwrap())
    ));
    fs::write(
        output.join("run.json"),
        serde_json::to_vec_pretty(&value).unwrap(),
    )
    .map_err(|e| e.to_string())
}
pub fn system_root() -> Result<PathBuf, String> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .map_err(|e| e.to_string())
}
pub fn repository() -> Result<PathBuf, String> {
    system_root()?
        .join("../..")
        .canonicalize()
        .map_err(|e| e.to_string())
}
pub fn unique(root: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    for attempt in 0..100 {
        let id = format!(
            "{}-{}-{attempt}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos(),
            std::process::id()
        );
        let path = root.join(id);
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("could not allocate a unique run directory".into())
}
pub fn app_root(app: &Definition) -> Result<PathBuf, String> {
    Ok(repository()?.join("target/apps").join(app.name))
}
pub fn verify(snapshot: &Path, logs: &Path) -> Result<serde_json::Value, String> {
    let mut command = Command::new("python");
    command
        .args(["-X", "utf8", "-B"])
        .arg(system_root()?.join("tools/application_artifact.py"))
        .arg("check")
        .arg(snapshot);
    let result = process::logged(&mut command, logs, "artifact-check", 60)?;
    if !result.success {
        return Err(result.failure("artifact validation"));
    }
    serde_json::from_slice(&fs::read(result.stdout).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
pub fn build(
    app: &Definition,
    output: &Path,
    settings: &Settings,
    logs: &Path,
) -> Result<serde_json::Value, String> {
    crate::require_empty_output(output)?;
    fs::create_dir_all(logs).map_err(|e| e.to_string())?;
    let diagnostic = logs.join("diagnostic.json");
    let mut command = cargo();
    command
        .current_dir(repository()?)
        .args([
            "run",
            "--release",
            "-p",
            "cpu-v3-application",
            "--bin",
            "cpu-v3-boot-assets",
            "--",
        ])
        .arg(output)
        .env("PYTHONUTF8", "1")
        .envs(settings)
        .env("CPU_V3_BUILD_DIAGNOSTIC", &diagnostic);
    if let Some(selection) = app.selection {
        command.env("CPU_V3_BOOT_EXAMPLE", selection);
    } else {
        command.env_remove("CPU_V3_BOOT_EXAMPLE");
    }
    let result = process::logged(&mut command, logs, "build", 600)?;
    if !result.success {
        if let Ok(bytes) = fs::read(&diagnostic) {
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                if value["schema"] == "digitaldesign.build-diagnostic" && value["version"] == 1 {
                    if let Some(rendered) = value["rendered"].as_str() {
                        return Err(format!("{rendered}\nLogs: {}", logs.display()));
                    }
                }
            }
        }
        return Err(result.failure("application build"));
    }
    verify(output, logs)
}
pub struct Preview {
    pub window: bool,
    pub samples: usize,
    pub steps: usize,
    pub presents: Option<usize>,
    pub seconds: u64,
    pub xinput: Option<u32>,
}
pub fn preview(snapshot: &Path, output: &Path, options: &Preview) -> Result<(), String> {
    let logs = output.join("logs");
    let verified = verify(snapshot, &logs)?;
    let mut build = cargo();
    build.current_dir(repository()?).args([
        "build",
        "--release",
        "-p",
        "cpu-v3-tang-nano-20k",
        "--bin",
        "cpu-v3-display-sim",
        "--message-format=json-render-diagnostics",
    ]);
    if options.window {
        build.args(["--features", "display-window"]);
    }
    let result = process::logged(&mut build, &logs, "runner-build", 600)?;
    if !result.success {
        return Err(result.failure("simulator build"));
    }
    let messages = fs::read_to_string(result.stdout).map_err(|e| e.to_string())?;
    let executable = messages
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|v| {
            v["reason"] == "compiler-artifact" && v["target"]["name"] == "cpu-v3-display-sim"
        })
        .filter_map(|v| v["executable"].as_str().map(PathBuf::from))
        .next_back()
        .ok_or("Cargo did not report the simulator executable")?;
    let runner_identity = identity(&executable)?;
    // Execute the run-owned image so a Windows session never locks Cargo's output.
    let frozen = output.join(
        executable
            .file_name()
            .ok_or("simulator executable has no filename")?,
    );
    fs::copy(&executable, &frozen).map_err(|e| format!("freeze simulator runtime: {e}"))?;
    if identity(&frozen)? != runner_identity {
        return Err("frozen simulator runtime changed".into());
    }
    let executable = frozen;
    let mut command = Command::new(&executable);
    command
        .args(["--cpu-mode", "instruction", "--package"])
        .arg(snapshot.join("cpu-v3-boot.bin"))
        .arg("--artifact")
        .arg(snapshot)
        .arg("--out-dir")
        .arg(output.join("output"))
        .arg("--frames")
        .arg(options.samples.to_string())
        .arg("--max-cpu-steps")
        .arg(options.steps.to_string())
        .arg("--ppm");
    if let Some(n) = options.presents {
        command.arg("--stop-after-present").arg(n.to_string());
    }
    if options.window {
        command.arg("--window");
    }
    if options.window || options.xinput.is_some() {
        command.arg("--realtime");
    }
    if let Some(slot) = options.xinput {
        command.arg("--xinput-slot").arg(slot.to_string());
    }
    let result = process::logged(&mut command, &logs, "run", options.seconds)?;
    let receipt = serde_json::json!({"schema":"digitaldesign.application-run","version":1,
        "artifact_id":verified["artifact_id"],"snapshot":snapshot,"success":result.success,"timed_out":result.timed_out,"exit_code":result.code,
        "runner":executable,"runner_identity":runner_identity,
        "package_identity":std::fs::metadata(output.join("output/application.cb06")).ok().map(|_|identity(&output.join("output/application.cb06"))).transpose()?,
        "diagnostic_reader_identity":identity(&system_root()?.join("tools/application_artifact.py"))?,
        "fault_decoder_identity":identity(&system_root()?.join("tools/gpu_fault.py"))?,"samples":options.samples,"max_cpu_steps":options.steps,"presents":options.presents,"window":options.window,"realtime":options.window || options.xinput.is_some(),"xinput_slot":options.xinput,
        "summary":"output/summary-instruction.json","diagnostic":"output/diagnostic.json","logs":"logs"});
    write_receipt(output, receipt)?;
    if !result.success {
        return Err(result.failure("application run"));
    }
    println!(
        "Snapshot: {}\nPreview: {}",
        show_path(snapshot),
        show_path(&output.join("output/frame-final.ppm"))
    );
    Ok(())
}
