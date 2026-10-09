//! Reproduce the formal compensated-floor DAG, calendars and matched RTL.
use gpu_v2::lighting::{
    rtl::{self, LightingRtlOptions},
    LightingProfile,
};
use std::{fmt::Write, fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/lighting-compensation-formal-20261005".into()),
    );
    fs::create_dir_all(&root)?;
    let mut summary = String::from("profile,full_latency,diffuse_latency,full_ii,diffuse_ii,register_bits,dsp9,dsp18,pairs,normalize_roms\n");
    {
        let profile = LightingProfile::Fast;
        let options = LightingRtlOptions::compensated_resource_profile(profile);
        let r = rtl::generate_with_options(profile, options)?;
        let dir = root.join(format!("{profile:?}"));
        fs::create_dir_all(&dir)?;
        fs::write(dir.join("lighting.v"), r.source)?;
        fs::write(dir.join("storage.csv"), r.storage_csv)?;
        fs::write(dir.join("dsp-inputs.csv"), r.dsp_input_csv)?;
        fs::write(
            dir.join("calendar.csv"),
            rtl::operation_calendar_with_options(profile, options)?,
        )?;
        writeln!(
            summary,
            "{profile:?},{},{},{},{},{},{},{},{},{}",
            r.latency,
            r.diffuse_latency,
            r.specular_ii,
            r.diffuse_ii,
            r.register_bits,
            r.small_multipliers,
            r.large_multipliers,
            r.pair_macros,
            r.normalization_roms
        )?;
        println!(
            "{profile:?}: full/diffuse latency {}/{} II {}/{}",
            r.latency, r.diffuse_latency, r.specular_ii, r.diffuse_ii
        );
    }
    fs::write(root.join("summary.csv"), summary)?;
    Ok(())
}
