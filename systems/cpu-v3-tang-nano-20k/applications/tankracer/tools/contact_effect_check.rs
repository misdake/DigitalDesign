//! Check synchronous effect writes and partial sampling state on both sides of an error.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("contact-effect.json"))?)?;
    let code = compile(
        "contact-effect",
        r#"
fn ptr(a:u16)->Array<u16>{unsafe{Ptr::from_addr(a).as_u16_array()}}
fn at(a:Array<u16>,w:u16)->Array<u16>{unsafe{a.as_ptr().add(w as i16).as_u16_array()}}
fn g(a:Array<u16>,i:u16)->fix32{attitude::get(a,i)}
fn p(a:Array<u16>,i:u16,v:fix32){attitude::put(a,i,v);}
fn copy(mut to:Array<u16>,from:Array<u16>,n:u16){let mut i=0u16;while i<n{to[i]=from[i];i+=1;}}
fn main(){
 let previous=read_dseg_page(3);
 let mut world=ground::World::begin(static_assets::AssetBytes{first_sector:8192,sector_count:512,byte_len_lo:0,byte_len_hi:1},fix32::zero(),ptr(0x2600));
 world.seed=g(ptr(0x2800),103);let mut i=0u16;while i<16{world.noise[i]=128;i+=1;}
 world.shield=ptr(0xc000);world.shield_page=ptr(0x2300)[0u16];
 let mut state=ground::zero();let raw=unsafe{view_of(&mut state).as_ptr().as_u16_array()};copy(raw,ptr(0x2800),206);
 let records:Buf<deformation::Record,1>=Buf::new([deformation::record_zero()]);
 let mut surface=deformation::disabled(ptr(0x2400),records.as_array());
 let count=ptr(0x2300)[1u16];let mut tick=0u16;
 while tick<count{
  let env:Buf<u16,20>=Buf::new([0;20]);let e=env.as_array();copy(e,ptr(0x3000+tick*8),8);copy(at(e,8),ptr(0x2500),12);
  let status=ground::sample(&mut state,&mut world,ptr(0x2480),ptr(0x2400),e,&mut surface);
  let page=read_dseg_page(3);let out=ptr(0x4000);copy(out,raw,206);p(out,103,world.seed);
  unsafe{write_dseg_page(3,137);}p(out,104,g(ptr(0xc000),44));p(out,105,g(ptr(0xc000),89));unsafe{write_dseg_page(3,page);}
  unsafe{Ptr::from_addr(0xfe).write(0,status);Ptr::from_addr(0xfd).write(0,page);Ptr::from_addr(0xff).write(0,tick+1);}tick+=1;
 }
 unsafe{write_dseg_page(3,previous);}halt(0x600d);
}
"#,
        output,
    )?;
    let mut calls = 0;
    let mut checked = 0;
    let mut instructions = 0;
    let mut minimum = u16::MAX;
    for (index, case) in data["cases"].as_array().unwrap().iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        let memory = cpu.physical_memory_mut();
        let cells = fs::read(fixture.join(format!(
            "contact-effect-cells-{}.bin",
            case["variant"].as_u64().unwrap()
        )))?;
        for (i, p) in cells.chunks_exact(2).enumerate() {
            memory[8192 * 64 + i] = u16::from_le_bytes([p[0], p[1]]);
        }
        memory[137 * 16384..138 * 16384].fill(0x79ab);
        let initial = case["input"].as_array().unwrap();
        assert_eq!(initial.len(), 212);
        for (i, v) in initial.iter().enumerate() {
            memory[0x2800 + i] = v.as_u64().unwrap() as u16;
        }
        memory[137 * 16384 + 88..137 * 16384 + 90].copy_from_slice(&[
            initial[208].as_u64().unwrap() as u16,
            initial[209].as_u64().unwrap() as u16,
        ]);
        memory[137 * 16384 + 178..137 * 16384 + 180].copy_from_slice(&[
            initial[210].as_u64().unwrap() as u16,
            initial[211].as_u64().unwrap() as u16,
        ]);
        for (i, v) in case["tracks"].as_array().unwrap().iter().enumerate() {
            memory[0x2480 + i * 2..0x2482 + i * 2]
                .copy_from_slice(&words(v.as_i64().unwrap() as i32));
        }
        for (i, v) in case["auxiliary"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|v| v.as_array().unwrap())
            .enumerate()
        {
            memory[0x2500 + i * 2..0x2502 + i * 2]
                .copy_from_slice(&words(v.as_i64().unwrap() as i32));
        }
        memory[0x2600..0x2640].fill(65535);
        memory[0x261c] = 0;
        memory[0x261d] = 1;
        let controls = case["controls"].as_array().unwrap();
        memory[0x2300] = if case["enabled"].as_bool().unwrap() {
            137
        } else {
            65535
        };
        memory[0x2301] = controls.len() as u16;
        for (tick, f) in controls.iter().enumerate() {
            for (i, v) in f.as_array().unwrap().iter().enumerate() {
                let at = 0x3000 + tick * 8 + i * 2;
                memory[at..at + 2].copy_from_slice(&words(v.as_i64().unwrap() as i32));
            }
        }
        let immutable = memory[0x2400..0x3000].to_vec();
        let mut done = 0;
        let mut passed = false;
        let mut started = false;
        for step in 1..=1_000_000 {
            let result = cpu
                .step()
                .map_err(|e| format!("contact-effect {index}: {e:?}"))?;
            let sp = cpu.registers()[13];
            if started || sp != 0 {
                started = true;
                minimum = minimum.min(sp);
                assert!(sp >= 0x6000);
            }
            let memory = cpu.physical_memory_slice();
            if usize::from(memory[0xff]) > done {
                let f = case["frames"][done].as_array().unwrap();
                assert_eq!(f.len(), 214);
                for (word, v) in f.iter().enumerate() {
                    let at = if word == 0 {
                        0xfe
                    } else if word == 1 {
                        0xfd
                    } else {
                        0x4000 + word - 2
                    };
                    let want = v.as_u64().unwrap() as u16;
                    if memory[at] != want {
                        return Err(format!(
                            "contact-effect case {index} tick {done} word {word}: {} != {want}",
                            memory[at]
                        )
                        .into());
                    }
                    checked += 1;
                }
                assert_eq!(&memory[0x2400..0x3000], &immutable);
                for (i, v) in memory[137 * 16384..138 * 16384].iter().enumerate() {
                    if !(88..90).contains(&i) && !(178..180).contains(&i) {
                        assert_eq!(*v, 0x79ab);
                    }
                }
                done += 1;
                calls += 1;
            }
            if let StepOutcome::Halted { signal } = result {
                assert_eq!(signal, PASS);
                assert_eq!(done, controls.len());
                instructions += step;
                passed = true;
                break;
            }
        }
        assert!(passed, "contact-effect {index} instruction bound");
    }
    let report = json!({"cases":data["cases"].as_array().unwrap().len(),"calls":calls,"checked_u16_values":checked,"maximum_integer_error":0,"code_bytes":code.total_code_words()*2,"maximum_stack_bytes":(32768-usize::from(minimum))*2,"instruction_steps":instructions,"scope":data["scope"],"backend":"instruction, diagnostic terrain planes and guarded external effect bank"});
    fs::write(
        output.join("contact-effect-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
