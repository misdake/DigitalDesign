//! Spray/marks/lights on one 32-KiB phase bank and the original shared pool.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("track-effects.json"))?)?;
    let code = compile(
        "track-effects",
        r#"
fn ptr(a:u16)->Array<u16>{unsafe{Ptr::from_addr(a).as_u16_array()}}
fn at(v:Array<u16>,w:u16)->Array<u16>{unsafe{v.as_ptr().add(w as i16).as_u16_array()}}
fn g(v:Array<u16>,i:u16)->fix32{attitude::get(v,i)}
fn p(v:Array<u16>,i:u16,x:fix32){attitude::put(v,i,x);}
fn copy(mut to:Array<u16>,from:Array<u16>,count:u16){let mut i=0u16;while i<count{to[i]=from[i];i+=1;}}
fn main(){
 let previous=read_dseg_page(2);let previous3=read_dseg_page(3);unsafe{write_dseg_page(2,137);}
 let s=ptr(0x8000);let mut lines=effect_pool::Pool{page:65535,lines:at(s,5528),count:g(s,2763),overflow:effect_pool::line_zero(),exhausted:at(s,7144)};
 let line_overflow=unsafe{view_of(&mut lines.overflow).as_ptr().as_u16_array()};copy(line_overflow,at(s,7128),16);
 let mut pool=quad_pool::Pool{records:at(s,384),exhausted:at(s,5492),count:g(s,2762)};
 let spray=track_effects::Spray{header:s,particles:at(s,6)};
 let tuning=track_effects::SprayTuning{kinds:unsafe{ptr(0x2400).as_ptr().as_i16_array()},styles:ptr(0x2420)};
 let count=ptr(0x2300)[0u16];let mut frame=0u16;
 while frame<count{
  let c=ptr(0x3000+frame*160);let op=g(c,0).lo_bits();let f=at(c,2);let mut status=fix32::zero();
  if op==0 || op==3{
   status=track_effects::spray(&spray,&track_effects::SprayInput{dt:g(f,0),track_speed:g(f,1),velocity:at(f,34),basis:at(f,46),materials:at(f,64),kinds:at(f,76),kind_stride:1,contacts:at(f,84),contact_stride:3,visible:g(f,7)},&tuning);
  }
  if op==1 || (op==3 && status==fix32::zero()){
   status=track_effects::marks(at(s,198),&track_effects::MarksInput{player:g(f,5)!=fix32::zero(),deformable:g(f,6)!=fix32::zero(),mode:g(f,4),dt:g(f,0),lateral:g(f,3),slip:g(f,2),velocity:at(f,34),materials:at(f,64),travel:at(f,68),kinds:at(f,76),kind_stride:1,contacts:at(f,84),contact_stride:3,tracks:at(f,108),track_stride:9},&mut pool,&lines);
  }
  if op==2 || (op==3 && status==fix32::zero()){
   status=track_effects::lights(at(s,374),&track_effects::LightInput{player:g(f,5)!=fix32::zero(),disabled:g(f,8)!=fix32::zero(),active:g(f,9)!=fix32::zero(),visible:g(f,7),queued:g(f,10)!=fix32::zero(),reverse_braking:g(f,11)!=fix32::zero(),material:g(f,15),throttle:g(f,12),brake:g(f,13),forward:g(f,14),position:at(f,40),basis:at(f,46),has_basis:g(f,16)!=fix32::zero(),materials:at(f,64),heights:at(f,144),height_stride:1,offset:at(f,152)},&mut pool,&lines);
  }
  p(s,2762,pool.count);p(s,2763,lines.count);copy(at(s,7128),line_overflow,16);
  if read_dseg_page(2)!=137 || read_dseg_page(3)!=previous3{halt(0x7e8f);}
  unsafe{Ptr::from_addr(0xfe).write(0,status.lo_bits());Ptr::from_addr(0xfd).write(0,status.hi_bits());Ptr::from_addr(0xff).write(0,frame+1);}frame+=1;
 }
 unsafe{write_dseg_page(2,previous);}halt(0x600d);
}
"#,
        output,
    )?;
    let cases = data["cases"].as_array().unwrap();
    let mut calls = 0;
    let mut checked = 0;
    let mut instructions = 0u64;
    let mut minimum_stack = u16::MAX;
    for (index, case) in cases.iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        let memory = cpu.physical_memory_mut();
        let input = case["input"].as_array().unwrap();
        assert_eq!(input.len(), 7160);
        for (i, v) in input.iter().enumerate() {
            memory[137 * 16384 + i] = v.as_u64().unwrap() as u16;
        }
        memory[137 * 16384 + 7160..138 * 16384].fill(0x79ab);
        let kinds = case["kinds"].as_array().unwrap();
        assert_eq!(kinds.len(), 16);
        for (i, v) in kinds.iter().enumerate() {
            memory[0x2400 + i] = v.as_i64().unwrap() as u16;
        }
        let styles = case["styles"].as_array().unwrap();
        assert_eq!(styles.len(), 48);
        for (i, v) in styles.iter().enumerate() {
            memory[0x2420 + i] = v.as_u64().unwrap() as u16;
        }
        let controls = case["controls"].as_array().unwrap();
        assert!(controls.len() <= 32);
        memory[0x2300] = controls.len() as u16;
        for (frame, c) in controls.iter().enumerate() {
            assert_eq!(c.as_array().unwrap().len(), 80);
            for (i, v) in c.as_array().unwrap().iter().enumerate() {
                let at = 0x3000 + frame * 160 + i * 2;
                memory[at..at + 2].copy_from_slice(&words(v.as_i64().unwrap() as i32));
            }
        }
        let expected = case["frames"].as_array().unwrap();
        let mut done = 0;
        let mut passed = false;
        let mut started = false;
        for step in 1..=3000000 {
            let result = cpu
                .step()
                .map_err(|e| format!("track-effects case {index}: {e:?}"))?;
            let sp = cpu.registers()[13];
            if started || sp != 0 {
                started = true;
                minimum_stack = minimum_stack.min(sp);
                assert!(sp >= 0x7000);
            }
            let memory = cpu.physical_memory_slice();
            if usize::from(memory[0xff]) > done {
                assert_eq!(usize::from(memory[0xff]), done + 1);
                let expected = expected[done].as_array().unwrap();
                assert_eq!(expected.len(), 7162);
                for (word, v) in expected.iter().enumerate() {
                    let at = if word == 0 {
                        0xfe
                    } else if word == 1 {
                        0xfd
                    } else {
                        137 * 16384 + word - 2
                    };
                    let actual = memory[at];
                    let want = v.as_u64().unwrap() as u16;
                    if actual != want {
                        return Err(format!("track-effects case {index} {} call {done} word {word}: {actual} != {want}",case["tag"]).into());
                    }
                    checked += 1;
                }
                assert!(
                    memory[137 * 16384 + 7160..138 * 16384]
                        .iter()
                        .all(|v| *v == 0x79ab)
                );
                for (i, v) in kinds.iter().enumerate() {
                    assert_eq!(memory[0x2400 + i], v.as_i64().unwrap() as u16);
                }
                for (i, v) in styles.iter().enumerate() {
                    assert_eq!(memory[0x2420 + i], v.as_u64().unwrap() as u16);
                }
                for (frame, c) in controls.iter().enumerate() {
                    for (i, v) in c.as_array().unwrap().iter().enumerate() {
                        let at = 0x3000 + frame * 160 + i * 2;
                        assert_eq!(&memory[at..at + 2], &words(v.as_i64().unwrap() as i32));
                    }
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
            return Err(format!("track-effects case {index}: instruction bound").into());
        }
    }
    let report = json!({"cases":cases.len(),"calls":calls,"checked_u16_values":checked,"maximum_integer_error":0,"instruction_steps":instructions,"code_bytes":code.total_code_words()*2,"minimum_stack_word":minimum_stack,"maximum_stack_bytes":(0x8000-usize::from(minimum_stack))*2,"phase_state_bytes":14320,"spray_bytes":396,"marks_bytes":352,"lights_bytes":20,"shared_pool_bytes":13552,"scope":data["scope"],"backend":"instruction, one 32-KiB phase bank and shared quad/line state without GPU competition"});
    fs::write(
        output.join("track-effects-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
