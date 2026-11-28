//! Mine kernel branches checked after every call, including error prefixes.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("mine.json"))?)?;
    let code = compile(
        "mine",
        r#"
fn at(v:Array<u16>,w:u16)->Array<u16>{unsafe{v.as_ptr().add(w as i16).as_u16_array()}}
fn g(v:Array<u16>,i:u16)->fix32{attitude::get(v,i)}
fn p(v:Array<u16>,i:u16,x:fix32){attitude::put(v,i,x);}
fn copy(mut to:Array<u16>,from:Array<u16>,count:u16){let mut i=0u16;while i<count{to[i]=from[i];i+=1;}}
fn main(){
 let input=unsafe{Ptr::from_addr(0x0100).as_u16_array()};let mut out=unsafe{Ptr::from_addr(0x1800).as_u16_array()};
 let cells=static_assets::AssetBytes{first_sector:16384,sector_count:16384,byte_len_lo:0,byte_len_hi:32};
 let directory:Buf<u16,64>=Buf::new([0;64]);let mut indices=directory.as_array();let mut i=0u16;while i<64{indices[i]=i;i+=1;}
 let mut map=ground::World::begin(cells,fix32::zero(),indices);
 let sine:Buf<u16,4>=Buf::new([45056,320,40960,0]);let angle:Buf<u16,4>=Buf::new([45376,5,514,0]);
 let tables=projectile::Tables{sine:unsafe{sine.as_array().as_ptr().as_i16_array()},angle:unsafe{angle.as_array().as_ptr().as_i16_array()}};
 let state=mine::State{core:input};let pose=projectile::Pose{basis:at(input,18),position:at(input,36)};
 let car=mine::Vehicle{base_position:at(input,42),velocity:at(input,48),angular:at(input,54),external:at(input,60),has_external:input[78u16]!=0,health:at(input,79),collision_size:at(input,81)};
 let count=input[83u16];let mut frame=0u16;
 while frame<count{
  let c=at(input,84+frame*12);let op=g(c,0).lo_bits();let mut status=fix32::zero();let mut action=0u16;
  let saved=read_dseg_page(3);
  if op==0{action=mine::tick(&state,g(c,1));}
  else if op==1{if mine::contact(&state,g(c,2)){action=1;}}
  else if op==2{status=mine::align(&pose,&mut map,&tables);}
  else if op==3{status=mine::point_hit(&car,at(c,6));}
  if saved!=read_dseg_page(3){halt(0xdead);}
  p(out,0,status);out[2u16]=action;copy(at(out,3),input,83);
  unsafe{Ptr::from_addr(0xff).write(0,frame+1);}frame+=1;
 }
 map.terrain.finish();halt(0x600d);
}
"#,
        output,
    )?;
    let cells = fs::read(fixture.join("projectile-cells.bin"))?;
    let sine = fs::read(fixture.join("sine.i16.bin"))?;
    let angle = fs::read(fixture.join("angle.i16.bin"))?;
    let cases = data["cases"].as_array().ok_or("mine cases")?;
    let mut checked = 0;
    let mut calls = 0;
    let mut steps = 0u64;
    let mut minimum_stack = u16::MAX;
    for (index, case) in cases.iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        for (base, bytes) in [
            (0x100000usize, &cells),
            (0x2c0000, &sine),
            (45376 * 64, &angle),
        ] {
            for (i, pair) in bytes.chunks_exact(2).enumerate() {
                cpu.physical_memory_mut()[base + i] = u16::from_le_bytes([pair[0], pair[1]]);
            }
        }
        let input = case["input"].as_array().ok_or("mine input")?;
        assert_eq!(input.len(), 83);
        for (i, v) in input.iter().enumerate() {
            cpu.physical_memory_mut()[0x100 + i] = v.as_u64().unwrap() as u16;
        }
        let controls = case["controls"].as_array().ok_or("mine controls")?;
        cpu.physical_memory_mut()[0x100 + 83] = controls.len() as u16;
        for (step, c) in controls.iter().enumerate() {
            for (i, v) in c.as_array().unwrap().iter().enumerate() {
                let at = 0x100 + 84 + step * 12 + i * 2;
                cpu.physical_memory_mut()[at..at + 2]
                    .copy_from_slice(&words(v.as_i64().unwrap() as i32));
            }
        }
        let expected = case["frames"].as_array().unwrap();
        let mut done = 0;
        let mut passed = false;
        let mut started = false;
        for step in 1..=10000000 {
            let result = cpu
                .step()
                .map_err(|e| format!("mine case {index}: {e:?}"))?;
            let sp = cpu.registers()[13];
            if started || sp != 0 {
                started = true;
                minimum_stack = minimum_stack.min(sp);
            }
            let memory = cpu.physical_memory_slice();
            if usize::from(memory[0xff]) > done {
                assert_eq!(usize::from(memory[0xff]), done + 1);
                for (field, v) in expected[done].as_array().unwrap().iter().enumerate() {
                    let actual = memory[0x1800 + field];
                    let want = v.as_u64().unwrap() as u16;
                    if actual != want {
                        return Err(format!(
                            "mine case {index} {} call {done} word {field}: {actual} != {want}",
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
                steps += step;
                passed = true;
                break;
            }
        }
        if !passed {
            return Err(format!("mine case {index}: instruction bound").into());
        }
    }
    let report = json!({"cases":cases.len(),"calls":calls,"checked_u16_values":checked,"maximum_integer_error":0,
        "instruction_steps":steps,"code_bytes":code.total_code_words()*2,"minimum_stack_word":minimum_stack,
        "mine_record_bytes":36,"scope":data["scope"],"backend":"instruction; functional terrain memory, without GPU competition"});
    fs::write(
        output.join("mine-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
