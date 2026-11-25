//! Complete prefix calls with immutable original classes and streamed snapshots.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("prefix.json"))?)?;
    let code = compile(
        "prefix",
        r#"
fn at(data:Array<u16>,offset:u16)->Array<u16>{unsafe{data.as_ptr().add(offset as i16).as_u16_array()}}
fn g(data:Array<u16>,index:u16)->fix32{attitude::get(data,index)}
fn copy(mut to:Array<u16>,from:Array<u16>,count:u16){let mut i=0u16;while i<count{to[i]=from[i];i+=1;}}
fn aux(data:Array<u16>,index:u16)->sampling::Auxiliary{
 sampling::Auxiliary{x:g(data,index),y:g(data,index+1),z:g(data,index+2),kind:g(data,index+3),surface:g(data,index+4),cached:g(data,index+5)}
}
fn main(){
 let input=unsafe{Ptr::from_addr(0x2000).as_u16_array()};
 let out=unsafe{Ptr::from_addr(0x1000).as_u16_array()};
 let ids=unsafe{Ptr::from_addr(0x6000).as_u16_array()};
 let identities=entities::State{header:ids,rows:at(ids,12),storage_capacity:g(ids,0).lo_bits()};
 let bytes=g(input,938).lo_bits();
 let asset=static_assets::AssetBytes{first_sector:40960,sector_count:(bytes+127)>>7,byte_len_lo:bytes,byte_len_hi:0};
 let scene=scene_query::Scene::begin(asset);if !scene.valid {halt(0x7f21);}
 let sine:Buf<u16,4>=Buf::new([45056,320,40960,0]);
 let auxiliary:Buf<sampling::Auxiliary,2>=Buf::new([aux(input,29),aux(input,35)]);
 let mut pool=effect_pool::Pool{lines:at(input,244),count:g(input,121),overflow:effect_pool::line_zero(),exhausted:at(input,1860)};
 let overflow=unsafe{view_of(&mut pool.overflow).as_ptr().as_u16_array()};copy(overflow,at(input,1844),16);
 let spin:Buf<u16,2>=Buf::new([g(input,59).lo_bits(),g(input,60).lo_bits()]);
 let state=prefix::State{core:input,spin:at(input,20),boost:at(input,38),boost_control:at(input,46),auxiliary:auxiliary.as_array()};
 let shared=prefix::Shared{view:at(input,82),camera:at(input,112),spin:spin.as_array(),boost:at(input,122),temporary:at(input,140),pool:view_of(&mut pool),seed:at(input,222),slot:at(input,224)};
 let count=g(input,939).lo_bits();let mut frame=0u16;
 while frame<count {
  let control=at(input,1880+frame*12);
  let step=prefix::Frame{dt:g(control,0),number:g(control,1),active:g(control,2)!=fix32::zero(),player:g(input,113)!=fix32::zero(),entity:g(input,114),camera:at(control,6),sine:unsafe{sine.as_array().as_ptr().as_i16_array()}};
  let status=prefix::prepare(&state,&shared,&step,at(input,230),&scene,&identities);
  attitude::put(input,34,auxiliary.as_array()[0u16].cached);attitude::put(input,40,auxiliary.as_array()[1u16].cached);
  attitude::put(input,59,fix32::from_words(spin[0u16],0));attitude::put(input,60,fix32::from_words(spin[1u16],0));
  attitude::put(input,121,pool.count);copy(at(input,1844),overflow,16);
  attitude::put(out,0,status);copy(at(out,2),input,1876);
  unsafe{Ptr::from_addr(0xff).write(0,frame+1);}frame+=1;
 }
 halt(0x600d);
}
"#,
        output,
    )?;
    let sine = fs::read(fixture.join("sine.i16.bin"))?;
    let cases = data["cases"].as_array().ok_or("prefix cases")?;
    let mut checked = 0;
    let mut frames = 0;
    let mut steps = 0u64;
    for (index, case) in cases.iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        let scene = fs::read(fixture.join(case["scene"].as_str().ok_or("prefix scene")?))?;
        for (base, bytes) in [(0x280000usize, &scene), (0x2c0000, &sine)] {
            for (i, pair) in bytes.chunks_exact(2).enumerate() {
                cpu.physical_memory_mut()[base + i] = u16::from_le_bytes([pair[0], pair[1]]);
            }
        }
        let input = case["input"].as_array().ok_or("prefix input")?;
        assert!(input.len() * 2 < 0x1000);
        for (i, v) in input.iter().enumerate() {
            cpu.physical_memory_mut()[INPUT + i * 2..INPUT + i * 2 + 2]
                .copy_from_slice(&words(v.as_i64().unwrap() as i32));
        }
        let identity = case["identity"].as_array().ok_or("prefix identity")?;
        assert!(identity.len() < 0x1000);
        for (i, v) in identity.iter().enumerate() {
            cpu.physical_memory_mut()[0x6000 + i] = v.as_u64().unwrap() as u16;
        }
        let expected = case["frames"].as_array().ok_or("prefix frames")?;
        let mut done = 0usize;
        let mut passed = false;
        for step in 1..=30000000 {
            let outcome = cpu
                .step()
                .map_err(|e| format!("prefix case {index}: {e:?}"))?;
            let memory = cpu.physical_memory_slice();
            if usize::from(memory[0xff]) > done {
                assert_eq!(usize::from(memory[0xff]), done + 1);
                for (field, value) in expected[done]
                    .as_array()
                    .ok_or("prefix frame")?
                    .iter()
                    .enumerate()
                {
                    let at = RESULT + field * 2;
                    let actual = i32::from(memory[at]) | (i32::from(memory[at + 1]) << 16);
                    let want = value.as_i64().unwrap() as i32;
                    if actual != want {
                        return Err(format!(
                            "prefix case {index} {} frame {done} field {field}: {actual} != {want}",
                            case["tag"]
                        )
                        .into());
                    }
                    checked += 1;
                }
                done += 1;
                frames += 1;
            }
            if let StepOutcome::Halted { signal } = outcome {
                if signal != PASS {
                    return Err(format!("prefix case {index}: halt {signal:04x}").into());
                }
                assert_eq!(done, expected.len());
                steps += step;
                passed = true;
                break;
            }
        }
        if !passed {
            return Err(format!("prefix case {index}: step limit").into());
        }
    }
    let report = json!({"cases":cases.len(),"frames":frames,"checked_i32_values":checked,"maximum_integer_error":0,
        "code_bytes":code.total_code_words()*2,"instruction_steps":steps,"scope":data["scope"],
        "boundary":"Complete prepare method, instruction semantics and original constructed class/identity resources; continuous driving integration remains separate"});
    fs::write(
        output.join("prefix-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
