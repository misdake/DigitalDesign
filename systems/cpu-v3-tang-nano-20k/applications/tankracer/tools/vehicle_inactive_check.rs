//! Original inactive Tank snapshots, with the live row and shared state in DSEG2.
use super::*;
const WORDS: usize = 14796;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("vehicle-inactive.json"))?)?;
    let code = compile(
        "vehicle-inactive",
        r#"
fn ptr(a:u16)->Array<u16>{unsafe{Ptr::from_addr(a).as_u16_array()}}
fn at(v:Array<u16>,w:u16)->Array<u16>{unsafe{v.as_ptr().add(w as i16).as_u16_array()}}
fn g(v:Array<u16>,i:u16)->fix32{attitude::get(v,i)}
fn p(v:Array<u16>,i:u16,x:fix32){attitude::put(v,i,x);}
fn copy(mut dst:Array<u16>,src:Array<u16>,count:u16){let mut i=0u16;while i<count{dst[i]=src[i];i+=1;}}
fn restore(state:&mut impact::State,src:Array<u16>){
 state.next=g(src,5);let mut cursor=824u16;let mut i=0u16;
 while i<4{
  let b=&mut state.batches[i];b.kind=g(src,cursor);b.corner=g(src,cursor+1);b.owner=g(src,cursor+2);b.active=g(src,cursor+3)!=fix32::zero();b.age=g(src,cursor+4);b.frame=g(src,cursor+5);cursor+=6;
  let mut j=0u16;while j<10{
   let p=&mut b.particles[j];p.distance[0u16]=g(src,cursor).lo_bits() as i16;p.distance[1u16]=g(src,cursor+1).lo_bits() as i16;let mut k=0u16;
   while k<2{attitude::put(p.speed.as_array(),k,g(src,cursor+2+k));attitude::put(p.pitch.as_array(),k,g(src,cursor+4+k));attitude::put(p.yaw.as_array(),k,g(src,cursor+6+k));let v=g(src,cursor+9+k);p.colors[k*2]=v.lo_bits();p.colors[k*2+1]=v.hi_bits();k+=1;}
   p.line=g(src,cursor+8);cursor+=11;j+=1;
  }i+=1;
 }
}
fn main(){
 let previous=read_dseg_page(2);let previous3=read_dseg_page(3);unsafe{write_dseg_page(2,138);write_dseg_page(3,137);}
 let s=ptr(0x8000);let pool=at(s,4844);let capture=at(s,11620);
 let banks:Buf<impact::Batch,4>=Buf::new([impact::batch_zero(),impact::batch_zero(),impact::batch_zero(),impact::batch_zero()]);
 let mut live_impacts=impact::State{page:65535,batches:banks.as_array(),next:fix32::zero()};
 let mut live_quads=quad_pool::Pool{records:pool,exhausted:at(pool,5108),count:g(pool,2570)};
 let mut context=impact::Context{frame:fix32::zero(),seed:at(s,14196),pool:effect_pool::Pool{page:65535,lines:at(pool,5144),count:g(pool,2571),overflow:effect_pool::line_zero(),exhausted:at(pool,6760)},sine:unsafe{ptr(0x2500).as_ptr().as_i16_array()},angle:unsafe{ptr(0x2504).as_ptr().as_i16_array()},forward_offset:ptr(0x2520),reverse_offset:ptr(0x2526)};
 let overflow=unsafe{view_of(&mut context.pool.overflow).as_ptr().as_u16_array()};copy(overflow,at(pool,6744),16);
 let prefix=prefix::Shared{view:at(s,14226),camera:at(s,14256),spin:at(s,4840),boost:at(s,14262),temporary:at(s,4566),pool:view_of(&mut context.pool),seed:context.seed,slot:ptr(0x2580)};
 let mut shared=car_effects::Shared{pickup:pickup_light::State{banks:at(s,2934)},impacts:view_of(&mut live_impacts),debris:at(s,1760),shield:at(s,2124),quads:view_of(&mut live_quads),temporary:temporary::State{core:at(s,4566),rows:at(s,4648)},owners:vehicle_debris::Owners{rows:at(s,14202),count:3,stride:4},own:0,owner_slots:3,impact_accumulator:at(s,701+152),sound:at(s,14198)};
 let mut vehicle=impact::Vehicle{entity:g(at(s,701),0),cooldown:at(s,701+146),contacts:at(s,252),contact_stride:4,forward:g(at(s,339),2),basis:at(s,6),has_basis:true};
 restore(&mut live_impacts,capture);
 let cells=static_assets::AssetBytes{first_sector:8192,sector_count:16384,byte_len_lo:0,byte_len_hi:32};
 let mut world=ground::World::begin(cells,fix32::zero(),ptr(0x2600));
 unsafe{write_dseg_page(3,137);}
 let state=vehicle_inactive::State{
 poses:vehicle_pose::State{physical_position:s,reference:at(s,6),base_position:at(s,24),body_basis:at(s,30),body_position:at(s,48),tracks:at(s,54),turret:at(s,102),gun:at(s,126),attachments:at(s,150),bottom_position:at(s,198),recoil:at(s,204)},
 previous:at(s,208),velocity:at(s,214),angular:at(s,220),acceleration:at(s,226),pivot:at(s,230),rates:at(s,331),travel:at(s,284),travel_stride:1,hull:at(s,292),sample:at(s,437),local_velocity:at(s,339),track_speed:at(s,345),load:at(s,355),vertical_delta:at(s,417),boost:at(s,643),core:at(s,651),spin:at(s,671),boost_control:at(s,689),pose_config:at(s,1661),pose_input:at(s,1715),stored_input:at(s,777),height:at(s,717),spray:at(s,921),smoke:at(s,1295),lights:at(s,1645),debris:at(s,1655),cooldown:at(s,847),contacts:at(s,252),contact_stride:4,reverse_braking:at(s,797),disabled:at(s,849)
 };
 let count=ptr(0x2300)[0u16];let mut tick=0u16;
 while tick<count{
  let f=ptr(0x3000+tick*10);let input=vehicle_inactive::Input{entity:g(at(s,701),0),player:g(at(s,701),2)!=fix32::zero(),drive:at(s,14280),hull:ptr(0x2400),pickup:ptr(0x2420),light_offset:ptr(0x2450),light_material:g(ptr(0x2460),0),numerics:ptr(0x2300)[1u16],dt:g(f,0),number:g(f,1),mode:g(f,2),throttle:g(f,3),brake:g(f,4)};
  let status=vehicle_inactive::step(&state,&input,&mut world,&prefix,&mut shared,&mut context);
  p(pool,2570,live_quads.count);p(pool,2571,context.pool.count);copy(at(pool,6744),overflow,16);
  copy(at(s,14204),at(s,1655),6);
  impact_capture::write(&mut live_impacts,&vehicle,&mut context,capture);
  if read_dseg_page(2)!=138 || read_dseg_page(3)!=137{halt(0x7e95);}
  unsafe{Ptr::from_addr(0xfe).write(0,status.lo_bits());Ptr::from_addr(0xfd).write(0,status.hi_bits());Ptr::from_addr(0xff).write(0,tick+1);}tick+=1;
 }
 unsafe{write_dseg_page(2,previous);write_dseg_page(3,previous3);}halt(0x600d);
}
"#,
        output,
    )?;
    let sine = fs::read(fixture.join("sine.i16.bin"))?;
    let angle = fs::read(fixture.join("angle.i16.bin"))?;
    let cells = fs::read(fixture.join("projectile-cells.bin"))?;
    assert_eq!(cells.len(), 64 * 32768);
    let cases = data["cases"].as_array().unwrap();
    let mut calls = 0;
    let mut checked = 0;
    let mut instructions = 0u64;
    let mut minimum = u16::MAX;
    for (index, case) in cases.iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        let memory = cpu.physical_memory_mut();
        for (base, bytes) in [
            (45056 * 64, &sine),
            (45376 * 64, &angle),
            (8192 * 64, &cells),
        ] {
            for (i, pair) in bytes.chunks_exact(2).enumerate() {
                memory[base + i] = u16::from_le_bytes([pair[0], pair[1]]);
            }
        }
        memory[138 * 16384..139 * 16384].fill(0x79ab);
        memory[137 * 16384..138 * 16384].fill(0xb63c);
        let input = case["input"].as_array().unwrap();
        assert_eq!(input.len(), WORDS);
        for (i, v) in input.iter().enumerate() {
            memory[138 * 16384 + i] = v.as_u64().unwrap() as u16;
        }
        for (name, base) in [("hull", 0x2400), ("light_offset", 0x2450)] {
            for (i, v) in case[name].as_array().unwrap().iter().enumerate() {
                memory[base + i * 2..base + i * 2 + 2]
                    .copy_from_slice(&words(v.as_i64().unwrap() as i32));
            }
        }
        for (i, v) in case["pickup"].as_array().unwrap().iter().enumerate() {
            memory[0x2420 + i] = v.as_u64().unwrap() as u16;
        }
        memory[0x2460..0x2462]
            .copy_from_slice(&words(case["light_material"].as_u64().unwrap() as i32));
        memory[0x2500..0x2508].copy_from_slice(&[45056, 320, 40960, 0, 45376, 5, 514, 0]);
        memory[0x2520..0x252c].copy_from_slice(&[0, 0, 0, 0, 2000, 0, 0, 0, 0, 0, 63536, 65535]);
        for i in 0..64 {
            memory[0x2600 + i] = i as u16;
        }
        let controls = case["controls"].as_array().unwrap();
        memory[0x2300] = controls.len() as u16;
        memory[0x2301] = case["numerics"].as_u64().unwrap() as u16;
        for (tick, f) in controls.iter().enumerate() {
            for (i, v) in f.as_array().unwrap().iter().enumerate() {
                let at = 0x3000 + tick * 10 + i * 2;
                memory[at..at + 2].copy_from_slice(&words(v.as_i64().unwrap() as i32));
            }
        }
        let immutable = memory[0x2400..0x2700].to_vec();
        let frames = case["frames"].as_array().unwrap();
        let mut done = 0;
        let mut passed = false;
        let mut started = false;
        for step in 1..=5_000_000 {
            let result = cpu
                .step()
                .map_err(|e| format!("inactive case {index}: {e:?}"))?;
            let sp = cpu.registers()[13];
            if started || sp != 0 {
                started = true;
                minimum = minimum.min(sp);
                assert!(sp >= 0x6800);
            }
            let memory = cpu.physical_memory_slice();
            if usize::from(memory[0xff]) > done {
                assert_eq!(usize::from(memory[0xff]), done + 1);
                let expected = frames[done].as_array().unwrap();
                assert_eq!(expected.len(), WORDS + 2);
                for (word, want) in expected.iter().enumerate() {
                    let at = if word == 0 {
                        0xfe
                    } else if word == 1 {
                        0xfd
                    } else {
                        138 * 16384 + word - 2
                    };
                    let actual = memory[at];
                    let want = want.as_u64().unwrap() as u16;
                    if actual != want {
                        fs::write(
                            output.join("failed-actual.bin"),
                            memory[138 * 16384..138 * 16384 + WORDS]
                                .iter()
                                .flat_map(|v| v.to_le_bytes())
                                .collect::<Vec<_>>(),
                        )?;
                        return Err(format!(
                            "inactive case {index} {} call {done} word {word}: {actual} != {want}",
                            case["tag"]
                        )
                        .into());
                    }
                    checked += 1;
                }
                assert!(
                    memory[138 * 16384 + WORDS..139 * 16384]
                        .iter()
                        .all(|v| *v == 0x79ab)
                );
                assert!(
                    memory[137 * 16384..138 * 16384]
                        .iter()
                        .all(|v| *v == 0xb63c)
                );
                assert_eq!(&memory[0x2400..0x2700], &immutable);
                for (tick, f) in controls.iter().enumerate() {
                    for (i, v) in f.as_array().unwrap().iter().enumerate() {
                        let at = 0x3000 + tick * 10 + i * 2;
                        assert_eq!(&memory[at..at + 2], &words(v.as_i64().unwrap() as i32));
                    }
                }
                calls += 1;
                done += 1;
            }
            if let StepOutcome::Halted { signal } = result {
                assert_eq!(signal, PASS);
                assert_eq!(done, frames.len());
                instructions += step;
                passed = true;
                break;
            }
        }
        assert!(passed, "inactive case {index} instruction bound");
    }
    let report = json!({"cases":cases.len(),"calls":calls,"checked_u16_values":checked,"maximum_integer_error":0,"code_bytes":code.total_code_words()*2,"maximum_stack_bytes":(32768-usize::from(minimum))*2,"instruction_steps":instructions,"state_and_diagnostic_bytes":WORDS*2,"vehicle_row_bytes":3520,"scope":data["scope"],"backend":"instruction, real cell assets and immutable original trig tables"});
    fs::write(
        output.join("vehicle-inactive-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
