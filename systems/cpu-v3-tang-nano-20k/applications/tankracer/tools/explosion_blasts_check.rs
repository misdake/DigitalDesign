//! Shared blast state, full effect pools and additive projection after each call.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("explosion-blasts.json"))?)?;
    let code = compile(
        "explosion-blasts",
        r#"
fn at(data:Array<u16>,w:u16)->Array<u16>{unsafe{data.as_ptr().add(w as i16).as_u16_array()}}
fn g(data:Array<u16>,i:u16)->fix32{attitude::get(data,i)}
fn p(data:Array<u16>,i:u16,v:fix32){attitude::put(data,i,v);}
fn n(v:i16)->fix32{integer::from_i16(v)}
fn copy(mut to:Array<u16>,from:Array<u16>,count:u16){let mut i=0u16;while i<count{to[i]=from[i];i+=1;}}
fn main(){
 let input=unsafe{Ptr::from_addr(0x100).as_u16_array()};let out=unsafe{Ptr::from_addr(0x3000).as_u16_array()};
 let previous2=read_dseg_page(2);let previous3=read_dseg_page(3);
 unsafe{write_dseg_page(2,120);write_dseg_page(3,119);}
 let controls=unsafe{Ptr::from_addr(0x2000).as_u16_array()};
 let state=explosion_blasts::view(input);let sine:Buf<u16,4>=Buf::new([45056,320,40960,0]);
 let ctx=explosion_blasts::Context{resources:at(input,7152),camera:at(input,7162),sine:unsafe{sine.as_array().as_ptr().as_i16_array()},seed:at(input,7150)};
 let mut quads=quad_pool::Pool{records:at(input,374),exhausted:at(input,5482),count:g(at(input,5514),0)};
 let mut lines=effect_pool::Pool{lines:at(input,5518),count:g(at(input,5516),0),overflow:effect_pool::line_zero(),exhausted:at(input,7134)};
 let overflow=unsafe{view_of(&mut lines.overflow).as_ptr().as_u16_array()};copy(overflow,at(input,7118),16);
 let count=controls[0u16];let mut frame=0u16;
 while frame<count{
  let c=at(controls,1+frame*12);let op=g(c,0).lo_bits();
  let start=explosion_blasts::Start{origin:at(input,7180),owner:g(c,5),time:g(c,4),outer:g(c,2),flipped:g(c,3)!=n(0),kind:g(c,1).lo_bits()&255};
  let step=explosion_blasts::Step{slot:g(c,1),dt:g(c,2)};
  let mut result=explosion_blasts::Outcome{status:n(0),slot:n(-1),has_released:false,owner:n(0),ring:false};
  if op==0{result=explosion_blasts::band(&state,&start,&ctx,&mut quads,&lines);}
  else if op==1{result=explosion_blasts::ring(&state,&start,&ctx,&mut quads,&lines);}
  else if op==2{result=explosion_blasts::tick_band(&state,&step,&mut quads,&lines);}
  else if op==3{result=explosion_blasts::tick_ring(&state,&step,ctx.seed,&mut quads,&lines);}
  else if op==4{result=explosion_blasts::release(&state,g(c,3)!=n(0),g(c,1),&mut quads,&lines);}
  if op>1{result.slot=n(-1);}
  p(out,0,result.status);p(out,1,result.slot);let mut header=out;header[4u16]=0;header[7u16]=0;
  if result.has_released{header[4u16]=1;}p(at(out,5),0,result.owner);if result.ring{header[7u16]=1;}
  p(at(input,5514),0,quads.count);p(at(input,5516),0,lines.count);copy(at(input,7118),overflow,16);
  let projected=explosion_blasts::project(&state,&ctx,at(out,7162));p(at(out,7160),0,projected);
  copy(at(out,8),input,7152);
  if read_dseg_page(2)!=120||read_dseg_page(3)!=119{halt(0x7e72);}
  unsafe{Ptr::from_addr(0xff).write(0,frame+1);}frame+=1;
 }
 unsafe{write_dseg_page(2,previous2);write_dseg_page(3,previous3);}halt(0x600d);
}
"#,
        output,
    )?;
    let sine = fs::read(fixture.join("sine.i16.bin"))?;
    let cases = data["cases"].as_array().ok_or("shared blast cases")?;
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
        let input = case["input"].as_array().ok_or("shared blast input")?;
        assert_eq!(input.len(), 7186);
        for (i, v) in input.iter().enumerate() {
            memory[0x100 + i] = v.as_u64().unwrap() as u16;
        }
        let controls = case["controls"].as_array().unwrap();
        assert!(controls.len() <= 64);
        memory[0x2000] = controls.len() as u16;
        for (frame, c) in controls.iter().enumerate() {
            for (i, v) in c.as_array().unwrap().iter().enumerate() {
                let at = 0x2001 + frame * 12 + i * 2;
                memory[at..at + 2].copy_from_slice(&words(v.as_i64().unwrap() as i32));
            }
        }
        let expected = case["frames"].as_array().unwrap();
        let mut done = 0;
        let mut passed = false;
        let mut started = false;
        for step in 1..=30000000 {
            let result = cpu.step().map_err(|e| format!("blasts {index}: {e:?}"))?;
            let sp = cpu.registers()[13];
            if started || sp != 0 {
                started = true;
                minimum_stack = minimum_stack.min(sp);
                assert!(
                    sp >= 0x7000,
                    "shared blast stack crossed its guarded region"
                );
            }
            let memory = cpu.physical_memory_slice();
            if usize::from(memory[0xff]) > done {
                assert_eq!(usize::from(memory[0xff]), done + 1);
                let answer = expected[done].as_array().unwrap();
                assert!(answer.len() >= 7162 && answer.len() <= 8698);
                for (field, v) in answer.iter().enumerate() {
                    let actual = memory[0x3000 + field];
                    let want = v.as_u64().unwrap() as u16;
                    if actual != want {
                        return Err(format!(
                            "blasts {index} {} call {done} word {field}: {actual} != {want}",
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
            return Err(format!("blasts {index}: instruction bound").into());
        }
    }
    let report = json!({"backend":"instruction, real authored materials and complete effect pools","cases":cases.len(),"calls":calls,"checked_u16_values":checked,"maximum_integer_error":0,"code_bytes":code.total_code_words()*2,"minimum_stack_word":minimum_stack,"maximum_stack_bytes":(0x8000-usize::from(minimum_stack))*2,"instruction_steps":instructions,"blast_state_bytes":748,"maximum_projection_bytes":3072,"scope":data["scope"]});
    fs::write(
        output.join("explosion-blasts-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
