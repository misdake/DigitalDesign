use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
pub fn run_tool(mut command: Command, directory: &Path, label: &str) {
    let stdout = fs::File::create(directory.join(format!("{label}.stdout"))).unwrap();
    let stderr = fs::File::create(directory.join(format!("{label}.stderr"))).unwrap();
    command
        .current_dir(directory)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    let mut child = command.spawn().unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "{label} failed: {}",
                fs::read_to_string(directory.join(format!("{label}.stderr"))).unwrap()
            );
            break;
        }
        if start.elapsed() > Duration::from_secs(15) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("{label} exceeded15s; evidence {}", directory.display());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
