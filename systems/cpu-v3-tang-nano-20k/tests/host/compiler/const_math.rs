//! Pure SDK descriptions: real rustc const checking, target storage and dynamic math.
use crate::shared_build_support_rcc_modules as resolver;
use cpu_v3::{rcc_backend, CpuV3Sim, RunOutcome};
use rcc::frontend::rustc_check::RustcCheck;
use std::path::Path;

fn compile(source: &str, label: &str) -> Result<rcc::frontend::Program, String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    resolver::GpuSources {
        local_dir: &root.join("rcc"),
        shared_dir: &root.join("rcc"),
        constants: "",
        ordinary_dirs: &[],
    }
    .compile_checked(
        label,
        source,
        &rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0),
        &RustcCheck::new(root.join("../../target/sdk-const-math/values").join(label)),
        &mut |_| {},
        &mut |_, _| {},
    )
}
const PREFIX: &str = "mod gpu_api;use gpu_api::*;mod duration;use duration::*;";

#[test]
fn const_projection_units_and_nonuniform_normal_matrices_execute_on_cpu() {
    let source = format!(
        "{PREFIX}{}",
        r#"
const POSE:Transform=Transform::new(Vec3::new(fix!(1),fix!(-2),fix!(-3)),
 Vec3::new(fix!(0),fix!(0),fix!(90).to_radians()),Vec3::new(fix!(2),fix!(3),fix!(-4)));
static WORLD:Mat3x4=POSE.world_matrix();
static NORMAL:Mat3x3=POSE.normal_matrix();
const VERTICAL:Projection=Projection::from_vertical_fov(fix!(60).to_radians(),fix!(199));
const HORIZONTAL:Projection=Projection::from_horizontal_fov(fix!(90).to_radians(),fix!(199));
static FOCAL:fix32=fix!(200);
static VERTICAL_FOCAL:fix32=VERTICAL.focal_pixels();
static HORIZONTAL_FOCAL:fix32=HORIZONTAL.focal_pixels();
const DURATION:u16=seconds_to_milliseconds(2);
fn close(a:fix32,b:fix32)->bool{(a-b).abs()<=fix32::from_words(8,0)}
fn dynamic_world(pose:Transform)->Mat3x4{pose.world_matrix()}
fn dynamic_normal(pose:Transform)->Mat3x3{pose.normal_matrix()}
fn degrees_runtime(angle:fix32)->fix32{angle.to_radians()}
fn main(){
 if WORLD.row0.x!=fix!(0)||WORLD.row0.y!=fix!(-3)||WORLD.row0.w!=fix!(1){halt(1);}
 if WORLD.row1.x!=fix!(2)||WORLD.row1.w!=fix!(-2)||WORLD.row2.z!=fix!(-4)||WORLD.row2.w!=fix!(-3){halt(2);}
 if NORMAL.row0.y!=fix32::from_ratio(-1,3)||NORMAL.row1.x!=fix!(0.5)||NORMAL.row2.z!=fix!(-0.25){halt(3);}
 if VERTICAL.focal_pixels()<fix!(207)||VERTICAL.focal_pixels()>fix!(208)||(HORIZONTAL.focal_pixels()-fix!(200)).abs()>fix32::from_words(128,0){halt(4);}
 let projection:Projection=Projection::from_focal_pixels(FOCAL,fix!(199));
 if projection.focal_pixels()!=fix!(200)||DURATION!=2000{halt(5);}
 let world:Mat3x4=dynamic_world(POSE);let normal:Mat3x3=dynamic_normal(POSE);
 if !close(world.row0.y,WORLD.row0.y)||!close(world.row1.x,WORLD.row1.x)||!close(normal.row0.y,NORMAL.row0.y){halt(6);}
 if !close(degrees_runtime(fix!(22.5)),fix!(22.5).to_radians()){halt(7);}
 halt(0x600d);
}
"#
    );
    let mut ir = compile(&source, "prepared-values").unwrap();
    let report_addresses: Vec<_> = ir
        .debug
        .globals
        .iter()
        .filter_map(|g| {
            if matches!(g.name.as_str(), "VERTICAL_FOCAL" | "HORIZONTAL_FOCAL") {
                if let rcc::VarLoc::Global(at) = g.loc {
                    return Some((g.name.clone(), at));
                }
            }
            None
        })
        .collect();
    let data = ir.take_static_initializers();
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let program = rcc_backend::try_compile(ir, &options, "main").unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    for (at, value) in data {
        cpu.physical_memory_mut()[usize::from(at)] = value;
    }
    let outcome = cpu.run(200000).unwrap();
    let values: serde_json::Map<_, _> = report_addresses
        .iter()
        .map(|(name, at)| {
            let memory = cpu.physical_memory_slice();
            let base = usize::from(*at);
            let raw = ((u32::from(memory[base + 1]) << 16) | u32::from(memory[base])) as i32;
            (
                name.clone(),
                serde_json::json!({"raw_q16":raw,"pixels":raw as f64/65536.0}),
            )
        })
        .collect();
    std::fs::write(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/sdk-const-math/values/numeric-results.json"),
        serde_json::to_vec_pretty(&values).unwrap(),
    )
    .unwrap();

    assert!(
        matches!(outcome, RunOutcome::Halted { signal: 0x600d, .. }),
        "{outcome:?}"
    );
}

#[test]
fn invalid_const_projection_scale_and_duration_are_rejected() {
    for (label, declaration) in [
        (
            "bad-fov",
            "const BAD:Projection=Projection::from_vertical_fov(fix!(0),fix!(199));",
        ),
        (
            "bad-focal",
            "const BAD:Projection=Projection::from_focal_pixels(fix!(100),fix!(199));",
        ),
        (
            "bad-scale",
            "const BAD:Mat3x3=Transform::uniform_scale(fix!(0)).normal_matrix();",
        ),
        ("bad-duration", "const BAD:u16=seconds_to_milliseconds(66);"),
    ] {
        let error = compile(
            &format!("{PREFIX}{declaration}fn main(){{halt(0);}}"),
            label,
        )
        .err()
        .expect("invalid const accepted");
        assert!(
            error.contains("rustc") && error.contains("E0080"),
            "{label}: {error}"
        );
    }
}
