//! Original pickup trajectories and inventory checked after each RCC call.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("powerup.json"))?)?;
    let code = compile(
        "powerup",
        r#"
fn at(data:Array<u16>,offset:u16)->Array<u16>{unsafe{data.as_ptr().add(offset as i16).as_u16_array()}}
fn g(data:Array<u16>,index:u16)->fix32{attitude::get(data,index)}
fn p(data:Array<u16>,index:u16,value:fix32){attitude::put(data,index,value);}
fn copy(mut to:Array<u16>,from:Array<u16>,count:u16){let mut i=0u16;while i<count{to[i]=from[i];i+=1;}}
fn main(){
 let input=unsafe{Ptr::from_addr(0x0100).as_u16_array()};
 let out=unsafe{Ptr::from_addr(0x1800).as_u16_array()};
 let count=g(input,1).lo_bits();let mut frame=0u16;
 if g(input,0)==fix32::zero(){
  let cells=static_assets::AssetBytes{first_sector:16384,sector_count:16384,byte_len_lo:0,byte_len_hi:32};
  let directory:Buf<u16,64>=Buf::new([0;64]);let mut indices=directory.as_array();let mut i=0u16;
  while i<64{indices[i]=i;i+=1;}
  let mut map=ground::World::begin(cells,fix32::zero(),indices);
  let sine:Buf<u16,4>=Buf::new([45056,320,40960,0]);let angle:Buf<u16,4>=Buf::new([45376,5,514,0]);
  let state=powerup::State{core:at(input,4),parachute:at(input,32)};
  let pose=powerup::Pose{basis:at(input,56),position:at(input,74)};let flags=at(input,80);
  while frame<count {
   let c=at(input,82+frame*28);let operation=g(c,0);let mut status=fix32::zero();
   if operation==integer::from_i16(1){powerup::finish_frame(&state,flags,g(c,1));}
   else {
    let step=powerup::Frame{dt:g(c,1),number:g(c,2),entity:g(c,3),player:at(c,8),camera:at(c,14),visibility:g(c,10).lo_bits(),auxiliary_active:g(c,11)!=fix32::zero(),separation:g(c,12),diameter:g(c,13),sine:unsafe{sine.as_array().as_ptr().as_i16_array()},angle:unsafe{angle.as_array().as_ptr().as_i16_array()}};
    status=powerup::phase(&state,&pose,flags,&step,&mut map);
    if status>=fix32::zero() && operation==integer::from_i16(2){powerup::finish_frame(&state,flags,g(c,1));}
   }
   p(out,0,status);copy(at(out,2),at(input,4),78);
   if g(state.core,10)==fix32::zero(){let mut i=0u16;while i<12{p(out,15+i,fix32::zero());i+=1;}}
   unsafe{Ptr::from_addr(0xff).write(0,frame+1);}frame+=1;
  }
  map.terrain.finish();
 }else{
  let amounts:Buf<u16,11>=Buf::new([0;11]);let mut table=amounts.as_array();let mut i=0u16;
  while i<11{table[i]=g(input,5+i).lo_bits();i+=1;}
  let state=at(input,4);let cooldown=at(input,8);
  while frame<count{
   let c=at(input,32+frame*4);let status=inventory::collect(state,g(c,0),table);
   if status==fix32::zero() && g(c,1)!=fix32::zero(){inventory::collected(state,cooldown);}
   p(out,0,status);p(out,1,g(state,0));p(out,2,fix32::from_words(state[2u16],0));p(out,3,g(cooldown,0));
   unsafe{Ptr::from_addr(0xff).write(0,frame+1);}frame+=1;
  }
 }
 halt(0x600d);
}
"#,
        output,
    )?;
    let sine = fs::read(fixture.join("sine.i16.bin"))?;
    let angle = fs::read(fixture.join("angle.i16.bin"))?;
    let cells = fs::read(fixture.join("powerup-cells.bin"))?;
    let cases = data["cases"].as_array().ok_or("powerup cases")?;
    let mut checked = 0;
    let mut frames = 0;
    let mut steps = 0u64;
    for (index, case) in cases.iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        let input = case["input"].as_array().ok_or("powerup input")?;
        assert!(input.len() * 2 < 0x1700);
        if input[0].as_i64() == Some(0) {
            for (base, bytes) in [
                (0x100000usize, &cells),
                (0x2c0000, &sine),
                (45376 * 64, &angle),
            ] {
                for (i, pair) in bytes.chunks_exact(2).enumerate() {
                    cpu.physical_memory_mut()[base + i] = u16::from_le_bytes([pair[0], pair[1]]);
                }
            }
        }
        for (i, v) in input.iter().enumerate() {
            cpu.physical_memory_mut()[0x100 + i * 2..0x102 + i * 2]
                .copy_from_slice(&words(v.as_i64().unwrap() as i32));
        }
        let expected = case["frames"].as_array().ok_or("powerup frames")?;
        let mut done = 0;
        let mut passed = false;
        for step in 1..=10000000 {
            let result = cpu
                .step()
                .map_err(|e| format!("powerup case {index}: {e:?}"))?;
            let memory = cpu.physical_memory_slice();
            if usize::from(memory[0xff]) > done {
                assert_eq!(usize::from(memory[0xff]), done + 1);
                for (field, v) in expected[done]
                    .as_array()
                    .ok_or("powerup answer")?
                    .iter()
                    .enumerate()
                {
                    let at = 0x1800 + field * 2;
                    let actual = i32::from(memory[at]) | (i32::from(memory[at + 1]) << 16);
                    if i64::from(actual) != v.as_i64().unwrap() {
                        return Err(format!(
                            "powerup case {index} {} frame {done} field {field}: {actual} != {v}",
                            case["tag"]
                        )
                        .into());
                    }
                    checked += 1;
                }
                done += 1;
                frames += 1;
            }
            if let StepOutcome::Halted { signal } = result {
                if signal != PASS {
                    return Err(format!("powerup case {index}: halt {signal:04x}").into());
                }
                assert_eq!(done, expected.len());
                steps += step;
                passed = true;
                break;
            }
        }
        if !passed {
            return Err(format!("powerup case {index}: step limit").into());
        }
    }
    let report = json!({"cases":cases.len(),"frames":frames,"checked_i32_values":checked,"maximum_integer_error":0,
        "code_bytes":code.total_code_words()*2,"instruction_steps":steps,"scope":data["scope"],"authored":data["authored"],
        "boundary":"Pickup phase/finish and inventory kernels in instruction sim; scene membership, collection effects, native loading and live driving are separate"});
    fs::write(
        output.join("powerup-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
