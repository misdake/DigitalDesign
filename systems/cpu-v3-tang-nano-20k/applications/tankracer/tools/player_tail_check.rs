//! Post-physics publication against the original Player tail, with guards.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("player-tail.json"))?)?;
    let code = compile(
        "player-tail",
        r#"
fn ptr(w:u16)->Array<u16>{unsafe{Ptr::from_addr(w).as_u16_array()}}
fn at(v:Array<u16>,w:u16)->Array<u16>{unsafe{v.as_ptr().add(w as i16).as_u16_array()}}
fn main(){
 let fields=ptr(0x1000);let binding=ptr(0x1020);let raw=ptr(0x1100);
 let state=player_tail::State{fields:fields,binding:binding,gun:unsafe{Ptr::from_addr(0x1100).as_i16_array()}};
 let output=ptr(0x1800);let count=ptr(0x1ff0)[0u16];let words=ptr(0x1ff1)[0u16];let mut frame=0u16;
 while frame<count{
  let c=at(ptr(0x2000),frame*(words+7));player_tail::capture(&state,output);
  let mut gun=raw;let mut i=0u16;while i<words{gun[i]=c[7+i];i+=1;}
  let expired=player_tail::finish(&state,output,attitude::get(c,0),c[2u16],c[3u16]!=0);
  let mut status=attitude::get(at(c,5),0);
  if expired{if c[4u16]!=0{let mut fields=fields;fields[2u16]=0;}else{status=integer::from_i16(-1);}}
  let mut o=output;i=0;while i<7{o[5+i]=fields[i];i+=1;}
  attitude::put(at(output,12),0,status);
  unsafe{Ptr::from_addr(0xff).write(0,frame+1);}frame+=1;
 }
 halt(0x600d);
}
"#,
        output,
    )?;
    let cases = data["cases"].as_array().ok_or("Player tail cases")?;
    let mut calls = 0;
    let mut checked = 0;
    let mut steps = 0u64;
    let mut minimum = 0x8000u16;
    for (index, case) in cases.iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        let values = |value: &Value| -> Vec<u16> {
            value
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_u64().unwrap() as u16)
                .collect()
        };
        let initial = values(&case["fields"]);
        let binding = values(&case["binding"]);
        let gun = values(&case["gun"]);
        assert_eq!(initial.len(), 7);
        assert_eq!(binding.len(), 6);
        assert!(gun.len() <= 768);
        let frames = case["frames"].as_array().unwrap();
        assert!(frames.len() * (gun.len() + 7) < 0x1000);
        let memory = cpu.physical_memory_mut();
        memory[0x1000..0x1007].copy_from_slice(&initial);
        memory[0x1007..0x1020].fill(0x79ab);
        memory[0x1020..0x1026].copy_from_slice(&binding);
        memory[0x1026..0x1040].fill(0x79ab);
        memory[0x1100..0x1100 + gun.len()].copy_from_slice(&gun);
        memory[0x1100 + gun.len()..0x1100 + gun.len() + 16].fill(0x79ab);
        memory[0x180e..0x1840].fill(0x79ab);
        memory[0x1ff0] = frames.len() as u16;
        memory[0x1ff1] = gun.len() as u16;
        for (number, frame) in frames.iter().enumerate() {
            let offset = 0x2000 + number * (gun.len() + 7);
            memory[offset..offset + 2]
                .copy_from_slice(&words(frame["dt"].as_i64().unwrap() as i32));
            memory[offset + 2] = frame["flags"].as_u64().unwrap() as u16;
            memory[offset + 3] = u16::from(frame["success"].as_bool().unwrap());
            memory[offset + 4] = u16::from(frame["peer_ok"].as_bool().unwrap());
            let status = if frame["success"].as_bool().unwrap() {
                0
            } else {
                -77
            };
            memory[offset + 5..offset + 7].copy_from_slice(&words(status));
            memory[offset + 7..offset + 7 + gun.len()].copy_from_slice(&values(&frame["gun"]));
        }
        let mut done = 0;
        let mut passed = false;
        for step in 1..=1000000 {
            let outcome = cpu
                .step()
                .map_err(|e| format!("tail case {index}: {e:?}"))?;
            let sp = cpu.registers()[13];
            if sp != 0 {
                minimum = minimum.min(sp);
            }
            let memory = cpu.physical_memory_slice();
            if usize::from(memory[0xff]) > done {
                assert_eq!(usize::from(memory[0xff]), done + 1);
                let mut expected = values(&frames[done]["before"]);
                expected.extend(values(&frames[done]["fields"]));
                expected.extend(words(frames[done]["status"].as_i64().unwrap() as i32));
                for (field, &wanted) in expected.iter().enumerate() {
                    let actual = memory[0x1800 + field];
                    if actual != wanted {
                        return Err(format!(
                            "tail case {index} call {done} word {field}: {actual} != {wanted}"
                        )
                        .into());
                    }
                    checked += 1;
                }
                assert_eq!(&memory[0x1000..0x1007], &expected[5..12]);
                assert_eq!(&memory[0x1020..0x1026], binding.as_slice());
                assert_eq!(
                    &memory[0x1100..0x1100 + gun.len()],
                    values(&frames[done]["gun"]).as_slice()
                );
                for (start, end) in [
                    (0x1007, 0x1020),
                    (0x1026, 0x1040),
                    (0x1100 + gun.len(), 0x1110 + gun.len()),
                    (0x180e, 0x1840),
                ] {
                    assert!(memory[start..end].iter().all(|x| *x == 0x79ab));
                }
                done += 1;
                calls += 1;
            }
            if let StepOutcome::Halted { signal } = outcome {
                assert_eq!(signal, PASS);
                assert_eq!(done, frames.len());
                steps += step;
                passed = true;
                break;
            }
        }
        assert!(passed, "tail case {index}: instruction bound");
    }
    let report = json!({"cases":cases.len(),"calls":calls,"checked_u16_values":checked,"maximum_integer_error":0,
        "instruction_steps":steps,"code_bytes":code.total_code_words()*2,"maximum_stack_bytes":(0x8000usize-usize::from(minimum))*2,
        "scope":data["scope"],"backend":"instruction, original post-physics Player body without GPU competition"});
    fs::write(
        output.join("player-tail-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
