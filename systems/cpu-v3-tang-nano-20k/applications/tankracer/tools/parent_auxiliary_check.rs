//! Full parent class chains through RCC and immutable map/math windows.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("parent-auxiliary.json"))?)?;
    let source = format!(
        r#"
fn main(){{
 let input=unsafe{{Ptr::from_addr(0x0100).as_u16_array()}};
 let out=unsafe{{Ptr::from_addr(0x1800).as_u16_array()}};
 let lifetime=unsafe{{Ptr::from_addr(0x3000).as_u16_array()}};
 let resources=unsafe{{Ptr::from_addr(0x6000).as_u16_array()}};
 let scratch=unsafe{{Ptr::from_addr(0x6100).as_u16_array()}};
 let mut directory=unsafe{{Ptr::from_addr(0x6800).as_u16_array()}};
 let mut i=0u16;while i<64{{directory[i]=i;i+=1;}}
 let cells=static_assets::AssetBytes{{first_sector:16384,sector_count:16384,byte_len_lo:0,byte_len_hi:32}};
 let scene_asset=static_assets::AssetBytes{{first_sector:40960,sector_count:302,byte_len_lo:38624,byte_len_hi:0}};
 let scene=scene_query::Scene::begin(scene_asset);if !scene.valid{{halt(0x7f21);}}
 let sine:Buf<u16,4>=Buf::new([45056,320,40960,0]);
 let angle:Buf<u16,4>=Buf::new([45376,5,514,0]);
 let inverse:Buf<u16,4>=Buf::new([45381,64,8192,0]);
 let tables=ParentTables{{sine:unsafe{{sine.as_array().as_ptr().as_i16_array()}},angle:unsafe{{angle.as_array().as_ptr().as_i16_array()}},inverse:unsafe{{inverse.as_array().as_ptr().as_i16_array()}}}};
 let map=auxiliary_map(cells,directory);
 let mut case=load_parent_case(input,lifetime,resources,scratch,map);
 let mut bank=scene_fragments::MotionBank{{page:184,used:0}};
 place_fragment_motion(&mut case.actor.fragments,&mut bank);
 let mut frame=0u16;
 while frame<case.count{{
  run_parent_frame(&scene,&mut case,frame,out,&tables);
  unsafe{{Ptr::from_addr(0x00ff).write(0,frame+1);}}frame+=1;
 }}
 scene_meshes::finish(&mut case.state.resources);case.state.map.terrain.finish();halt(0x600d);
}}
{}
{}
{}
{}
{}
{}
"#,
        include_str!("scene_lifecycle_case.rs"),
        include_str!("mesh_store_case.rs"),
        include_str!("auxiliary_hit_case.rs"),
        include_str!("fragment_motion_case.rs"),
        include_str!("parent_auxiliary_case.rs"),
        ""
    );
    let code = compile("parent-auxiliary", &source, output)?;
    let blobs = [
        ("parent-cells.bin", 0x100000usize),
        ("parent-scene.bin", 0x280000),
        ("sine.i16.bin", 0x2c0000),
        ("angle.i16.bin", 45376 * 64),
        ("cosine-angle.bin", 45381 * 64),
    ];
    let bytes = blobs
        .iter()
        .map(|(name, base)| Ok((*base, fs::read(fixture.join(name))?)))
        .collect::<Result<Vec<_>, std::io::Error>>()?;
    let cases = data["cases"].as_array().ok_or("parent cases")?;
    let mut checked = 0;
    let mut frames = 0;
    let mut instructions = 0u64;
    for (index, case) in cases.iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        for (base, bytes) in &bytes {
            for (i, p) in bytes.chunks_exact(2).enumerate() {
                cpu.physical_memory_mut()[base + i] = u16::from_le_bytes([p[0], p[1]]);
            }
        }
        let input = case["input"].as_array().unwrap();
        assert!(input.len() < 0x1700);
        for (i, v) in input.iter().enumerate() {
            cpu.physical_memory_mut()[0x100 + i] = v.as_u64().unwrap() as u16;
        }
        let expected = case["frames"].as_array().unwrap();
        let mut done = 0;
        let mut passed = false;
        for step in 1..=30000000 {
            let outcome = cpu.step().expect("parent class instruction");
            let memory = cpu.physical_memory_mut();
            if usize::from(memory[0xff]) > done {
                assert_eq!(usize::from(memory[0xff]), done + 1);
                let values = expected[done].as_array().unwrap();
                assert!(values.len() < 0x1800);
                for (i, v) in values.iter().enumerate() {
                    if u64::from(memory[0x1800 + i]) != v.as_u64().unwrap() {
                        fs::write(
                            output.join("failure.json"),
                            serde_json::to_vec(&json!({
                                "case":index,"frame":done,"field":i,"expected":values,
                                "actual":&memory[0x1800..0x1800+values.len()]
                            }))?,
                        )?;
                    }
                    assert_eq!(
                        u64::from(memory[0x1800 + i]),
                        v.as_u64().unwrap(),
                        "parent case {index}, object {}, variant {}, frame {done}, word {i}",
                        case["object"],
                        case["variant"]
                    );
                    checked += 1;
                }
                done += 1;
                frames += 1;
            }
            if let StepOutcome::Halted { signal } = outcome {
                assert_eq!(signal, PASS, "parent case {index}");
                assert_eq!(done, expected.len());
                instructions += step;
                passed = true;
                break;
            }
        }
        assert!(passed, "parent case {index} exceeded bound");
    }
    Ok(
        json!({"cases":cases.len(),"frames":frames,"checked_u16_values":checked,"instruction_steps":instructions,
        "maximum_integer_error":0,"code_bytes":code.total_code_words()*2,"scope":data["scope"]}),
    )
}
