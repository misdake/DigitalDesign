//! Full light-bank and shared pool state compared after each original call.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("pickup-light.json"))?)?;
    let code = compile(
        "pickup-light",
        r#"
fn at(v:Array<u16>,w:u16)->Array<u16>{unsafe{v.as_ptr().add(w as i16).as_u16_array()}}
fn g(v:Array<u16>,i:u16)->fix32{attitude::get(v,i)}
fn p(v:Array<u16>,i:u16,x:fix32){attitude::put(v,i,x);}
fn n(v:i16)->fix32{integer::from_i16(v)}
fn copy(mut to:Array<u16>,from:Array<u16>,count:u16){let mut i=0u16;while i<count{to[i]=from[i];i+=1;}}
fn main(){
 let input=unsafe{Ptr::from_addr(0x8000).as_u16_array()};
 let controls=unsafe{Ptr::from_addr(0x0100).as_u16_array()};
 let mut lines=effect_pool::Pool{lines:at(input,6776),count:g(input,3387),overflow:effect_pool::line_zero(),exhausted:at(input,8392)};
 let overflow=unsafe{view_of(&mut lines.overflow).as_ptr().as_u16_array()};copy(overflow,at(input,8376),16);
 let mut pool=quad_pool::Pool{records:at(input,1632),exhausted:at(input,6740),count:g(input,3386)};
 let state=pickup_light::State{banks:input};
 let tuning:Buf<u16,36>=Buf::new([0;36]);copy(tuning.as_array(),at(input,8436),6);
 let mut i=0u16;let mut rates=tuning.as_array();while i<30{rates[6+i]=g(input,4221+i).lo_bits();i+=1;}
 let sine:Buf<u16,4>=Buf::new([45056,320,40960,0]);
 let ctx=pickup_light::Context{camera:at(input,8408),materials:at(input,8432),tuning:tuning.as_array(),sine:unsafe{sine.as_array().as_ptr().as_i16_array()}};
 let count=g(controls,0).lo_bits();let mut frame=0u16;
 while frame<count{
  let c=at(controls,2+frame*22);let op=g(c,0);let mut status=n(0);
  if op==n(0){
   let start=pickup_light::Start{target:g(c,3),origin:at(c,8),velocity:at(c,14),frame:g(c,2),follow:g(c,10)!=n(0)};
   status=pickup_light::start(&state,&start,&ctx,&mut pool,&lines);
  }else if op==n(1){let step=pickup_light::Frame{target:g(c,3),position:at(c,8),number:g(c,2),dt:g(c,1)};status=pickup_light::fade(&state,&step,tuning.as_array(),&mut pool,&lines);}
  else if op==n(2){status=pickup_light::project(&state,g(c,3),g(c,2),&ctx,&pool,&lines);}
  else if op==n(3){
   let colors:Buf<u16,4>=Buf::new([2055,2569,3083,3597]);
   effect_pool::update(&mut lines,n(-1),at(c,8),colors.as_array());
  }else if op==n(4){
   let q:Buf<u16,32>=Buf::new([0;32]);i=0;while i<4{copy(at(q.as_array(),i*6),at(c,8),6);i+=1;}
   p(q.as_array(),12,fix32::from_words(43690,61440));p(q.as_array(),13,fix32::from_words(775,2312));
   p(q.as_array(),14,fix32::from_words(2052,2052));p(q.as_array(),15,fix32::from_words(2052,2052));
   if quad_pool::replace(&pool,&lines,n(159),q.as_array())!=0{status=n(-1);}
  }else if op==n(5){
   let kind=g(controls,134);let mut handle=n(999);if kind==n(21){handle=n(160);}else if kind==n(22){handle=n(-1);}
   if quad_pool::release(&mut pool,&lines,handle)!=0{status=n(-1);}
  }else{pickup_light::reset(&state);quad_pool::reset(&mut pool,&lines);effect_pool::reset(&mut lines);}
  p(input,3386,pool.count);p(input,3387,lines.count);copy(at(input,8376),overflow,16);
  unsafe{Ptr::from_addr(0xfe).write(0,status.lo_bits());Ptr::from_addr(0xfd).write(0,status.hi_bits());Ptr::from_addr(0xff).write(0,frame+1);}frame+=1;
 }
 halt(0x600d);
}
"#,
        output,
    )?;
    let sine = fs::read(fixture.join("sine.i16.bin"))?;
    let mut checked = 0;
    let mut frames = 0;
    let mut steps = 0u64;
    let mut minimum_stack = u16::MAX;
    let cases = data["cases"].as_array().ok_or("pickup light cases")?;
    for (index, case) in cases.iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        for (i, pair) in sine.chunks_exact(2).enumerate() {
            cpu.physical_memory_mut()[0x2c0000 + i] = u16::from_le_bytes([pair[0], pair[1]]);
        }
        let input = case["input"].as_array().ok_or("light input")?;
        assert_eq!(input.len(), 4251);
        for (i, v) in input.iter().enumerate() {
            cpu.physical_memory_mut()[0x8000 + i * 2..0x8002 + i * 2]
                .copy_from_slice(&words(v.as_i64().unwrap() as i32));
        }
        let controls = case["controls"].as_array().ok_or("light controls")?;
        cpu.physical_memory_mut()[0x100] = controls.len() as u16;
        for (step, c) in controls.iter().enumerate() {
            for (field, v) in c.as_array().unwrap().iter().enumerate() {
                let at = 0x102 + step * 22 + field * 2;
                cpu.physical_memory_mut()[at..at + 2]
                    .copy_from_slice(&words(v.as_i64().unwrap() as i32));
            }
        }
        // Explicit scenario number follows the control stream.
        cpu.physical_memory_mut()[0x100 + 134 * 2] = index as u16;
        let expected = case["frames"].as_array().ok_or("light answers")?;
        let mut done = 0;
        let mut passed = false;
        let mut started = false;
        for step in 1..=30000000 {
            let result = cpu
                .step()
                .map_err(|e| format!("light case {index}: {e:?}"))?;
            let sp = cpu.registers()[13];
            if started || sp != 0 {
                started = true;
                minimum_stack = minimum_stack.min(sp);
            }
            let memory = cpu.physical_memory_slice();
            if usize::from(memory[0xff]) > done {
                assert_eq!(usize::from(memory[0xff]), done + 1);
                for (field, v) in expected[done].as_array().unwrap().iter().enumerate() {
                    let actual = if field == 0 {
                        i32::from(memory[0xfe]) | (i32::from(memory[0xfd]) << 16)
                    } else {
                        let at = 0x8000 + (field - 1) * 2;
                        i32::from(memory[at]) | (i32::from(memory[at + 1]) << 16)
                    };
                    if i64::from(actual) != v.as_i64().unwrap() {
                        return Err(format!(
                            "light case {index} frame {done} field {field}: {actual} != {v}"
                        )
                        .into());
                    }
                    checked += 1;
                }
                done += 1;
                frames += 1;
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
            return Err(format!("light case {index}: instruction limit").into());
        }
    }
    let report = json!({"cases":cases.len(),"calls":frames,"checked_i32_values":checked,"maximum_integer_error":0,"instruction_steps":steps,
        "code_bytes":code.total_code_words()*2,"minimum_stack_word":minimum_stack,"quad_and_light_bytes":13572,"external_effect_bytes":13544,"shared_state_bytes":16816,
        "boundary":"Original pickup start/fade/project and shared quad/line aliases in instruction sim; live collection integration and native loading remain separate"});
    fs::write(
        output.join("pickup-light-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
