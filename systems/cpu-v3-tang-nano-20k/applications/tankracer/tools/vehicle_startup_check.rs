//! Complete fresh-player rows from immutable model metadata and spawn input.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("vehicle-startup.json"))?)?;
    let code = compile(
        "vehicle-startup",
        r#"
fn ptr(a:u16)->Array<u16>{unsafe{Ptr::from_addr(a).as_u16_array()}}
fn main(){
 let saved=read_dseg_page(2);let saved3=read_dseg_page(3);
 unsafe{write_dseg_page(2,138);write_dseg_page(3,137);}
 let sine:Buf<u16,4>=Buf::new([45056,320,40960,0]);let angle:Buf<u16,4>=Buf::new([45376,5,514,0]);
 let model=vehicle_startup::Model{pose:ptr(0xc000),contacts:ptr(0xc040),shadow:unsafe{ptr(0xc060).as_ptr().as_i16_array()},shadow_vertices:ptr(0x2300)[1u16],shadow_radius:ptr(0x2300)[2u16],control:ptr(0xc080),sine:unsafe{sine.as_array().as_ptr().as_i16_array()},angle:unsafe{angle.as_array().as_ptr().as_i16_array()}};
 let mut row=ptr(ptr(0x2300)[0u16]);let mut call=0u16;
 while call<2 {
  let mut pattern=0xa5a5u16;if call!=0{pattern=0x5a5a;}
  let mut i=0u16;while i<vehicle_startup::WORDS{row[i]=pattern;i+=1;}
  let status=vehicle_startup::construct(row,&model,ptr(0x2800),ptr(0x2840));
  if read_dseg_page(2)!=138 || read_dseg_page(3)!=137{halt(0x7e94);}
  unsafe{Ptr::from_addr(0xfe).write(0,status.lo_bits());Ptr::from_addr(0xfd).write(0,status.hi_bits());Ptr::from_addr(0xff).write(0,call+1);}call+=1;
 }
 unsafe{write_dseg_page(2,saved);write_dseg_page(3,saved3);}halt(0x600d);
}
"#,
        output,
    )?;
    let sine = fs::read(fixture.join("sine.i16.bin"))?;
    let angle = fs::read(fixture.join("angle.i16.bin"))?;
    let cases = data["cases"].as_array().unwrap();
    let mut checked = 0;
    let mut instructions = 0u64;
    let mut minimum = u16::MAX;
    for (index, case) in cases.iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        let memory = cpu.physical_memory_mut();
        for (base, bytes) in [(45056 * 64, &sine), (45376 * 64, &angle)] {
            for (i, pair) in bytes.chunks_exact(2).enumerate() {
                memory[base + i] = u16::from_le_bytes([pair[0], pair[1]]);
            }
        }
        let offset = 16 + (index % 9) * 1760;
        let row = 138 * 16384 + offset;
        memory[138 * 16384..139 * 16384].fill(0x79ab);
        memory[137 * 16384..138 * 16384].fill(0xb63c);
        memory[0x2300] = (0x8000 + offset) as u16;
        memory[0x2301] = case["shadow_count"].as_u64().unwrap() as u16;
        memory[0x2302] = case["shadow_radius"].as_u64().unwrap() as u16;
        for (name, at) in [
            ("pose", 137 * 16384),
            ("contacts", 137 * 16384 + 64),
            ("shadow", 137 * 16384 + 96),
            ("control", 137 * 16384 + 128),
            ("spawn", 0x2800),
            ("input", 0x2840),
        ] {
            let values = case[name].as_array().unwrap();
            if name == "control" || name == "input" {
                for (i, v) in values.iter().enumerate() {
                    memory[at + i * 2..at + i * 2 + 2]
                        .copy_from_slice(&words(v.as_i64().unwrap() as i32));
                }
            } else {
                for (i, v) in values.iter().enumerate() {
                    memory[at + i] = v.as_u64().unwrap() as u16;
                }
            }
        }
        let immutable = memory[137 * 16384..138 * 16384].to_vec();
        let spawn = memory[0x2800..0x2852].to_vec();
        let mut calls = 0usize;
        let mut passed = false;
        let mut started = false;
        for step in 1..=1000000 {
            let result = cpu
                .step()
                .map_err(|e| format!("vehicle-startup case {index}: {e:?}"))?;
            let sp = cpu.registers()[13];
            if started || sp != 0 {
                started = true;
                minimum = minimum.min(sp);
                assert!(sp >= 0x7000);
            }
            let memory = cpu.physical_memory_slice();
            if usize::from(memory[0xff]) > calls {
                assert_eq!(usize::from(memory[0xff]), calls + 1);
                let status = i32::from(memory[0xfe]) | (i32::from(memory[0xfd]) << 16);
                assert_eq!(
                    i64::from(status),
                    case["status"].as_i64().unwrap(),
                    "vehicle-startup case {index} {} status",
                    case["tag"]
                );
                if status == 0 {
                    let expected = case["state"].as_array().unwrap();
                    assert_eq!(expected.len(), 1760);
                    for (i, want) in expected.iter().enumerate() {
                        let actual = memory[row + i];
                        let want = want.as_u64().unwrap() as u16;
                        if actual != want {
                            return Err(format!("vehicle-startup case {index} {} call {calls} word {i}: {actual} != {want}",case["tag"]).into());
                        }
                        checked += 1;
                    }
                } else if status == -4 {
                    assert!(
                        memory[row..row + 1760]
                            .iter()
                            .all(|w| *w == if calls == 0 { 0xa5a5 } else { 0x5a5a })
                    );
                }
                assert_eq!(&memory[137 * 16384..138 * 16384], &immutable);
                assert_eq!(&memory[0x2800..0x2852], &spawn);
                assert!(
                    memory[138 * 16384..row]
                        .iter()
                        .chain(&memory[row + 1760..139 * 16384])
                        .all(|w| *w == 0x79ab)
                );
                calls += 1;
            }
            if let StepOutcome::Halted { signal } = result {
                assert_eq!(signal, PASS);
                assert_eq!(calls, 2);
                instructions += step;
                passed = true;
                break;
            }
        }
        assert!(passed, "vehicle-startup case {index}: instruction bound");
    }
    let report = json!({"cases":cases.len(),"calls":cases.len()*2,"checked_u16_values":checked,"maximum_integer_error":0,"code_bytes":code.total_code_words()*2,"minimum_stack_word":minimum,"maximum_stack_bytes":(32768-usize::from(minimum))*2,"instruction_steps":instructions,"vehicle_row_bytes":3520,"nine_vehicle_rows_bytes":31680,"scope":data["scope"],"backend":"instruction, disjoint model/state banks and immutable original trig assets"});
    fs::write(
        output.join("vehicle-startup-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
