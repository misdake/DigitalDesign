//! Stateful original weapon-spin calls through the RCC instruction backend.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("spin.json"))?)?;
    let code = compile(
        "spin",
        r#"
fn at(data:Array<u16>,offset:u16)->Array<u16>{unsafe{data.as_ptr().add(offset as i16).as_u16_array()}}
fn main(){
 let input=unsafe{Ptr::from_addr(0x2000).as_u16_array()};
 let out=unsafe{Ptr::from_addr(0x1000).as_u16_array()};
 let shared:Buf<u16,2>=Buf::new([0;2]);let mut words=shared.as_array();
 words[0u16]=attitude::get(input,9).lo_bits();words[1u16]=attitude::get(input,10).lo_bits();
 let seed=at(input,22);let count=attitude::get(input,12).lo_bits();
 let mut frame=0u16;
 while frame<count {
  let c=at(input,26+frame*22);let result=at(out,frame*26);let mut status=fix32::zero();
  if attitude::get(c,0)==fix32::zero(){
   status=spin::start(input,attitude::get(c,2)!=fix32::zero(),attitude::get(c,3),words,seed);
  }else{
   status=spin::step(input,attitude::get(c,1),at(c,10),attitude::get(c,4),words,seed);
  }
  attitude::put(result,0,status);attitude::put(result,1,attitude::get(seed,0));
  let mut i=0u16;while i<9{attitude::put(result,2+i,attitude::get(input,i));i+=1;}
  attitude::put(result,11,fix32::from_words(words[0u16],0));
  attitude::put(result,12,fix32::from_words(words[1u16],0));
  frame+=1;
 }
 halt(0x600d);
}
"#,
        output,
    )?;
    let cases = data["cases"].as_array().ok_or("spin cases")?;
    let mut checked = 0;
    let mut frames = 0;
    let mut steps = 0u64;
    for (index, case) in cases.iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        let input = case["input"].as_array().ok_or("spin input")?;
        assert!(input.len() * 2 < 0x1000);
        for (i, v) in input.iter().enumerate() {
            cpu.physical_memory_mut()[INPUT + i * 2..INPUT + i * 2 + 2]
                .copy_from_slice(&words(v.as_i64().unwrap() as i32));
        }
        let mut passed = false;
        for step in 1..=20000000 {
            if let StepOutcome::Halted { signal } = cpu
                .step()
                .map_err(|e| format!("spin case {index}: {e:?}"))?
            {
                if signal != PASS {
                    return Err(format!("spin case {index}: halt {signal:04x}").into());
                }
                steps += step;
                passed = true;
                break;
            }
        }
        if !passed {
            return Err(format!("spin case {index}: step limit").into());
        }
        let expected = case["frames"].as_array().ok_or("spin frames")?;
        for (frame, row) in expected.iter().enumerate() {
            for (field, value) in row.as_array().ok_or("spin output")?.iter().enumerate() {
                let at = RESULT + (frame * 13 + field) * 2;
                let memory = cpu.physical_memory_slice();
                let actual = i32::from_le_bytes([
                    memory[at] as u8,
                    (memory[at] >> 8) as u8,
                    memory[at + 1] as u8,
                    (memory[at + 1] >> 8) as u8,
                ]);
                let want = value.as_i64().unwrap() as i32;
                if actual != want {
                    return Err(format!(
                        "spin case {index} {} frame {frame} field {field}: {actual} != {want}",
                        case["tag"]
                    )
                    .into());
                }
                checked += 1;
            }
            frames += 1;
        }
    }
    let report = json!({"cases":cases.len(),"frames":frames,"checked_i32_values":checked,"maximum_integer_error":0,
        "code_bytes":code.total_code_words()*2,"instruction_steps":steps,"scope":data["scope"],
        "boundary":"Weapon spin kernel, instruction semantics; full vehicle prefix and normal loading remain separate"});
    fs::write(
        output.join("spin-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
