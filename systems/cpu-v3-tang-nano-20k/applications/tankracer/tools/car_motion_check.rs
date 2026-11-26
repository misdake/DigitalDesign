//! Complete motion state and mesh after each original integrate/finish call.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("car-motion.json"))?)?;
    let code = compile(
        "car-motion",
        r#"
fn ptr(a:u16)->Array<u16>{unsafe{Ptr::from_addr(a).as_u16_array()}}
fn at(v:Array<u16>,w:u16)->Array<u16>{unsafe{v.as_ptr().add(w as i16).as_u16_array()}}
fn g(v:Array<u16>,i:u16)->fix32{attitude::get(v,i)}
fn p(v:Array<u16>,i:u16,x:fix32){attitude::put(v,i,x);}
fn main(){
 let previous=read_dseg_page(2);let previous3=read_dseg_page(3);unsafe{write_dseg_page(2,138);write_dseg_page(3,137);}
 let s=ptr(0x8000);
 let state=car_motion::State{poses:vehicle_pose::State{physical_position:s,reference:at(s,6),base_position:at(s,24),body_basis:at(s,30),body_position:at(s,48),tracks:at(s,54),turret:at(s,102),gun:at(s,126),attachments:at(s,150),bottom_position:at(s,198),recoil:at(s,204)},previous:at(s,208),velocity:at(s,214),angular:at(s,220),acceleration:at(s,226),pivot:at(s,230),articulation:at(s,236),lean:at(s,244),contacts:at(s,252),contact_stride:4,contact_gap:3,travel:at(s,284),hull:at(s,292),ready:at(s,329),rates:at(s,331),gun:unsafe{at(s,339).as_ptr().as_i16_array()},gun_binding:ptr(0x2480)};
 let sine:Buf<u16,4>=Buf::new([45056,320,40960,0]);let angle:Buf<u16,4>=Buf::new([45376,5,514,0]);
 let config=car_motion::Config{attitude:ptr(0x2400),body:ptr(0x2420),pose:ptr(0x2440),sine:unsafe{sine.as_array().as_ptr().as_i16_array()},angle:unsafe{angle.as_array().as_ptr().as_i16_array()}};
 let pose_input:Buf<u16,36>=Buf::new([0;36]);let e=pose_input.as_array();
 let count=ptr(0x2300)[0u16];let mut frame=0u16;
 while frame<count{
  let f=ptr(0x3000+frame*42);let mut i=0u16;while i<6{p(e,i,g(f,5+i));i+=1;}
  p(e,6,integer::from_i16(111));p(e,7,integer::from_i16(-222));p(e,8,g(f,11));i=0;while i<9{p(e,9+i,g(f,12+i));i+=1;}
  let dt=g(f,1);let op=g(f,0).lo_bits();let mut status=fix32::zero();
  if op!=1{status=car_motion::integrate_and_pose(&state,&config,&car_motion::Frame{dt:dt,number:g(f,2),player:g(f,3)!=fix32::zero(),mode:g(f,4),pose:vehicle_pose::Environment{timer:g(f,5).lo_bits(),values:e}});}
  if op!=0 && status==fix32::zero(){car_motion::finish(&state,dt);}
  if read_dseg_page(2)!=138 || read_dseg_page(3)!=137{halt(0x7e92);}
  unsafe{Ptr::from_addr(0xfe).write(0,status.lo_bits());Ptr::from_addr(0xfd).write(0,status.hi_bits());Ptr::from_addr(0xff).write(0,frame+1);}frame+=1;
 }
 unsafe{write_dseg_page(2,previous);write_dseg_page(3,previous3);}halt(0x600d);
}
"#,
        output,
    )?;
    let sine = fs::read(fixture.join("sine.i16.bin"))?;
    let angle = fs::read(fixture.join("angle.i16.bin"))?;
    let cases = data["cases"].as_array().unwrap();
    let mut calls = 0;
    let mut checked = 0;
    let mut instructions = 0u64;
    let mut minimum_stack = u16::MAX;
    for (index, case) in cases.iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        let memory = cpu.physical_memory_mut();
        for (base, bytes) in [(45056 * 64, &sine), (45376 * 64, &angle)] {
            for (i, pair) in bytes.chunks_exact(2).enumerate() {
                memory[base + i] = u16::from_le_bytes([pair[0], pair[1]]);
            }
        }
        let state = case["state"].as_array().unwrap();
        assert_eq!(state.len(), 357);
        for (i, v) in state.iter().enumerate() {
            memory[138 * 16384 + i] = v.as_u64().unwrap() as u16;
        }
        memory[138 * 16384 + 357..139 * 16384].fill(0x79ab);
        for (name, base, len) in [
            ("attitude", 0x2400, 5),
            ("body", 0x2420, 12),
            ("pose", 0x2440, 54),
            ("binding", 0x2480, 10),
        ] {
            let values = case[name].as_array().unwrap();
            assert_eq!(values.len(), len);
            for (i, v) in values.iter().enumerate() {
                memory[base + i] = v.as_i64().unwrap() as u16;
            }
        }
        let controls = case["controls"].as_array().unwrap();
        assert!(controls.len() <= 32);
        memory[0x2300] = controls.len() as u16;
        for (frame, c) in controls.iter().enumerate() {
            assert_eq!(c.as_array().unwrap().len(), 21);
            for (i, v) in c.as_array().unwrap().iter().enumerate() {
                let at = 0x3000 + frame * 42 + i * 2;
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
                .map_err(|e| format!("car-motion case {index}: {e:?}"))?;
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
                assert_eq!(expected.len(), 359);
                for (word, v) in expected.iter().enumerate() {
                    let at = if word == 0 {
                        0xfe
                    } else if word == 1 {
                        0xfd
                    } else {
                        138 * 16384 + word - 2
                    };
                    let actual = memory[at];
                    let want = v.as_u64().unwrap() as u16;
                    if actual != want {
                        return Err(format!("car-motion case {index} {} call {done} word {word}: {actual} != {want}",case["tag"]).into());
                    }
                    checked += 1;
                }
                assert!(
                    memory[138 * 16384 + 357..139 * 16384]
                        .iter()
                        .all(|v| *v == 0x79ab)
                );
                for (name, base) in [
                    ("attitude", 0x2400),
                    ("body", 0x2420),
                    ("pose", 0x2440),
                    ("binding", 0x2480),
                ] {
                    for (i, v) in case[name].as_array().unwrap().iter().enumerate() {
                        assert_eq!(memory[base + i], v.as_i64().unwrap() as u16);
                    }
                }
                for (frame, c) in controls.iter().enumerate() {
                    for (i, v) in c.as_array().unwrap().iter().enumerate() {
                        let at = 0x3000 + frame * 42 + i * 2;
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
            return Err(format!("car-motion case {index}: instruction bound").into());
        }
    }
    let report = json!({"cases":cases.len(),"calls":calls,"checked_u16_values":checked,"maximum_integer_error":0,"instruction_steps":instructions,"code_bytes":code.total_code_words()*2,"minimum_stack_word":minimum_stack,"maximum_stack_bytes":(0x8000-usize::from(minimum_stack))*2,"motion_state_bytes":678,"mesh_bytes":36,"scope":data["scope"],"backend":"instruction, banked state and original trig assets without GPU competition"});
    fs::write(
        output.join("car-motion-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
