//! Matched real Quad/Lighting/preparation replay for static storage profiles.
#[path = "pixel_runtime_ingress.rs"]
mod reference;
use gpu_v2::{
    system::pixel::{pixel_runtime_ingress, quad_ingress},
    texture::rtl::runtime_rows::Profile,
};

#[test]
#[ignore = "requires actual RTL and Gowin primitive library"]
fn matched_runtime_profiles_qualify() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/pixel-runtime-profiles-20261010");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("tb.v"), reference::tb()).unwrap();
    for (name, profile) in [
        (
            "early",
            Profile {
                continuous_prepare: false,
                early_ordered: true,
                canonical_owner: false,
            },
        ),
        (
            "canonical",
            Profile {
                continuous_prepare: false,
                early_ordered: false,
                canonical_owner: true,
            },
        ),
        (
            "both",
            Profile {
                continuous_prepare: false,
                early_ordered: true,
                canonical_owner: true,
            },
        ),
        (
            "continuous",
            Profile {
                early_ordered: true,
                canonical_owner: true,
                continuous_prepare: true,
            },
        ),
    ] {
        for native in [false, true] {
            let backend = if native {
                quad_ingress::rtl::Backend::Native
            } else {
                quad_ingress::rtl::Backend::Portable
            };
            let source = pixel_runtime_ingress::verilog_profile(backend, profile).unwrap();
            let source = if native {
                "`define GPU_V2_GOWIN_DSP\n".to_owned() + &source
            } else {
                source
            };
            let file = format!("{name}-native-{native}.v");
            std::fs::write(dir.join(&file), source).unwrap();
            let mut compile = std::process::Command::new(
                std::env::var_os("IVERILOG_EXE").unwrap_or("iverilog".into()),
            );
            compile
                .current_dir(&dir)
                .args(["-g2012", "-s", "tb", "-o", "sim.vvp"]);
            if native {
                compile.arg(
                    std::path::PathBuf::from(std::env::var_os("GOWIN_HOME").expect("GOWIN_HOME"))
                        .join("IDE/simlib/gw2a/prim_sim.v"),
                );
            }
            let compiled = compile.args([&file, "tb.v"]).output().unwrap();
            assert!(
                compiled.status.success(),
                "{name}/{native}: {}",
                String::from_utf8_lossy(&compiled.stderr)
            );
            let result =
                std::process::Command::new(std::env::var_os("VVP_EXE").unwrap_or("vvp".into()))
                    .current_dir(&dir)
                    .arg("sim.vvp")
                    .output()
                    .unwrap();
            assert!(
                result.status.success(),
                "{name}/{native}: {} {}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            println!(
                "{name}/{native}: {}",
                String::from_utf8_lossy(&result.stdout)
            );
        }
    }
}
