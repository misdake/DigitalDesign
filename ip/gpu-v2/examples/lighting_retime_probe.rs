//! Matched-kernel, bounded retiming rounds; generated register bits are not fitted FF.
use gpu_v2::lighting::{
    rtl::{self, LightingRtlOptions},
    LightingProfile, LightingRetiming,
};
use std::{fmt::Write, fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/lighting-retime-20261005".into()),
    );
    fs::create_dir_all(&root)?;
    let mut csv=String::from("profile,round,full_ii,diffuse_ii,full_latency,diffuse_latency,register_bits,mul9,mul18,mac,normal_roms,dsp_macros,dsp_tiles_used,retained_delay_bits,zero_delay_retained_rows\n");
    for profile in [LightingProfile::Fast, LightingProfile::Compact] {
        for (round, functions, large, small, reads, compact) in [
            ("baseline", false, 0, 0, 0, false),
            ("functions", true, 0, 0, 0, false),
            ("lifetimes", true, 0, 0, 0, true),
            ("large1", true, 1, 0, 0, true),
            ("small2", true, 0, 2, 0, true),
            ("read1", true, 0, 0, 1, true),
            ("large1-read1", true, 1, 0, 1, true),
            ("large2-read1", true, 2, 0, 1, true),
        ] {
            let base = if std::env::var_os("LIGHTING_RETIME_RESOURCE").is_some() {
                LightingRtlOptions::resource_profile(profile)
            } else {
                Default::default()
            };
            let options = LightingRtlOptions {
                retiming: LightingRetiming {
                    measured_functions: functions,
                    extra_large_multiply: large,
                    extra_small_multiply: small,
                    extra_normalize_reads: reads,
                    compact_lifetimes: compact,
                },
                ..base
            };
            match rtl::generate_with_options(profile, options) {
                Ok(r) => {
                    let dir = root.join(format!("{profile:?}-{round}"));
                    fs::create_dir_all(&dir)?;
                    fs::write(dir.join("lighting.v"), &r.source)?;
                    fs::write(dir.join("storage.csv"), &r.storage_csv)?;
                    fs::write(
                        dir.join("lanes.txt"),
                        r.lanes
                            .iter()
                            .map(|l| {
                                format!(
                                    "{} {} width={} latency={} full={} diffuse={}\n",
                                    l.lane,
                                    l.kind,
                                    l.width,
                                    l.latency,
                                    l.full_operations,
                                    l.diffuse_operations
                                )
                            })
                            .collect::<String>(),
                    )?;
                    // Value-delay rows in the emitter's authoritative storage table.
                    let mut retained = 0_usize;
                    let mut direct = 0_usize;
                    for line in r.storage_csv.lines().skip(1) {
                        let cols: Vec<_> = line.split(',').collect();
                        if cols.first() == Some(&"retained") {
                            if let (Some(bits), Some(delay)) = (
                                cols.get(2).and_then(|s| s.parse::<usize>().ok()),
                                cols.get(6).and_then(|s| s.parse::<usize>().ok()),
                            ) {
                                retained += bits * delay;
                                if delay == 0 {
                                    direct += 1;
                                }
                            }
                        }
                    }
                    let macros = r.small_multipliers.div_ceil(4)
                        + r.large_multipliers.div_ceil(2)
                        + r.pair_macros;
                    writeln!(
                        csv,
                        "{profile:?},{round},{},{},{},{},{},{},{},{},{},{macros},{},{retained},{direct}",
                        r.specular_ii,
                        r.diffuse_ii,
                        r.latency,
                        r.diffuse_latency,
                        r.register_bits,
                        r.small_multipliers,
                        r.large_multipliers,
                        r.pair_macros,
                        r.normalization_roms,
                        macros.div_ceil(2)
                    )?;
                    println!("{profile:?}/{round}: latency {}/{} registers {} DSP9/18/MAC {}/{}/{} ROM {}",r.latency,r.diffuse_latency,r.register_bits,r.small_multipliers,r.large_multipliers,r.pair_macros,r.normalization_roms);
                }
                Err(e) => {
                    fs::write(root.join(format!("{profile:?}-{round}-rejected.txt")), &e)?;
                    println!("{profile:?}/{round}: rejected {e}");
                }
            }
        }
    }
    fs::write(root.join("summary.csv"), csv)?;
    Ok(())
}
