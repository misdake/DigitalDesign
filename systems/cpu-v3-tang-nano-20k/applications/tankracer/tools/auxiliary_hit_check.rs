//! Persistent auxiliary contact, ownership and terrain through production RCC.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let mut report = run_layout(output, fixture, false)?;
    report["mapped_indices"] = run_layout(output, fixture, true)?;
    Ok(report)
}
fn run_layout(output: &Path, fixture: &Path, paged: bool) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("auxiliary-hit.json"))?)?;
    let label = if paged {
        "auxiliary-hit-paged"
    } else {
        "auxiliary-hit"
    };
    let case_source = include_str!("auxiliary_hit_case.rs").replace(
        "    let initial = store_at(input, input[10u16]);",
        "    if PAGED && !scene_meshes::use_arena(&mut resources,200,2048,14336) {halt(0x7f91);}
    let initial = store_at(input, input[10u16]);",
    );
    let code = compile(
        label,
        &format!(
            r#"
const PAGED:bool={};
fn page_indices(source:Array<u16>,count:u16)->Array<u16>{{
 let address=source.as_ptr().addr();
 let saved=read_dseg_page(3);unsafe{{write_dseg_page(3,201);}}
 let mut target=unsafe{{Ptr::from_addr(0xc000+(address&16383)).as_u16_array()}};
 let mut i=0u16;while i<count{{target[i]=source[i];i+=1;}}
 unsafe{{write_dseg_page(3,saved);}}
 unsafe{{Ptr::from_addr(0x8000+(address&16383)).as_u16_array()}}
}}
fn main(){{
 let input=unsafe{{Ptr::from_addr(0x0100).as_u16_array()}};
 let out=unsafe{{Ptr::from_addr(0x1800).as_u16_array()}};
 let mut storage=unsafe{{Ptr::from_addr(0x5000).as_u16_array()}};
 if PAGED{{unsafe{{write_dseg_page(2,202);}}storage=unsafe{{Ptr::from_addr(0x8f00).as_u16_array()}};}}
 let mut scratch=unsafe{{Ptr::from_addr(0x8000).as_u16_array()}};
 if PAGED{{scratch=unsafe{{Ptr::from_addr(0xc000).as_u16_array()}};}}
 let mut directory=unsafe{{Ptr::from_addr(0x8700).as_u16_array()}};
 if PAGED{{directory=unsafe{{Ptr::from_addr(0xbe80).as_u16_array()}};}}let mut i=0u16;
 while i<64{{directory[i]=i;i+=1;}}
 let cells=static_assets::AssetBytes{{first_sector:16384,sector_count:16384,byte_len_lo:0,byte_len_hi:32}};
 let mut map=auxiliary_map(cells,directory);
 if PAGED{{unsafe{{write_dseg_page(3,200);}}}}
 let mut case=load_auxiliary_hit(input,storage,scratch);let mut frame=0u16;
 if PAGED{{
  let metadata=unsafe{{Ptr::from_addr(0x5000).as_u16_array()}};
  store_copy(metadata,storage,0,76);
  case.resources.records=metadata;case.resources.free=store_at(metadata,52);
  case.resources.names=store_at(metadata,56);case.resources.sources=store_at(metadata,60);
  case.resources.data=store_at(metadata,64);case.resources.indices=store_at(metadata,68);
  case.resources.pages=store_at(metadata,72);
  let mut slot=0u16;while slot<case.resources.count{{
   let row=store_at(case.resources.records,slot*13);
   let source=scene_meshes::source(&case.resources,slot);
   let indices=unsafe{{Ptr::from_addr(case.resources.indices[slot]).as_u16_array()}};
   let mapped=page_indices(indices,source[1u16]*3);
   let mut addresses=case.resources.indices;addresses[slot]=mapped.as_ptr().addr();
   let mut flags=row;flags[0u16]=(flags[0u16]&3)|(202<<2);slot+=1;
  }}
  case.model.indices=page_indices(case.model.indices,case.model.geometry[1u16]*3);case.model.index_page=201;
  case.object.motion_page=200;
  let view=scene_meshes::view(&case.resources,case.object.private_mesh.lo_bits(),false);
  case.object.mesh=view.mesh;case.object.index_page=view.index_page;
  let collider=unsafe{{Ptr::from_addr(0xbe00).as_u16_array()}};
  store_copy(collider,case.object.collider,0,60);case.object.collider=collider;
  let gains=unsafe{{Ptr::from_addr(0xbe40).as_u16_array()}};
  store_copy(gains,case.object.config.gains,0,10);case.object.config.gains=gains;
 }}
 while frame<case.count{{run_auxiliary_hit(&mut case,frame,out,&mut map);if PAGED && (read_dseg_page(2)!=202 || read_dseg_page(3)!=200){{halt(0x7f92);}}let mut progress=input;progress[0u16]=frame+1;frame+=1;}}
 scene_meshes::finish(&mut case.resources);map.terrain.finish();halt(0x600d);
}}
{}
{}
"#,
            paged,
            include_str!("mesh_store_case.rs"),
            case_source
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
                    assert_eq!(
                        u64::from(memory[0x1800 + i]),
                        v.as_u64().unwrap(),
                        "auxiliary case {index}, object {}, variant {}, limit {}, frame {done}, word {i}",
                        case["object"],
                        case["variant"],
                        case["limit"]
                    );
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
        "maximum_integer_error":0,"mapped_indices":paged,"models":data["names"],"scope":data["scope"],
        "boundary":"Actual auxiliary-contact method with authored objects/local resource arena and real terrain pages; outer car collision dispatch and complete-world resource construction remain separate"}),
    )
}
