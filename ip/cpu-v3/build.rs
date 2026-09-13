use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=CPU_V3_BTC_ENTRIES");
    let entries = env::var("CPU_V3_BTC_ENTRIES").unwrap_or_else(|_| "4".into());
    assert!(
        matches!(entries.as_str(), "0" | "4" | "8"),
        "CPU_V3_BTC_ENTRIES must be 0, 4, or 8"
    );
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(
        output.join("fetch_config.rs"),
        format!(
            "/// Build-time BTC capacity shared by the cycle model and generated RTL.\n\
             pub const CPU_V3_BTC_ENTRIES: usize = {entries};\n"
        ),
    )
    .unwrap();
}
