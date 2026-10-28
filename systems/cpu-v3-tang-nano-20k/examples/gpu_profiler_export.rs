//! Export the current production GPU for observer-only profiler smoke tests.
use cpu_v3_tang_nano_20k::CpuV3Gpu;
use digital_design_hardware::VerilogProject;
use digital_design_hardware_gowin::sdram_memory_controller::combination;
use std::{collections::BTreeSet, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = PathBuf::from(std::env::args().nth(1).ok_or("Expected output directory")?);
    std::fs::create_dir_all(&out)?;
    match option_env!("CPU_V3_GPU_PROFILE") {
        None | Some("production") | Some("texture16x8-column16") => {}
        Some(profile) => {
            return Err(format!("Unsupported profiler GPU storage profile: {profile}").into())
        }
    }
    let mut project = VerilogProject::generate::<CpuV3Gpu>()?;
    // Inspect the generated source, not just the requested environment: an old
    // board leaf can ignore a newly named profile. Never label row RTL column.
    let column = project.files.values().any(|source| {
        source.contains("function[8:0]column_ui_tile;")
            && source.contains("clear_sector==1599")
            && source.contains("wire[11:0]macro_column=")
    });
    if option_env!("CPU_V3_GPU_PROFILE") == Some("texture16x8-column16") && !column {
        return Err(
            "Selected board leaf did not emit the column framebuffer/texture contract".into(),
        );
    }
    std::fs::write(
        out.join("layout.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema": 1,
            "layout_abi": if column { 2 } else { 1 },
            "gpu_profile": if column { "texture16x8-column16" } else { "production" }
        }))?,
    )?;
    let mut peer_names = BTreeSet::new();
    if std::env::args().nth(2).as_deref() == Some("--mc") {
        // Production streaming transport and controller, with an accelerated
        // power-up delay only. The measured submission starts after init.
        for (path, source) in combination::rtl_sources(16)? {
            if path.extension().is_some_and(|ext| ext == "json") {
                std::fs::create_dir_all(out.join("peer"))?;
                std::fs::write(out.join("peer").join(&path), source)?;
                continue;
            }
            peer_names.insert(path.clone());
            if let Some(previous) = project.files.insert(path, source.clone()) {
                if previous != source {
                    return Err("Conflicting dependency in GPU+MC export".into());
                }
            }
        }
    }
    let mut sources = Vec::new();
    for (index, (path, source)) in project.files.into_iter().enumerate() {
        let name = if peer_names.contains(&path) {
            format!("peer/{}", path.display())
        } else {
            format!("source_{index}.v")
        };
        std::fs::write(out.join(&name), source)?;
        sources.push(name);
    }
    std::fs::write(out.join("rtl.json"), serde_json::to_vec_pretty(&sources)?)?;
    Ok(())
}
