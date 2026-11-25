//! Persistent original resource ownership and complete geometry comparisons.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("mesh-store.json"))?)?;
    let code = compile(
        "mesh-store",
        &format!(
            r#"
fn main(){{
 let input=unsafe{{Ptr::from_addr(0x0100).as_u16_array()}};
 let out=unsafe{{Ptr::from_addr(0x1800).as_u16_array()}};
 let storage=unsafe{{Ptr::from_addr(0x5000).as_u16_array()}};
 let mut case=load_mesh_store_case(input,storage);let mut frame=0u16;
 while frame<case.count{{run_mesh_store_step(&mut case,frame,out);let mut progress=input;progress[0u16]=frame+1;frame+=1;}}
 scene_meshes::finish(&mut case.store);halt(0x600d);
}}
{}
"#,
            include_str!("mesh_store_case.rs")
        ),
        output,
    )?;
    let cases = data["cases"].as_array().ok_or("mesh store cases")?;
    let mut checked = 0usize;
    let mut frames = 0usize;
    let mut instructions = 0u64;
    for (index, case) in cases.iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        let memory = cpu.physical_memory_mut();
        let input = case["input"].as_array().unwrap();
        assert!(input.len() < 0x1700);
        for (i, v) in input.iter().enumerate() {
            memory[0x100 + i] = v.as_u64().unwrap() as u16;
        }
        let expected = case["frames"].as_array().unwrap();
        let mut done = 0usize;
        let mut passed = false;
        for step in 1..=10000000 {
            let outcome = cpu.step().expect("mesh store instruction");
            let memory = cpu.physical_memory_mut();
            if usize::from(memory[0x100]) > done {
                assert_eq!(usize::from(memory[0x100]), done + 1);
                let frame = expected[done].as_array().unwrap();
                assert!(frame.len() < 0x3800);
                for (i, v) in frame.iter().enumerate() {
                    assert_eq!(
                        u64::from(memory[0x1800 + i]),
                        v.as_u64().unwrap(),
                        "mesh store case {index}, frame {done}, word {i}"
                    );
                    checked += 1;
                }
                done += 1;
                frames += 1;
            }
            if let StepOutcome::Halted { signal } = outcome {
                assert_eq!(signal, PASS, "mesh store case {index}");
                assert_eq!(done, expected.len());
                instructions += step;
                passed = true;
                break;
            }
        }
        assert!(passed, "mesh store case {index} exceeded bound");
    }
    let failure = compile(
        "mesh-store-oom",
        &format!(
            r#"
fn main(){{
 let input=unsafe{{Ptr::from_addr(0x0100).as_u16_array()}};
 let output=unsafe{{Ptr::from_addr(0x1800).as_u16_array()}};
 let mut storage=unsafe{{Ptr::from_addr(0x5000).as_u16_array()}};
 let mut case=load_mesh_store_case(input,storage);let mut i=0u16;
 while i<76{{storage[i]=0x35ca+i;i+=1;}}
 if run_mesh_store_step(&mut case,0,output)!=7{{halt(0x7f74);}}
 if attitude::get(output,0)!=integer::from_i16(-4)||output[2u16]!=0||output[3u16]!=0||output[4u16]!=0||output[5u16]!=65535||output[6u16]!=65535{{halt(0x7f75);}}
 i=0;while i<76{{if storage[i]!=0x35ca+i{{halt(0x7f76);}}i+=1;}}
 halt(0x600d);
}}
{}
"#,
            include_str!("mesh_store_case.rs")
        ),
        output,
    )?;
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&failure.words);
    for (i, v) in cases[0]["input"].as_array().unwrap().iter().enumerate() {
        cpu.physical_memory_mut()[0x100 + i] = v.as_u64().unwrap() as u16;
    }
    let mut complete = false;
    for _ in 0..1000000 {
        if let StepOutcome::Halted { signal } = cpu.step().expect("resource OOM instruction") {
            assert_eq!(signal, PASS);
            complete = true;
            break;
        }
    }
    assert!(complete, "resource OOM exceeded instruction bound");
    Ok(
        json!({"cases":cases.len(),"frames":frames,"checked_u16_values":checked,"code_bytes":code.total_code_words()*2,"instruction_steps":instructions,
        "maximum_integer_error":0,"allocation_failure_checked":true,"authored_models":data["authored_models"],"scope":data["scope"],
        "boundary":"Resource owner with caller-owned addressable arenas and offline normalized name/material keys; world activation and normal loading remain separate"}),
    )
}
