//! Exact generated application snapshot, materializer and qualification data.
use std::path::Path;
include!(concat!(env!("OUT_DIR"), "/boot_asset_bindings.rs"));

pub fn asset(name: &str) -> Option<&'static [u8]> {
    ASSETS
        .iter()
        .find(|(file, _)| *file == name)
        .map(|(_, bytes)| *bytes)
}
pub fn package() -> &'static [u8] {
    asset("cpu-v3-boot.bin").expect("generated application package")
}
pub fn materialize(output: &Path) -> Result<(), String> {
    cpu_v3_tools::require_empty_output(output)?;
    std::fs::create_dir_all(output)
        .map_err(|error| format!("cannot create {}: {error}", output.display()))?;
    // The manifest is the completion marker, published after every payload.
    for (name, bytes) in ASSETS
        .iter()
        .filter(|(name, _)| *name != "application.build.json")
    {
        write_new(&output.join(name), bytes)?;
    }
    let (_, manifest) = ASSETS
        .iter()
        .find(|(name, _)| *name == "application.build.json")
        .ok_or("missing generated application manifest")?;
    write_new(&output.join("application.build.json"), manifest)?;
    println!(
        "materialized {} generated CPU V3 boot artifacts -> {}",
        ASSETS.len(),
        output.display()
    );
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|error| format!("cannot create {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_assets_have_only_one_application_and_fixed_package() {
        assert!(ASSETS.len() >= 8);
        assert!(ASSETS.iter().all(|(name, _)| !name.starts_with('/')
            && !name.contains('\\')
            && !name.contains(':')
            && name.split('/').all(|part| !matches!(part, "" | "." | ".."))));
        let (_, bytes) = ASSETS
            .iter()
            .find(|(name, _)| *name == "application.build.json")
            .unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        let files = manifest["files"].as_object().unwrap();
        for name in [
            "application.v3bin",
            "application.static-words",
            "cpu-v3-boot.bin",
        ] {
            assert!(files.contains_key(name));
        }
        assert_eq!(files.len() + 1, ASSETS.len());
        assert!(ASSETS
            .iter()
            .all(|(name, _)| *name == "application.build.json" || files.contains_key(*name)));
        let (_, map) = ASSETS
            .iter()
            .find(|(name, _)| *name == "cpu-v3-boot.map")
            .unwrap();
        let map = std::str::from_utf8(map).unwrap();
        assert!(map
            .lines()
            .any(|line| matches!(line, "rcc_gpu_debug=false" | "rcc_gpu_debug=true")));

        assert!(!ASSETS.iter().any(|(name, _)| name.contains("stage0")
            || name.contains("s1")
            || name.contains("s2")
            || name.contains("manifest")));
    }
}
