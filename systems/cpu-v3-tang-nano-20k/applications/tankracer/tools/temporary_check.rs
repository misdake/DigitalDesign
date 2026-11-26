//! Shared temporary effects compared after each original operation.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("temporary.json"))?)?;
    let code = compile(
        "temporary",
        r#"
fn at(v:Array<u16>,w:u16)->Array<u16>{unsafe{v.as_ptr().add(w as i16).as_u16_array()}}
fn g(v:Array<u16>,i:u16)->fix32{attitude::get(v,i)}
fn copy(mut to:Array<u16>,from:Array<u16>,count:u16){let mut i=0u16;while i<count{to[i]=from[i];i+=1;}}
fn main(){
 let input=unsafe{Ptr::from_addr(0x0100).as_u16_array()};let out=unsafe{Ptr::from_addr(0x1800).as_u16_array()};
 let state=temporary::State{core:input,rows:at(input,82)};
 let pools:Buf<effect_pool::Pool,1>=Buf::new([effect_pool::Pool{lines:at(input,280),count:g(at(input,278),0),overflow:effect_pool::line_zero(),exhausted:at(input,1896)}]);
 let mut all=pools.as_array();let mut pool=&mut all[0u16];
 copy(unsafe{view_of(&mut pool.overflow).as_ptr().as_u16_array()},at(input,1880),16);
 let sine:Buf<u16,4>=Buf::new([45056,320,40960,0]);
 let ctx=temporary::Context{pool:pools.as_array(),seed:at(input,276),sine:unsafe{sine.as_array().as_ptr().as_i16_array()}};
 let count=input[1912u16];let mut frame=0u16;
 while frame<count{
  let c=at(input,1913+frame*12);let op=g(c,0).lo_bits();let timer=g(c,1).lo_bits() as i16;let slot=g(at(input,274),0).lo_bits() as i16;let mut status=fix32::zero();
  if op==0{status=temporary::start(&state,timer,at(input,274),at(c,6),g(c,2),&ctx);}
  else if op==1{status=temporary::tick(&state,timer,slot,at(c,6),&ctx);}
  else if op==2{status=temporary::expire(&state,slot,&ctx);}
  else if op==3{temporary::reset(&state);}
  all=pools.as_array();pool=&mut all[0u16];
  attitude::put(at(input,278),0,pool.count);copy(at(input,1880),unsafe{view_of(&mut pool.overflow).as_ptr().as_u16_array()},16);
  attitude::put(out,0,status);copy(at(out,2),input,1912);
  unsafe{Ptr::from_addr(0xff).write(0,frame+1);}frame+=1;
 }
 halt(0x600d);
}
"#,
        output,
    )?;
    let sine = fs::read(fixture.join("sine.i16.bin"))?;
    let cases = data["cases"].as_array().ok_or("temporary cases")?;
    let mut checked = 0;
    let mut calls = 0;
    let mut steps = 0u64;
    let mut minimum_stack = u16::MAX;
    for (index, case) in cases.iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        for (i, pair) in sine.chunks_exact(2).enumerate() {
            cpu.physical_memory_mut()[0x2c0000 + i] = u16::from_le_bytes([pair[0], pair[1]]);
        }
        let input = case["input"].as_array().ok_or("temporary input")?;
        assert_eq!(input.len(), 1912);
        for (i, v) in input.iter().enumerate() {
            cpu.physical_memory_mut()[0x100 + i] = v.as_u64().unwrap() as u16;
        }
        let controls = case["controls"].as_array().ok_or("temporary controls")?;
        cpu.physical_memory_mut()[0x100 + 1912] = controls.len() as u16;
        for (frame, c) in controls.iter().enumerate() {
            for (i, v) in c.as_array().unwrap().iter().enumerate() {
                let at = 0x100 + 1913 + frame * 12 + i * 2;
                cpu.physical_memory_mut()[at..at + 2]
                    .copy_from_slice(&words(v.as_i64().unwrap() as i32));
            }
        }
        let expected = case["frames"].as_array().unwrap();
        let mut done = 0;
        let mut passed = false;
        let mut started = false;
        for step in 1..=20000000 {
            let result = cpu
                .step()
                .map_err(|e| format!("temporary case {index}: {e:?}"))?;
            let sp = cpu.registers()[13];
            if started || sp != 0 {
                started = true;
                minimum_stack = minimum_stack.min(sp);
            }
            let memory = cpu.physical_memory_slice();
            if usize::from(memory[0xff]) > done {
                assert_eq!(usize::from(memory[0xff]), done + 1);
                for (field, v) in expected[done].as_array().unwrap().iter().enumerate() {
                    let actual = memory[0x1800 + field];
                    let want = v.as_u64().unwrap() as u16;
                    if actual != want {
                        return Err(format!("temporary case {index} {} call {done} word {field}: {actual} != {want}",case["tag"]).into());
                    }
                    checked += 1;
                }
                done += 1;
                calls += 1;
            }
            if let StepOutcome::Halted { signal } = result {
                assert_eq!(signal, PASS);
                assert_eq!(done, expected.len());
                steps += step;
                passed = true;
                break;
            }
        }
        if !passed {
            return Err(format!("temporary case {index}: instruction bound").into());
        }
    }
    let report = json!({"cases":cases.len(),"calls":calls,"checked_u16_values":checked,"maximum_integer_error":0,"instruction_steps":steps,
        "code_bytes":code.total_code_words()*2,"minimum_stack_word":minimum_stack,"temporary_record_bytes":548,"scope":data["scope"],"backend":"instruction; shared line state without GPU competition"});
    fs::write(
        output.join("temporary-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
