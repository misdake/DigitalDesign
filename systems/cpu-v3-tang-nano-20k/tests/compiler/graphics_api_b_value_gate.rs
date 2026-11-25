use cpu_v3::{rcc_backend, CpuV3Sim, RunOutcome};
use rcc::frontend::rustc_check::RustcCheck;
use std::path::Path;
#[allow(dead_code)]
#[path = "../support/checked_flat_fixture.rs"]
mod checked_flat_fixture;

#[test]
fn frozen_shininess_enum_methods_compile() {
    let source = r#"
mod gpu_scene_value_methods;
static EXPONENTS:Buf<u16,17>=Buf::new([4,5,6,7,8,10,12,14,16,20,24,28,32,40,48,56,64]);
fn main(){
 let mut index:u16=0;
 while index<17{
  let selected=Shininess::nearest(EXPONENTS[index]);
  if selected as u16!=index||selected.exponent()!=EXPONENTS[index]{halt(1);}
  index+=1;
 }
 if Shininess::nearest(13).exponent()!=12||Shininess::nearest(36).exponent()!=32{halt(2);}
 if Shininess::nearest(0) as u16!=0||Shininess::nearest(65535) as u16!=16{halt(3);}
 halt(0x600d);
}
"#;
    let package = Path::new(env!("CARGO_MANIFEST_DIR"));
    let sources = package.join("rcc");
    let a_root = std::env::var_os("GRAPHICS_API_A_RCC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| sources.clone());
    let root = std::env::var_os("GRAPHICS_API_B_OUTPUT")
        .map(|root| std::path::PathBuf::from(root).join("values"))
        .unwrap_or_else(|| package.join("../../target/graphics-api-b-value-gate"));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("enum-methods.rs"), source).unwrap();
    // Freeze the actual dependency bytes once, even while A develops its root.
    let mut inputs: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut reports = Vec::new();
    for gpu_debug in [false, true] {
        let profile = if gpu_debug { "debug" } else { "release" };
        let output = root.join(profile);
        std::fs::create_dir_all(&output).unwrap();
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = gpu_debug;
        let result = checked_flat_fixture::compile(
            "frozen enum value methods",
            source,
            &options,
            &RustcCheck::new(output.join("checked")),
            &mut |name| {
                if let Some(text) = inputs.get(name) {
                    return Ok(text.clone());
                }
                let base = if name == "gpu_schema" {
                    &a_root
                } else {
                    &sources
                };
                let text = std::fs::read_to_string(base.join(format!("{name}.rs")))
                    .map_err(|e| e.to_string())?;
                std::fs::write(root.join(format!("input-{name}.rs")), &text)
                    .map_err(|e| e.to_string())?;
                inputs.insert(name.to_owned(), text.clone());
                Ok(text)
            },
        );
        let diagnostic = result.as_ref().err().map(ToString::to_string);
        let mut report = serde_json::json!({"accepted":result.is_ok(),"diagnostic":diagnostic,"gpu_debug":gpu_debug,"source_root":sources,"schema_root":a_root,"maximum_steps":250000});
        std::fs::write(
            output.join("result.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        let ir = result.unwrap_or_else(|error| panic!("frozen public API gate: {error}"));
        let static_bytes = ir.debug.static_data_end * 2;
        let program = rcc_backend::try_compile(ir, &options, "main").unwrap();
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        let outcome = cpu.run(250_000).unwrap();
        assert!(matches!(outcome, RunOutcome::Halted { signal: 0x600d, .. }));
        let bytes = program
            .words
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect::<Vec<_>>();
        std::fs::write(output.join("program.bin"), bytes).unwrap();
        report["outcome"] = format!("{outcome:?}").into();
        report["code_bytes"] = (program.words.len() * 2).into();
        report["static_bytes"] = static_bytes.into();
        std::fs::write(
            output.join("result.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        reports.push(report);
    }
    std::fs::write(
        root.join("result.json"),
        serde_json::to_vec_pretty(&reports).unwrap(),
    )
    .unwrap();
}
