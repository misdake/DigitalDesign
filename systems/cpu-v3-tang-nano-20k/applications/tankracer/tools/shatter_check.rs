//! Stateful original debris rows checked after every bounded instruction step.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("shatter.json"))?)?;
    let code = compile(
        "shatter",
        &format!(
            r#"
fn main(){{
 let input=unsafe{{Ptr::from_addr(0x0100).as_u16_array()}};
 let out=unsafe{{Ptr::from_addr(0x1900).as_u16_array()}};
 let sine=unsafe{{Ptr::from_addr(0x3000).as_i16_array()}};
 let angle=unsafe{{Ptr::from_addr(0x8000).as_i16_array()}};
 let storage=unsafe{{Ptr::from_addr(0x8200).as_u16_array()}};
 let info=unsafe{{Ptr::from_addr(0xb800).as_u16_array()}};
 run_shatter(input,out,storage,info,sine,angle);halt(0x600d);
}}
{}"#,
            include_str!("shatter_case.rs")
        ),
        output,
    )?;
    let sine = fs::read(fixture.join("sine.i16.bin"))?;
    let angle = fs::read(fixture.join("angle.i16.bin"))?;
    let mut checked = 0usize;
    let mut frames = 0usize;
    let mut steps = 0u64;
    let mut minimum_stack = 65535u16;
    let cases = data["cases"].as_array().ok_or("shatter cases")?;
    for (index, case) in cases.iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        let memory = cpu.physical_memory_mut();
        for (base, table) in [(0x3000, &sine), (0x8000, &angle)] {
            for (i, pair) in table.chunks_exact(2).enumerate() {
                memory[base + i] = u16::from_le_bytes([pair[0], pair[1]]);
            }
        }
        let input = case["input"].as_array().ok_or("shatter input")?;
        assert!(input.len() * 2 < 0x1800);
        for (i, v) in input.iter().enumerate() {
            memory[0x100 + i * 2..0x102 + i * 2]
                .copy_from_slice(&words(v.as_i64().unwrap() as i32));
        }
        let expected = case["frames"].as_array().ok_or("shatter frames")?;
        let mut done = 0usize;
        let mut passed = false;
        let mut started = false;
        for step in 1..=20000000 {
            let outcome = cpu.step().expect("shatter instruction");
            let sp = cpu.registers()[13];
            if started || sp != 0 {
                started = true;
                minimum_stack = minimum_stack.min(sp);
            }
            let memory = cpu.physical_memory_mut();
            if usize::from(memory[0xfffe]) > done {
                assert_eq!(usize::from(memory[0xfffe]), done + 1);
                let values = expected[done].as_array().unwrap();
                assert!(values.len() * 2 < 0x1600);
                for (i, v) in values.iter().enumerate() {
                    let actual = i32::from(memory[0x1900 + i * 2])
                        | (i32::from(memory[0x1901 + i * 2]) << 16);
                    assert_eq!(
                        i64::from(actual),
                        v.as_i64().unwrap(),
                        "shatter case {index}, tag {}, frame {done}, field {i}",
                        case["tag"]
                    );
                    checked += 1;
                }
                done += 1;
                frames += 1;
            }
            if let StepOutcome::Halted { signal } = outcome {
                assert_eq!(signal, PASS, "shatter case {index}");
                assert_eq!(done, expected.len());
                steps += step;
                passed = true;
                break;
            }
        }
        assert!(passed, "shatter case {index} exceeded bound");
    }
    Ok(
        json!({"cases":cases.len(),"frames":frames,"checked_i32_values":checked,
        "instruction_steps":steps,"maximum_integer_error":0,"code_bytes":code.total_code_words()*2,
        "minimum_stack_word":minimum_stack,"authored":data["authored"],"scope":data["scope"],
        "boundary":"Debris kernel and callback resume semantics in instruction sim; scene blast dispatch, loading and completion remain separate"}),
    )
}
