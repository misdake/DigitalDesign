//! Real checkpoint restoration and complete car state after every source call.
use super::*;
fn values(v: &Value) -> Vec<u16> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u16)
        .collect()
}
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("vehicle-transfer.json"))?)?;
    let code = compile(
        "vehicle-transfer",
        r#"
fn pointer(a:u16)->Array<u16>{unsafe{Ptr::from_addr(a).as_u16_array()}}
fn at(v:Array<u16>,w:u16)->Array<u16>{unsafe{v.as_ptr().add(w as i16).as_u16_array()}}
fn main(){
 let s=pointer(0x100);let meta=pointer(0x380);
 let pose=vehicle_pose::State{physical_position:s,reference:at(s,6),base_position:at(s,24),body_basis:at(s,30),body_position:at(s,48),tracks:at(s,54),turret:at(s,102),gun:at(s,126),attachments:at(s,150),bottom_position:at(s,198),recoil:at(s,204)};
 let car=vehicle_transfer::Car{pose,checkpoint:at(s,208),passed:at(s,212),laps:at(s,214),controls:at(s,216),velocity:at(s,222),acceleration:at(s,228),angular:at(s,232),track_slip:at(s,238),temporary_timer:at(s,240),config:pointer(0x300),travel:pointer(0x370),environment:pointer(0x340)};
 let routes=vehicle_transfer::Routes{order:ordered::List{items:pointer(0x2000),count:meta[4u16],capacity:512},points:pointer(0x2400),count:meta[5u16],occupied:ordered::List{items:pointer(0x2800),count:meta[6u16],capacity:512}};
 let ids=entities::State{row_words:6,header:pointer(0x1000),rows:pointer(0x100c),storage_capacity:512};
 let world=scene_world::World{colliders:pointer(0x8000),properties:pointer(0),count:meta[3u16],fixed_count:meta[3u16],dynamic_page:121,dynamic_capacity:0,grid:spatial::Grid{page:65535,origin:pointer(0),shift_x:0,shift_z:0,items:pointer(0),ranges:pointer(0)},deformables:ordered::List{items:pointer(0),count:0,capacity:0}};
 let sine:Buf<u16,4>=Buf::new([45056,320,40960,0]);let angle:Buf<u16,4>=Buf::new([45376,5,514,0]);
 let request=vehicle_transfer::Request{limit:attitude::get(meta,0),sine:unsafe{sine.as_array().as_ptr().as_i16_array()},angle:unsafe{angle.as_array().as_ptr().as_i16_array()},height:at(s,242)};
 unsafe{write_dseg_page(2,120);write_dseg_page(3,119);}
 let mut frame=0u16;
 while frame<meta[2u16]{
  let status=vehicle_transfer::restore(&car,&routes,&world,&ids,&request);
  if read_dseg_page(2)!=120 || read_dseg_page(3)!=119{halt(0x7e81);}
  let mut out=pointer(0x4000);attitude::put(out,0,status);let mut i=0u16;while i<244{out[2+i]=s[i];i+=1;}
  unsafe{Ptr::from_addr(0xff).write(0,frame+1);}frame+=1;
 }
 halt(0x600d);
}
"#,
        output,
    )?;
    let cases = data["cases"].as_array().unwrap();
    let mut calls = 0;
    let mut checked = 0;
    let mut instructions = 0u64;
    let mut minimum_stack = u16::MAX;
    let sine = fs::read(fixture.join("sine.i16.bin"))?;
    let angle = fs::read(fixture.join("angle.i16.bin"))?;
    for (index, case) in cases.iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        let memory = cpu.physical_memory_mut();
        for (base, bytes) in [(0x2c0000, &sine), (45376 * 64, &angle)] {
            for (i, pair) in bytes.chunks_exact(2).enumerate() {
                memory[base + i] = u16::from_le_bytes([pair[0], pair[1]]);
            }
        }
        for (base, name) in [
            (0x100, "state"),
            (0x300, "config"),
            (0x340, "environment"),
            (0x370, "travel"),
            (0x1000, "identity"),
            (0x2000, "order"),
            (0x2400, "points"),
            (0x2800, "occupied"),
            (120 * 16384, "colliders"),
        ] {
            let v = values(&case[name]);
            let maximum = match name {
                "state" => 244,
                "config" => 54,
                "environment" => 36,
                "travel" => 8,
                "identity" => 12 + 512 * 6,
                "order" | "points" | "occupied" => 1024,
                _ => 16384,
            };
            assert!(v.len() <= maximum, "transfer {name}");
            memory[base..base + v.len()].copy_from_slice(&v);
        }
        let colliders = values(&case["colliders"]);
        assert_eq!(colliders.len() % 60, 0);
        memory[120 * 16384 + colliders.len()..121 * 16384].fill(0x79ab);
        memory[0x380..0x382].copy_from_slice(&words(case["limit"].as_u64().unwrap() as i32));
        memory[0x382] = case["frames"].as_array().unwrap().len() as u16;
        memory[0x383] = (colliders.len() / 60) as u16;
        memory[0x384] = (values(&case["order"]).len() / 2) as u16;
        memory[0x385] = (values(&case["points"]).len() / 4) as u16;
        memory[0x386] = (values(&case["occupied"]).len() / 2) as u16;
        let expected = case["frames"].as_array().unwrap();
        let mut done = 0;
        let mut passed = false;
        let mut started = false;
        for step in 1..=5000000 {
            let result = cpu
                .step()
                .map_err(|e| format!("transfer case {index}: {e:?}"))?;
            let sp = cpu.registers()[13];
            if started || sp != 0 {
                started = true;
                minimum_stack = minimum_stack.min(sp);
                assert!(sp >= 0x7000, "transfer stack guard");
            }
            let memory = cpu.physical_memory_slice();
            if usize::from(memory[0xff]) > done {
                assert_eq!(usize::from(memory[0xff]), done + 1);
                let want = values(&expected[done]);
                assert_eq!(want.len(), 246);
                for (word, &v) in want.iter().enumerate() {
                    let actual = memory[0x4000 + word];
                    if actual != v {
                        return Err(format!(
                            "transfer case {index} {} call {done} word {word}: {actual} != {v}",
                            case["tag"]
                        )
                        .into());
                    }
                    checked += 1;
                }
                for (base, name) in [
                    (0x300, "config"),
                    (0x340, "environment"),
                    (0x370, "travel"),
                    (0x1000, "identity"),
                    (0x2000, "order"),
                    (0x2400, "points"),
                    (0x2800, "occupied"),
                    (120 * 16384, "colliders"),
                ] {
                    let want = values(&case[name]);
                    assert_eq!(
                        &memory[base..base + want.len()],
                        want.as_slice(),
                        "transfer immutable {name}"
                    );
                }
                assert!(
                    memory[120 * 16384 + colliders.len()..121 * 16384]
                        .iter()
                        .all(|v| *v == 0x79ab)
                );
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
            return Err(format!("transfer case {index}: instruction bound").into());
        }
    }
    let report = json!({"cases":cases.len(),"calls":calls,"checked_u16_values":checked,"maximum_integer_error":0,"instruction_steps":instructions,"code_bytes":code.total_code_words()*2,"maximum_stack_bytes":(0x8000-usize::from(minimum_stack))*2,"minimum_stack_word":minimum_stack,"car_snapshot_bytes":488,"scope":data["scope"],"backend":"instruction, live ENGLAND collider/identity/waypoint snapshots without GPU competition"});
    fs::write(
        output.join("vehicle-transfer-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
