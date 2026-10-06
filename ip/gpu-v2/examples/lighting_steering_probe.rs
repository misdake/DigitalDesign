//! Fixed-edge steering rounds, preserving the reviewed resource numerical kernel.
use gpu_v2::lighting::{
    rtl::{self, DspSteering, LightingRtlOptions},
    LightingProfile,
};
use std::{fmt::Write, fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/lighting-steering-20261005/round2".into()),
    );
    fs::create_dir_all(&root)?;
    let mut csv = String::from("profile,candidate,full_latency,diffuse_latency,full_ii,diffuse_ii,register_bits,mul9,mul18,mac,rom\n");
    {
        let profile = LightingProfile::Fast;
        for (name, steering, one_hot, extra_large) in [
            ("baseline", DspSteering::None, false, 0),
            ("onehot", DspSteering::None, true, 0),
            ("orient", DspSteering::Orient, false, 0),
            ("local", DspSteering::Local, false, 0),
            ("local-onehot", DspSteering::Local, true, 0),
            ("local-large1", DspSteering::Local, false, 1),
            ("joint", DspSteering::Joint, false, 0),
            ("joint-large1", DspSteering::Joint, false, 1),
        ] {
            let mut options = LightingRtlOptions::retimed_resource_profile(profile);
            options.dsp_steering = steering;
            options.one_hot_dsp = one_hot;
            options.retiming.extra_large_multiply += extra_large;
            let rtl = rtl::generate_with_options(profile, options)?;
            if name == "local-large1" {
                assert_eq!(
                    rtl.source,
                    rtl::generate_with_options(
                        profile,
                        LightingRtlOptions::steered_resource_profile(profile)
                    )?
                    .source
                );
            }
            let d = root.join(format!("{profile:?}-{name}"));
            fs::create_dir_all(&d)?;
            fs::write(d.join("lighting.v"), &rtl.source)?;
            fs::write(d.join("storage.csv"), &rtl.storage_csv)?;
            fs::write(d.join("dsp-inputs.csv"), &rtl.dsp_input_csv)?;
            fs::write(
                d.join("calendar.csv"),
                rtl::operation_calendar_with_options(profile, options)?,
            )?;
            writeln!(
                csv,
                "{profile:?},{name},{},{},{},{},{},{},{},{},{}",
                rtl.latency,
                rtl.diffuse_latency,
                rtl.specular_ii,
                rtl.diffuse_ii,
                rtl.register_bits,
                rtl.small_multipliers,
                rtl.large_multipliers,
                rtl.pair_macros,
                rtl.normalization_roms
            )?;
            println!(
                "{profile:?}/{name}: latency {}/{}; bits {}; DSP9/18 {}/{}",
                rtl.latency,
                rtl.diffuse_latency,
                rtl.register_bits,
                rtl.small_multipliers,
                rtl.large_multipliers
            );
        }
    }
    fs::write(root.join("summary.csv"), csv)?;
    Ok(())
}
