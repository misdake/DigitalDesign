//! RCC fragment kernels checked against all mutable original fields and RNG.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("mesh-break.json"))?)?;
    let code = compile(
        "mesh-break",
        &format!(
            "fn main(){{let input=unsafe{{Ptr::from_addr(0x8000).as_u16_array()}};let out=unsafe{{Ptr::from_addr(0xa000).as_u16_array()}};let scratch=unsafe{{Ptr::from_addr(0x1000).as_u16_array()}};let sine=unsafe{{Ptr::from_addr(0x3000).as_i16_array()}};run_mesh_break(input,out,scratch,sine);halt(0x600d);}}\n{}",
            include_str!("mesh_break_case.rs")
        ),
        output,
    )?;
    let sine = fs::read(fixture.join("sine.i16.bin"))?;
    let cases = data["cases"].as_array().ok_or("mesh-break cases")?;
    let mut instructions = 0u64;
    let mut checked = 0usize;
    for (index, case) in cases.iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        let memory = cpu.physical_memory_mut();
        for (i, pair) in sine.chunks_exact(2).enumerate() {
            memory[0x3000 + i] = u16::from_le_bytes([pair[0], pair[1]]);
        }
        let input = case["input"].as_array().ok_or("mesh-break input")?;
        assert!(input.len() < 4096);
        for (i, v) in input.iter().enumerate() {
            memory[0x8000 + i * 2..0x8002 + i * 2]
                .copy_from_slice(&words(v.as_i64().unwrap() as i32));
        }
        let expected = case["expected"].as_array().ok_or("mesh-break expected")?;
        assert!(expected.len() < 2048);
        for i in 0..expected.len() {
            memory[0xa000 + i * 2..0xa002 + i * 2].copy_from_slice(&words(0x12345678));
        }
        let mut passed = false;
        for step in 1..=2000000 {
            if let StepOutcome::Halted { signal } = cpu.step().expect("mesh-break instruction") {
                assert_eq!(signal, PASS, "mesh-break case {index}");
                instructions += step;
                passed = true;
                break;
            }
        }
        assert!(passed, "mesh-break case {index} exceeded bound");
        let memory = cpu.physical_memory_mut();
        for (i, v) in expected.iter().enumerate() {
            let actual =
                i32::from(memory[0xa000 + i * 2]) | (i32::from(memory[0xa001 + i * 2]) << 16);
            assert_eq!(
                i64::from(actual),
                v.as_i64().unwrap(),
                "mesh-break case {index}, tag {}, field {i}",
                case["tag"]
            );
            checked += 1;
        }
    }
    Ok(
        json!({"cases":cases.len(),"checked_i32_values":checked,"maximum_integer_error":0,"instruction_steps":instructions,
        "code_bytes":code.total_code_words()*2,"authored":data["authored"],"scope":data["scope"],
        "boundary":"RCC modules, RAM tables, CPU instruction backend; world allocation and virtual completion remain separate"}),
    )
}
