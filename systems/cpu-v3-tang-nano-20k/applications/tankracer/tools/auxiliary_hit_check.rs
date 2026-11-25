//! Persistent auxiliary contact, ownership and terrain through production RCC.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("auxiliary-hit.json"))?)?;
    let code = compile(
        "auxiliary-hit",
        &format!(
            r#"
fn main(){{
 let input=unsafe{{Ptr::from_addr(0x0100).as_u16_array()}};
 let out=unsafe{{Ptr::from_addr(0x1800).as_u16_array()}};
 let storage=unsafe{{Ptr::from_addr(0x5000).as_u16_array()}};
 let scratch=unsafe{{Ptr::from_addr(0x8000).as_u16_array()}};
 let mut directory=unsafe{{Ptr::from_addr(0x8700).as_u16_array()}};let mut i=0u16;
 while i<64{{directory[i]=i;i+=1;}}
 let cells=static_assets::AssetBytes{{first_sector:16384,sector_count:16384,byte_len_lo:0,byte_len_hi:32}};
 let mut map=auxiliary_map(cells,directory);let mut case=load_auxiliary_hit(input,storage,scratch);let mut frame=0u16;
 while frame<case.count{{run_auxiliary_hit(&mut case,frame,out,&mut map);let mut progress=input;progress[0u16]=frame+1;frame+=1;}}
 scene_meshes::finish(&mut case.resources);map.terrain.finish();halt(0x600d);
}}
{}
{}
"#,
            include_str!("mesh_store_case.rs"),
            include_str!("auxiliary_hit_case.rs")
        ),
        output,
    )?;
    let cells = fs::read(fixture.join("auxiliary-cells.bin"))?;
    let cases = data["cases"].as_array().ok_or("auxiliary hit cases")?;
    let mut checked = 0usize;
    let mut frames = 0usize;
    let mut instructions = 0u64;
    for (index, case) in cases.iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        let memory = cpu.physical_memory_mut();
        for (i, pair) in cells.chunks_exact(2).enumerate() {
            memory[0x100000 + i] = u16::from_le_bytes([pair[0], pair[1]]);
        }
        let input = case["input"].as_array().unwrap();
        assert!(input.len() < 0x1700);
        for (i, v) in input.iter().enumerate() {
            memory[0x100 + i] = v.as_u64().unwrap() as u16;
        }
        let expected = case["frames"].as_array().unwrap();
        let mut done = 0usize;
        let mut passed = false;
        for step in 1..=20000000 {
            let outcome = cpu.step().expect("auxiliary hit instruction");
            let memory = cpu.physical_memory_mut();
            if usize::from(memory[0x100]) > done {
                assert_eq!(usize::from(memory[0x100]), done + 1);
                let frame = expected[done].as_array().unwrap();
                assert!(frame.len() < 0x3800);
                for (i, v) in frame.iter().enumerate() {
                    assert_eq!(u64::from(memory[0x1800+i]),v.as_u64().unwrap(),"auxiliary case {index}, object {}, variant {}, limit {}, frame {done}, word {i}",case["object"],case["variant"],case["limit"]);
                    checked += 1;
                }
                done += 1;
                frames += 1;
            }
            if let StepOutcome::Halted { signal } = outcome {
                assert_eq!(signal, PASS, "auxiliary case {index}");
                assert_eq!(done, expected.len());
                instructions += step;
                passed = true;
                break;
            }
        }
        assert!(passed, "auxiliary case {index} exceeded bound");
    }
    Ok(
        json!({"cases":cases.len(),"frames":frames,"checked_u16_values":checked,"code_bytes":code.total_code_words()*2,"instruction_steps":instructions,
        "maximum_integer_error":0,"models":data["names"],"scope":data["scope"],
        "boundary":"Actual auxiliary-contact method with authored objects/local resource arena and real terrain pages; outer car collision dispatch and complete-world resource construction remain separate"}),
    )
}
