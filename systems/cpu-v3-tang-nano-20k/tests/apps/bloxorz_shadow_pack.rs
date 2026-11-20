//! Decode the real fan packer's complete native words, including untouched guards.
use cpu_v3::{rcc_backend, CpuV3Sim, PhysicalWordAddress, RunOutcome};
use rcc::frontend::{compile_program_named_checked, rustc_check::RustcCheck};

#[test]
fn shadow_fan_indices_follow_native_six_bit_wire_layout() {
    let module = include_str!("../../examples/game_bloxorz/rcc/shadow.rs");
    let start = module.find("fn shadow_batch_index(").unwrap();
    let end = start + module[start..].find("fn shadow_batch_cell(").unwrap();
    let mut source = String::from(
        r#"static WORDS:Buf<u16,40>=Buf::new([0;40]);
fn main(){let output=Ptr::from_addr(0x4000);let mut case:u16=0;let mut triangle:u16=0;
 while triangle<22 {let mut fan:u16=2;while fan<24 {
  let mut i:u16=0;while i<40 {WORDS[i]=0;i+=1;}
  shadow_batch_index(WORDS.as_array(),4,triangle,fan);
  i=0;while i<40 {unsafe{output.write((case*40+i) as i16,WORDS[i]);}i+=1;}
  case+=1;fan+=1;
 }triangle+=1;}halt(0x600d);}
"#,
    );
    source.push_str(&module[start..end]);
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/bloxorz-cpu-series/04-pack-core");
    let parsed = compile_program_named_checked(
        "shadow index wire layout",
        &source,
        &options,
        &RustcCheck::new(out),
        &mut |name| Err(format!("unexpected module {name}")),
    )
    .unwrap();
    let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    assert!(matches!(
        cpu.run(2_000_000).unwrap(),
        RunOutcome::Halted { signal: 0x600d, .. }
    ));
    for triangle in 0..22usize {
        for fan in 2..24usize {
            let case = triangle * 22 + fan - 2;
            let base = 4 + triangle / 3 * 4;
            let shift = (triangle % 3) * 18;
            let expected = ((fan - 1) as u64) << (shift + 6) | (fan as u64) << (shift + 12);
            for word in 0..40usize {
                let want = if (base..base + 4).contains(&word) {
                    (expected >> ((word - base) * 16)) as u16
                } else {
                    0
                };
                assert_eq!(
                    cpu.physical_memory(PhysicalWordAddress::new(
                        (0x4000 + case * 40 + word) as u32
                    )),
                    want,
                    "triangle{triangle} fan{fan} word{word}"
                );
            }
        }
    }
}
