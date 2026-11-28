//! Persistent auxiliary-update class sequences against original full-world state.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let mut report = run_layout(output, fixture, false)?;
    report["mapped_indices"] = run_layout(output, fixture, true)?;
    Ok(report)
}
fn run_layout(output: &Path, fixture: &Path, paged: bool) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("scene-fragments.json"))?)?;
    let label = if paged {
        "scene-fragments-paged"
    } else {
        "scene-fragments"
    };
    let code = compile(
        label,
        &format!(
            r#"
const PAGED:bool={};
fn main(){{
 let input=unsafe{{Ptr::from_addr(0x0100).as_u16_array()}};
 let out=unsafe{{Ptr::from_addr(0x1900).as_u16_array()}};
 let storage=unsafe{{Ptr::from_addr(0x8000).as_u16_array()}};
 let mut scratch=unsafe{{Ptr::from_addr(0xb000).as_u16_array()}};
 if PAGED{{scratch=unsafe{{Ptr::from_addr(0x5000).as_u16_array()}};}}
 let sine_root:Buf<u16,4>=Buf::new([49152,320,40960,0]);
 let mut sine=unsafe{{Ptr::from_addr(0x3000).as_i16_array()}};
 if PAGED{{sine=unsafe{{sine_root.as_array().as_ptr().as_i16_array()}};}}
 let words=attitude::get(input,7).lo_bits();
 let asset=static_assets::AssetBytes{{first_sector:40960,sector_count:(words+63)>>6,byte_len_lo:words<<1,byte_len_hi:words>>15}};
 let scene=scene_query::Scene::begin(asset);if !scene.valid{{halt(0x7f21);}}
 let mut case=load_scene_fragment_case(input,storage,scratch);
 if PAGED{{
  let indices=case.object.mesh.indices;let address=indices.as_ptr().addr();
  let saved=read_dseg_page(3);unsafe{{write_dseg_page(3,201);}}
  copy(unsafe{{Ptr::from_addr(0xc000+(address&16383)).as_u16_array()}},indices,case.object.mesh.triangle_count*3);
  unsafe{{write_dseg_page(3,saved);}}
  case.object.mesh.indices=unsafe{{Ptr::from_addr(0x8000+(address&16383)).as_u16_array()}};
  case.object.index_page=201;
  let collider=unsafe{{Ptr::from_addr(0xb000).as_u16_array()}};
  copy(collider,case.object.collider,60);case.object.collider=collider;
  let gains=unsafe{{Ptr::from_addr(0xb040).as_u16_array()}};
  copy(gains,case.object.config.gains,10);case.object.config.gains=gains;
 }}
 let saved3=read_dseg_page(3);
 let mut frame=0u16;
 while frame<case.count{{
  run_scene_fragment_step(&scene,&mut case,frame,out,sine);
  if PAGED && (read_dseg_page(2)!=2 || read_dseg_page(3)!=saved3){{halt(0x7f92);}}
  let mut progress=input;progress[0u16]=frame+1;
  frame+=1;
 }}
 halt(0x600d);
}}
{}
{}
{}
"#,
            paged,
            include_str!("scene_lifecycle_case.rs"),
            include_str!("mesh_break_case.rs"),
            include_str!("scene_fragment_case.rs")
        ),
        output,
    )?;
    let sine = fs::read(fixture.join("sine.i16.bin"))?;
    let scene = fs::read(fixture.join("scene-fragments.bin"))?;
    let cases = data["cases"].as_array().ok_or("scene fragment cases")?;
    let mut checked = 0usize;
    let mut instructions = 0u64;
    let mut frames = 0usize;
    for (index, case) in cases.iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        let memory = cpu.physical_memory_mut();
        let sine_base = if paged { 0x300000 } else { 0x3000 };
        for (base, bytes) in [(sine_base, &sine), (0x280000, &scene)] {
            for (i, pair) in bytes.chunks_exact(2).enumerate() {
                memory[base + i] = u16::from_le_bytes([pair[0], pair[1]]);
            }
        }
        let input = case["input"].as_array().ok_or("scene fragment input")?;
        assert!(input.len() < 3072);
        for (i, v) in input.iter().enumerate() {
            memory[0x100 + i * 2..0x102 + i * 2]
                .copy_from_slice(&words(v.as_i64().unwrap() as i32));
        }
        memory[0x100] = 0;
        let expected = case["frames"].as_array().ok_or("scene fragment frames")?;
        let mut done = 0usize;
        let mut minimum_stack = 65535u16;
        let mut stack_started = false;
        let mut passed = false;
        for step in 1..=10000000 {
            let outcome = cpu.step().expect("scene fragment instruction");
            let sp = cpu.registers()[13];
            if stack_started || sp != 0 {
                stack_started = true;
                minimum_stack = minimum_stack.min(sp);
            }
            let memory = cpu.physical_memory_mut();
            if usize::from(memory[0x100]) > done {
                assert_eq!(usize::from(memory[0x100]), done + 1);
                let values = expected[done].as_array().unwrap();
                assert!(values.len() < 2944);
                for (i, v) in values.iter().enumerate() {
                    let actual = i32::from(memory[0x1900 + i * 2])
                        | (i32::from(memory[0x1901 + i * 2]) << 16);
                    assert_eq!(
                        i64::from(actual),
                        v.as_i64().unwrap(),
                        "scene fragment case {index}, object {}, variant {}, limit {}, frame {done}, field {i}",
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
                assert_eq!(signal, PASS, "scene fragment case {index}");
                assert_eq!(done, expected.len());
                instructions += step;
                passed = true;
                break;
            }
        }
        assert!(passed, "scene fragment case {index} exceeded bound");
        assert!(
            minimum_stack >= if paged { 0x6040 } else { 0xb600 },
            "scene fragment case {index} crossed fixture scratch at {minimum_stack:04x}"
        );
    }
    Ok(
        json!({"cases":cases.len(),"frames":frames,"checked_i32_values":checked,"maximum_integer_error":0,"mapped_indices":paged,"instruction_steps":instructions,
        "code_bytes":code.total_code_words()*2,"objects":data["objects"],"scope":data["scope"],
        "boundary":"Persistent RCC class calls, RAM sine table and actual class assets; native loading and full world scheduling remain separate"}),
    )
}
