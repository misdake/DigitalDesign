//! Train/Pickup cold state against actual original resource constructors.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("scene-actor-initial.json"))?)?;
    let code = compile(
        "scene-actor-initial",
        r#"
fn ptr(a:u16)->Array<u16>{unsafe{Ptr::from_addr(a).as_u16_array()}}
fn copy(mut to:Array<u16>,from:Array<u16>,count:u16){let mut i=0u16;while i<count{to[i]=from[i];i+=1;}}
fn main(){
 let saved=read_dseg_page(2);let saved3=read_dseg_page(3);
 unsafe{write_dseg_page(2,138);write_dseg_page(3,139);}
 let sine:Buf<u16,4>=Buf::new([45056,320,40960,0]);
 if ptr(0x1ffe)[0u16]==0 {
  copy(ptr(0x1038),ptr(0x2000),24);copy(ptr(0x1050),ptr(0x2012),6);copy(ptr(0x1056),ptr(0x2024),4);
  let state=train::State{travel:ptr(0x1000),path:ptr(0x1026),position:ptr(0x104a),basis:ptr(0x1038),
    speed:attitude::get(ptr(0x2024),0),range:attitude::get(ptr(0x2026),0),sound:false,seed:ptr(0x2100),sine:unsafe{sine.as_array().as_ptr().as_i16_array()}};
  train::initialize(&state,ptr(0x2018),ptr(0x201e),ptr(0x2028)[0u16]!=0);
 } else {
  let tilts:Buf<u16,4>=Buf::new([45440,1,64,0]);
  let count=ptr(0x1ffc)[0u16];if count>128{halt(0x7e9e);}
  let mut i=0u16;while i<count{
   scene_actor_initial::pickup(ptr(0xc000+i*112),ptr(0x8000+i*54),ptr(0x2000),unsafe{sine.as_array().as_ptr().as_i16_array()},unsafe{tilts.as_array().as_ptr().as_i16_array()});
   copy(ptr(0xc064+i*112),ptr(0x2000),12);i+=1;
  }
 }
 if read_dseg_page(2)!=138 || read_dseg_page(3)!=139{halt(0x7e98);}
 unsafe{write_dseg_page(2,saved);write_dseg_page(3,saved3);}halt(0x600d);
}
"#,
        output,
    )?;
    let sine = fs::read(fixture.join("sine.i16.bin"))?;
    let tilts = fs::read(fixture.join("pickup-tilt.u16.bin"))?;
    assert_eq!(tilts.len(), 64);
    let mut checked = 0usize;
    let mut calls = 0usize;
    let mut instructions = 0u64;
    let mut minimum = u16::MAX;
    let mut cases = Vec::new();
    for train in data["trains"].as_array().unwrap() {
        cases.push((
            false,
            train["input"].as_array().unwrap().clone(),
            Vec::new(),
            train["expected"].as_array().unwrap().clone(),
        ));
    }
    for group in data["pickups"].as_array().unwrap() {
        let mut inputs = Vec::new();
        let mut expected = Vec::new();
        for row in group["rows"].as_array().unwrap() {
            assert_eq!(row["input"].as_array().unwrap().len(), 54);
            assert_eq!(row["expected"].as_array().unwrap().len(), 112);
            inputs.extend(row["input"].as_array().unwrap().clone());
            expected.extend(row["expected"].as_array().unwrap().clone());
        }
        cases.push((
            true,
            inputs,
            group["initial"].as_array().unwrap().clone(),
            expected,
        ));
    }
    for (index, (pickup, input, setup, expected)) in cases.iter().enumerate() {
        let input: Vec<u16> = input.iter().map(|v| v.as_u64().unwrap() as u16).collect();
        let expected: Vec<u16> = expected
            .iter()
            .map(|v| v.as_u64().unwrap() as u16)
            .collect();
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        let memory = cpu.physical_memory_mut();
        memory[0x1ffe] = u16::from(*pickup);
        memory[0x2100..0x2102].copy_from_slice(&[0xbeef, 0xdead]);
        memory[138 * 16384..139 * 16384].fill(0xa53a);
        memory[139 * 16384..140 * 16384].fill(0x5a3d);
        if *pickup {
            assert_eq!(input.len() % 54, 0);
            assert_eq!(setup.len(), 12);
            assert!(input.len() < 16384 && expected.len() < 16384);
            memory[0x1ffc] = (input.len() / 54) as u16;
            memory[138 * 16384..138 * 16384 + input.len()].copy_from_slice(&input);
            for (i, value) in setup.iter().enumerate() {
                memory[0x2000 + i] = value.as_u64().unwrap() as u16;
            }
            calls += input.len() / 54;
        } else {
            assert_eq!(input.len(), 41);
            assert_eq!(expected.len(), 90);
            memory[0x2000..0x2029].copy_from_slice(&input);
            memory[0x1000..0x105b].fill(0x5a3d);
            calls += 1;
        }
        for (i, pair) in sine.as_chunks::<2>().0.iter().enumerate() {
            memory[45056 * 64 + i] = u16::from_le_bytes(*pair);
        }
        for (i, pair) in tilts.as_chunks::<2>().0.iter().enumerate() {
            memory[45440 * 64 + i] = u16::from_le_bytes(*pair);
        }
        let mut started = false;
        let mut passed = false;
        for step in 0..3_000_000u64 {
            let sp = cpu.registers()[13];
            if started || sp != 0 {
                started = true;
                minimum = minimum.min(sp);
                assert!(sp >= 0x7000);
            }
            if let StepOutcome::Halted { signal } = cpu.step().map_err(|e| format!("{e:?}"))? {
                assert_eq!(signal, PASS, "actor init {index}");
                passed = true;
                instructions += step;
                break;
            }
        }
        assert!(passed, "actor init {index}: instruction bound");
        let memory = cpu.physical_memory_mut();
        let base = if *pickup { 139 * 16384 } else { 0x1000 };
        for (word, &value) in expected.iter().enumerate() {
            assert_eq!(memory[base + word], value, "actor init {index} word {word}");
        }
        assert_eq!(&memory[0x2100..0x2102], &[0xbeef, 0xdead]);
        if *pickup {
            assert_eq!(
                &memory[138 * 16384..138 * 16384 + input.len()],
                input.as_slice()
            );
            assert!(
                memory[138 * 16384 + input.len()..139 * 16384]
                    .iter()
                    .all(|&v| v == 0xa53a)
            );
            assert!(
                memory[139 * 16384 + expected.len()..140 * 16384]
                    .iter()
                    .all(|&v| v == 0x5a3d)
            );
        } else {
            assert_eq!(&memory[0x2000..0x2029], input.as_slice());
            assert_eq!(memory[0x105a], 0x5a3d);
            assert!(
                memory[138 * 16384..139 * 16384]
                    .iter()
                    .all(|&v| v == 0xa53a)
            );
            assert!(
                memory[139 * 16384..140 * 16384]
                    .iter()
                    .all(|&v| v == 0x5a3d)
            );
        }
        checked += expected.len();
    }
    let report = json!({"cases":cases.len(),"constructor_calls":calls,"checked_u16_values":checked,"maximum_integer_error":0,
        "code_bytes":code.total_code_words()*2,"maximum_stack_bytes":(32768-usize::from(minimum))*2,"instruction_steps":instructions,
        "backend":"instruction, bounded independent input/output banks and sequential shared pickup setup", "scope":data["scope"]});
    fs::write(
        output.join("scene-actor-initial-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
