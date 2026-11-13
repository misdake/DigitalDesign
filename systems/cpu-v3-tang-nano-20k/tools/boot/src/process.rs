//! Bounded child processes with full output retained outside immutable snapshots.
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
pub struct Outcome {
    pub success: bool,
    pub timed_out: bool,
    pub code: Option<i32>,
    pub stdout: PathBuf,
    pub stderr: PathBuf,
}
pub fn logged(
    command: &mut Command,
    logs: &Path,
    name: &str,
    seconds: u64,
) -> Result<Outcome, String> {
    fs::create_dir_all(logs).map_err(|e| e.to_string())?;
    let stdout = logs.join(format!("{name}.stdout.log"));
    let stderr = logs.join(format!("{name}.stderr.log"));
    let description = serde_json::json!({"program":command.get_program().to_string_lossy(),
        "args":command.get_args().map(|s|s.to_string_lossy()).collect::<Vec<_>>(),"deadline_seconds":seconds});
    fs::write(
        logs.join(format!("{name}.command.json")),
        serde_json::to_vec_pretty(&description).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    command
        .stdout(Stdio::from(
            File::create(&stdout).map_err(|e| e.to_string())?,
        ))
        .stderr(Stdio::from(
            File::create(&stderr).map_err(|e| e.to_string())?,
        ));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().map_err(|error| {
        let message = format!("start {}: {error}", command.get_program().to_string_lossy());
        let _ = fs::write(
            logs.join(format!("{name}.result.json")),
            serde_json::to_vec_pretty(&serde_json::json!({"success":false,"start_error":message}))
                .unwrap(),
        );
        message
    })?;
    let start = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if start.elapsed() >= Duration::from_secs(seconds) {
            timed_out = true;
            #[cfg(windows)]
            {
                let _ = Command::new("taskkill")
                    .args(["/PID", &child.id().to_string(), "/T", "/F"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            let _ = child.kill();
            break child.wait().map_err(|e| e.to_string())?;
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    fs::write(
        logs.join(format!("{name}.result.json")),
        serde_json::to_vec_pretty(&serde_json::json!({
        "success":status.success() && !timed_out,"timed_out":timed_out,"exit_code":status.code(),
        "stdout":stdout,"stderr":stderr}))
        .unwrap(),
    )
    .map_err(|e| e.to_string())?;
    Ok(Outcome {
        success: status.success() && !timed_out,
        timed_out,
        code: status.code(),
        stdout,
        stderr,
    })
}
impl Outcome {
    pub fn failure(&self, stage: &str) -> String {
        let bytes = fs::read(&self.stderr).unwrap_or_default();
        let stderr = String::from_utf8_lossy(&bytes);
        let lines = stderr.lines().collect::<Vec<_>>();
        format!(
            "{stage} {} (exit {:?})\n{}\nLogs: {}",
            if self.timed_out {
                "timed out"
            } else {
                "failed"
            },
            self.code,
            lines[lines.len().saturating_sub(20)..].join("\n"),
            self.stderr.parent().unwrap().display()
        )
    }
}
