//! Full part-pose update checked after each original call and partial failure.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("vehicle-pose.json"))?)?;
    let code = compile(
        "vehicle-pose",
        r#"
fn pointer(address:u16)->Array<u16>{unsafe{Ptr::from_addr(address).as_u16_array()}}
fn at(v:Array<u16>,w:u16)->Array<u16>{unsafe{v.as_ptr().add(w as i16).as_u16_array()}}
fn copy(mut out:Array<u16>,from:Array<u16>,words:u16){let mut i=0u16;while i<words{out[i]=from[i];i+=1;}}
fn main(){
 let s=pointer(0x100);let out=pointer(0x800);
 let state=vehicle_pose::State{physical_position:s,reference:at(s,6),base_position:at(s,24),body_basis:at(s,30),body_position:at(s,48),tracks:at(s,54),turret:at(s,102),gun:at(s,126),attachments:at(s,150),bottom_position:at(s,198),recoil:at(s,204)};
 let sine:Buf<u16,4>=Buf::new([45056,320,40960,0]);let angle:Buf<u16,4>=Buf::new([45376,5,514,0]);
 unsafe{write_dseg_page(2,120);write_dseg_page(3,119);}
 let mut frame=0u16;let count=pointer(0x240)[0u16];
 while frame<count{
  let control=pointer(0x250+frame*44);
  let status=vehicle_pose::update(&state,pointer(0x200),control,&vehicle_pose::Environment{timer:attitude::get(at(control,8),0).lo_bits(),values:at(control,8)},unsafe{sine.as_array().as_ptr().as_i16_array()},unsafe{angle.as_array().as_ptr().as_i16_array()});
  if read_dseg_page(2)!=120 || read_dseg_page(3)!=119{halt(0x7e80);}
  attitude::put(out,0,status);copy(at(out,2),s,208);
  unsafe{Ptr::from_addr(0xff).write(0,frame+1);}frame+=1;
 }
 halt(0x600d);
}
"#,
        output,
    )?;
    let cases = data["cases"].as_array().ok_or("pose cases")?;
    let sine = fs::read(fixture.join("sine.i16.bin"))?;
    let angle = fs::read(fixture.join("angle.i16.bin"))?;
    let mut calls = 0;
    let mut checked = 0;
    let mut instructions = 0u64;
    let mut minimum_stack = u16::MAX;
    for (index, case) in cases.iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        let memory = cpu.physical_memory_mut();
        for (base, bytes) in [(0x2c0000, &sine), (45376 * 64, &angle)] {
            for (i, pair) in bytes.chunks_exact(2).enumerate() {
                memory[base + i] = u16::from_le_bytes([pair[0], pair[1]]);
            }
        }
        let state = case["state"].as_array().unwrap();
        assert_eq!(state.len(), 208);
        for (i, v) in state.iter().enumerate() {
            memory[0x100 + i] = v.as_u64().unwrap() as u16;
        }
        let config = case["config"].as_array().unwrap();
        assert_eq!(config.len(), 54);
        for (i, v) in config.iter().enumerate() {
            memory[0x200 + i] = v.as_u64().unwrap() as u16;
        }
        memory[0x1d0..0x200].fill(0x79ab);
        memory[0x236..0x240].fill(0x79ab);
        let controls = case["controls"].as_array().unwrap();
        assert!(controls.len() <= 8);
        memory[0x240] = controls.len() as u16;
        for (frame, c) in controls.iter().enumerate() {
            let c = c.as_array().unwrap();
            assert_eq!(c.len(), 22);
            for (i, v) in c.iter().enumerate() {
                let at = 0x250 + frame * 44 + i * 2;
                memory[at..at + 2].copy_from_slice(&words(v.as_i64().unwrap() as i32));
            }
        }
        let expected = case["frames"].as_array().unwrap();
        let mut done = 0;
        let mut passed = false;
        let mut started = false;
        for step in 1..=2000000 {
            let result = cpu
                .step()
                .map_err(|e| format!("pose case {index}: {e:?}"))?;
            let sp = cpu.registers()[13];
            if started || sp != 0 {
                started = true;
                minimum_stack = minimum_stack.min(sp);
                assert!(sp >= 0x7000, "pose stack guard");
            }
            let memory = cpu.physical_memory_slice();
            if usize::from(memory[0xff]) > done {
                assert_eq!(usize::from(memory[0xff]), done + 1);
                let frame = expected[done].as_array().unwrap();
                assert_eq!(frame.len(), 210);
                for (word, v) in frame.iter().enumerate() {
                    let actual = memory[0x800 + word];
                    let want = v.as_u64().unwrap() as u16;
                    if actual != want {
                        return Err(format!(
                            "pose case {index} {} call {done} word {word}: {actual} != {want}",
                            case["tag"]
                        )
                        .into());
                    }
                    checked += 1;
                }
                for (word, v) in config.iter().enumerate() {
                    assert_eq!(
                        memory[0x200 + word],
                        v.as_u64().unwrap() as u16,
                        "pose immutable config {word}"
                    );
                }
                assert!(memory[0x1d0..0x200].iter().all(|v| *v == 0x79ab));
                assert!(memory[0x236..0x240].iter().all(|v| *v == 0x79ab));
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
            return Err(format!("pose case {index}: instruction bound").into());
        }
    }
    let report = json!({"cases":cases.len(),"calls":calls,"checked_u16_values":checked,"maximum_integer_error":0,"instruction_steps":instructions,"code_bytes":code.total_code_words()*2,"maximum_stack_bytes":(0x8000-usize::from(minimum_stack))*2,"minimum_stack_word":minimum_stack,"pose_state_bytes":416,"config_bytes":108,"environment_bytes":72,"travel_bytes":16,"scope":data["scope"],"backend":"instruction, original trig assets without GPU competition"});
    fs::write(
        output.join("vehicle-pose-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
