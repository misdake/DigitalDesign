//! Real terrain and shared banked quad state for the original crater queue.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("projectile-marks.json"))?)?;
    let code = compile(
        "projectile-marks",
        r#"
fn pointer(at:u16)->Array<u16>{unsafe{Ptr::from_addr(at).as_u16_array()}}
fn g(v:Array<u16>,i:u16)->fix32{attitude::get(v,i)}
fn p(v:Array<u16>,i:u16,x:fix32){attitude::put(v,i,x);}
fn main(){
 let saved2=read_dseg_page(2);let saved3=read_dseg_page(3);
 unsafe{write_dseg_page(2,120);write_dseg_page(3,119);}
 let directory:Buf<u16,64>=Buf::new([0;64]);let mut indices=directory.as_array();let mut i=0u16;
 while i<64{indices[i]=i;i+=1;}
 let mut map=ground::World::begin(static_assets::AssetBytes{first_sector:49152,sector_count:16384,byte_len_lo:0,byte_len_hi:32},integer::from_i16(0),indices);
 unsafe{write_dseg_page(3,119);}
 let marks=projectile_marks::State{core:pointer(0x9bee)};
 let mut quads=quad_pool::Pool{records:pointer(0x8000),exhausted:pointer(0x93f4),count:g(pointer(0x9414),0)};
 let mut lines=effect_pool::Pool{page:65535,lines:pointer(0x9418),exhausted:pointer(0x9a68),count:g(pointer(0x9416),0),overflow:effect_pool::line_zero()};
 let overflow=unsafe{view_of(&mut lines.overflow).as_ptr().as_u16_array()};
 i=0;let mut extra=overflow;while i<16{extra[i]=pointer(0x9a58)[i];i+=1;}
 let mut frame=0u16;
 while frame<pointer(0x90)[0u16]{
  let c=pointer(0x2000+frame*8);let request=projectile_marks::Mark{origin:c,material:g(c,3)};
  let status=projectile_marks::add(&marks,&request,&mut map,&mut quads,&lines);
  p(pointer(0xf0),0,status);p(pointer(0x9414),0,quads.count);p(pointer(0x9416),0,lines.count);
  i=0;let mut tail=pointer(0x9a58);while i<16{tail[i]=overflow[i];i+=1;}
  if read_dseg_page(2)!=120 || read_dseg_page(3)!=119{halt(0x7e7a);}
  unsafe{Ptr::from_addr(0xff).write(0,frame+1);}frame+=1;
 }
 map.terrain.finish();unsafe{write_dseg_page(2,saved2);write_dseg_page(3,saved3);}halt(0x600d);
}
"#,
        output,
    )?;
    let cells = fs::read(fixture.join("projectile-cells.bin"))?;
    let cases = data["cases"].as_array().ok_or("marks cases")?;
    let mut checked = 0;
    let mut calls = 0;
    let mut steps = 0;
    let mut minimum_stack = u16::MAX;
    for (index, case) in cases.iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        let memory = cpu.physical_memory_mut();
        for (i, pair) in cells.chunks_exact(2).enumerate() {
            memory[0x300000 + i] = u16::from_le_bytes([pair[0], pair[1]]);
        }
        let input = case["input"].as_array().unwrap();
        assert_eq!(input.len(), 6830);
        let base = 120 * 16384;
        memory[base..base + 16384].fill(0x79ab);
        for (i, v) in input.iter().take(54).enumerate() {
            memory[base + 7150 + i] = v.as_u64().unwrap() as u16;
        }
        for (i, v) in input.iter().skip(54).enumerate() {
            memory[base + i] = v.as_u64().unwrap() as u16;
        }
        let controls = case["controls"].as_array().unwrap();
        assert!(controls.len() <= 64);
        memory[0x90] = controls.len() as u16;
        for (frame, c) in controls.iter().enumerate() {
            for (i, v) in c.as_array().unwrap().iter().enumerate() {
                memory[0x2000 + frame * 8 + i * 2..0x2002 + frame * 8 + i * 2]
                    .copy_from_slice(&words(v.as_i64().unwrap() as i32));
            }
        }
        let expected = case["frames"].as_array().unwrap();
        let mut done = 0;
        let mut passed = false;
        for step in 1..=20000000 {
            let result = cpu.step().map_err(|e| format!("marks {index}: {e:?}"))?;
            let sp = cpu.registers()[13];
            if sp != 0 {
                minimum_stack = minimum_stack.min(sp);
                assert!(sp >= 0x7000);
            }
            let memory = cpu.physical_memory_slice();
            if usize::from(memory[0xff]) > done {
                assert_eq!(usize::from(memory[0xff]), done + 1);
                for (field, v) in expected[done].as_array().unwrap().iter().enumerate() {
                    let at = if field < 2 {
                        0xf0 + field
                    } else if field < 56 {
                        base + 7150 + field - 2
                    } else {
                        base + field - 56
                    };
                    let want = v.as_u64().unwrap() as u16;
                    if memory[at] != want {
                        return Err(format!(
                            "marks {index} {} call {done} word {field}: {} != {want}",
                            case["tag"], memory[at]
                        )
                        .into());
                    }
                    checked += 1;
                }
                assert!(
                    memory[base + 6776..base + 7150]
                        .iter()
                        .all(|v| *v == 0x79ab)
                );
                assert!(
                    memory[base + 7204..base + 16384]
                        .iter()
                        .all(|v| *v == 0x79ab)
                );
                done += 1;
                calls += 1;
            }
            if let StepOutcome::Halted { signal } = result {
                assert_eq!(signal, PASS);
                assert_eq!(done, expected.len());
                passed = true;
                steps += step;
                break;
            }
        }
        assert!(passed, "marks {index} step bound");
    }
    let report = json!({"cases":cases.len(),"calls":calls,"checked_u16_values":checked,"maximum_integer_error":0,"code_bytes":code.total_code_words()*2,"maximum_stack_bytes":(32768-usize::from(minimum_stack))*2,"mark_state_bytes":108,"shared_effect_pool_bytes":13552,"instruction_steps":steps,"scope":data["scope"],"backend":"instruction with real terrain and banked effect pool; no GPU competition"});
    fs::write(
        output.join("projectile-marks-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
