//! Fresh output directories for independently runnable Display fixtures.
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Every invocation owns a fresh directory, including parallel integration tests.
pub fn fresh_output(label: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let root = std::env::var_os("DISPLAY_CYCLE_OUTPUT_ROOT").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"),
        PathBuf::from,
    );
    fs::create_dir_all(&root).unwrap();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let output = root.join(format!(
        "{label}-{}-{nonce}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&output).unwrap();
    println!("{label} artifacts: {}", output.display());
    output
}
