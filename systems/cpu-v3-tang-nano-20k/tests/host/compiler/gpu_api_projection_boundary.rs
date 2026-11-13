//! Language boundary only; B owns production Projection math and representation.
use cpu_v3::{rcc_backend, CpuV3Sim, RunOutcome};
use rcc::frontend::rustc_check::RustcCheck;
use std::path::Path;

#[test]
fn projection_root_can_forward_a_small_prepared_value_without_new_option_types() {
    let source = r#"mod projection_values;
impl Projection{fn from_parts(focal:fix32,far:fix32)->Self{
 let prepared:Buf<u16,6>=Buf::new([0;6]);
 if !projection_prepare(prepared.as_array(),focal,far){halt(0xbad);}
 projection_value(prepared.as_array())}}
fn main(){let projection:Projection=Projection::from_parts(fix32::from_int(200),fix32::from_int(199));
 if projection.focal_pixels()!=fix32::from_int(200){halt(0xbad);}halt(0x600d);}"#;
    let values = r#"pub struct Projection{focal:fix32,far:fix32,ray_x:u16,ray_y:u16}
impl Projection{pub fn focal_pixels(&self)->fix32{self.focal}}
fn projection_prepare(mut out:Array<u16>,focal:fix32,far:fix32)->bool{
 if focal<=fix32::zero(){return false;}
 out[0u16]=focal.lo_bits();out[1u16]=focal.hi_bits();out[2u16]=far.lo_bits();out[3u16]=far.hi_bits();
 out[4u16]=0;out[5u16]=0;true}
fn projection_value(out:Array<u16>)->Projection{Projection{focal:fix32::from_words(out[0u16],out[1u16]),
 far:fix32::from_words(out[2u16],out[3u16]),ray_x:out[4u16],ray_y:out[5u16]}}"#;
    let root = std::env::var_os("RCC_RUSTC_TEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/graphics-api-g0")
        });
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let parsed = crate::checked_flat_fixture::compile(
        "projection-root-boundary",
        source,
        &options,
        &RustcCheck::new(root.join("projection-boundary")),
        &mut |name| {
            if name == "projection_values" {
                Ok(values.to_string())
            } else {
                Err(format!("unexpected {name}"))
            }
        },
    )
    .unwrap();
    let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    assert!(matches!(
        cpu.run(100_000).unwrap(),
        RunOutcome::Halted { signal: 0x600d, .. }
    ));
}
