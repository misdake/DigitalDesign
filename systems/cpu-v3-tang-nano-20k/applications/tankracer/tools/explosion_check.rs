//! Original explosion and fragment state checked after every method call.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("explosion.json"))?)?;
    let code = compile(
        "explosion",
        r#"
fn at(data:Array<u16>,w:u16)->Array<u16>{unsafe{data.as_ptr().add(w as i16).as_u16_array()}}
fn g(data:Array<u16>,i:u16)->fix32{attitude::get(data,i)}
fn main(){
 let input=unsafe{Ptr::from_addr(0x100).as_u16_array()};let out=unsafe{Ptr::from_addr(0x1800).as_u16_array()};
 let state=explosion::view(input);let sine:Buf<u16,4>=Buf::new([45056,320,40960,0]);
 let ctx=explosion::Context{resources:at(input,678),seed:at(input,676),sine:unsafe{sine.as_array().as_ptr().as_i16_array()}};
 let count=input[749u16];let mut frame=0u16;
 while frame<count {
  let c=at(input,750+frame*6);let op=g(c,0).lo_bits();let mut status=integer::from_i16(0);
  let step=explosion::Frame{dt:g(c,1),origin:at(input,688),has_owner_pose:input[694u16]!=0,owner_basis:at(input,695),owner_position:at(input,713)};
  if op==0{explosion::initialize(&state,step.origin,g(c,1),&ctx);}
  else if op==1{explosion::finish_spawn(&state);}
  else if op==2{status=explosion::tick(&state,&step,&ctx);}
  else if op==3 {
   let contact=explosion::Contact{owner:g(at(input,719),0),vehicle_velocity:at(input,721),local_point:at(input,727),world_point:at(input,733),velocity:at(input,739),kind:g(c,2).lo_bits(),spin:g(at(input,745),0),has_body:input[747u16]!=0,radius:input[748u16] as i16};
   status=explosion::contact(&state,&contact,&ctx);
  }
  attitude::put(out,0,status);let mut dest=out;let mut i=0u16;while i<678{dest[2+i]=input[i];i+=1;}
  unsafe{Ptr::from_addr(0xff).write(0,frame+1);}frame+=1;
 }
 halt(0x600d);
}
"#,
        output,
    )?;
    let sine = fs::read(fixture.join("sine.i16.bin"))?;
    let cases = data["cases"].as_array().ok_or("explosion cases")?;
    let mut checked = 0;
    let mut calls = 0;
    let mut instructions = 0u64;
    let mut minimum_stack = u16::MAX;
    for (index, case) in cases.iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        let memory = cpu.physical_memory_mut();
        for (i, pair) in sine.chunks_exact(2).enumerate() {
            memory[0x2c0000 + i] = u16::from_le_bytes([pair[0], pair[1]]);
        }
        let input = case["input"].as_array().ok_or("explosion input")?;
        assert_eq!(input.len(), 749);
        for (i, v) in input.iter().enumerate() {
            memory[0x100 + i] = v.as_u64().unwrap() as u16;
        }
        let controls = case["controls"].as_array().unwrap();
        memory[0x100 + 749] = controls.len() as u16;
        assert!(controls.len() <= 32);
        for (frame, c) in controls.iter().enumerate() {
            for (i, v) in c.as_array().unwrap().iter().enumerate() {
                let at = 0x100 + 750 + frame * 6 + i * 2;
                memory[at..at + 2].copy_from_slice(&words(v.as_i64().unwrap() as i32));
            }
        }
        let expected = case["frames"].as_array().unwrap();
        let mut done = 0;
        let mut passed = false;
        let mut started = false;
        for step in 1..=20000000 {
            let result = cpu
                .step()
                .map_err(|e| format!("explosion {index}: {e:?}"))?;
            let sp = cpu.registers()[13];
            if started || sp != 0 {
                started = true;
                minimum_stack = minimum_stack.min(sp);
                assert!(sp >= 0x7000, "explosion stack crossed its guarded region");
            }
            let memory = cpu.physical_memory_slice();
            if usize::from(memory[0xff]) > done {
                assert_eq!(usize::from(memory[0xff]), done + 1);
                let answer = expected[done].as_array().unwrap();
                assert_eq!(answer.len(), 680);
                for (field, v) in answer.iter().enumerate() {
                    let actual = memory[0x1800 + field];
                    let want = v.as_u64().unwrap() as u16;
                    if actual != want {
                        return Err(format!(
                            "explosion {index} {} call {done} word {field}: {actual} != {want}",
                            case["tag"]
                        )
                        .into());
                    }
                    checked += 1;
                }
                done += 1;
                calls += 1;
            }
            if let StepOutcome::Halted { signal } = result {
                assert_eq!(signal, PASS);
                assert_eq!(done, expected.len());
                instructions += step;
                passed = true;
                break;
            }
        }
        if !passed {
            return Err(format!("explosion {index}: instruction bound").into());
        }
    }
    let report = json!({"cases":cases.len(),"calls":calls,"checked_u16_values":checked,"maximum_integer_error":0,"instruction_steps":instructions,"code_bytes":code.total_code_words()*2,"minimum_stack_word":minimum_stack,"maximum_stack_bytes":(0x8000-usize::from(minimum_stack))*2,"explosion_record_bytes":1352,"scope":data["scope"],"backend":"instruction method snapshots, real authored model/resources; shared blasts and scene/native integration remain separate"});
    fs::write(
        output.join("explosion-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
