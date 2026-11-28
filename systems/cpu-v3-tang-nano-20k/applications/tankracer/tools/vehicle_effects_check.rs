//! Banked smoke/debris/shield with real shared impact state and immutable tables.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("vehicle-effects.json"))?)?;
    let code = compile(
        "vehicle-effects",
        r#"
fn ptr(a:u16)->Array<u16>{unsafe{Ptr::from_addr(a).as_u16_array()}}
fn at(v:Array<u16>,w:u16)->Array<u16>{unsafe{v.as_ptr().add(w as i16).as_u16_array()}}
fn g(v:Array<u16>,i:u16)->fix32{attitude::get(v,i)}
fn p(v:Array<u16>,i:u16,x:fix32){attitude::put(v,i,x);}
fn copy(mut to:Array<u16>,from:Array<u16>,count:u16){let mut i=0u16;while i<count{to[i]=from[i];i+=1;}}
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
 let s=ptr(0x8000);let capture=at(s,1558);
 let banks:Buf<impact::Batch,4>=Buf::new([impact::batch_zero(),impact::batch_zero(),impact::batch_zero(),impact::batch_zero()]);
 let mut impacts=impact::State{page:65535,batches:banks.as_array(),next:fix32::zero()};restore(&mut impacts,capture);
 let mut vehicle=impact::Vehicle{entity:fix32::zero(),cooldown:at(s,1556),contacts:s,contact_stride:3,forward:fix32::zero(),basis:s,has_basis:false};
 let sine:Buf<u16,4>=Buf::new([45056,320,40960,0]);let angle:Buf<u16,4>=Buf::new([45376,5,514,0]);
 let offsets:Buf<u16,12>=Buf::new([0,0,0,0,2000,0,0,0,0,0,63536,65535]);
 let mut context=impact::Context{frame:fix32::zero(),seed:at(s,1554),pool:effect_pool::Pool{page:65535,lines:at(capture,16),count:g(capture,7),overflow:effect_pool::line_zero(),exhausted:at(capture,1632)},sine:unsafe{sine.as_array().as_ptr().as_i16_array()},angle:unsafe{angle.as_array().as_ptr().as_i16_array()},forward_offset:offsets.as_array(),reverse_offset:at(offsets.as_array(),6)};
 let overflow=unsafe{view_of(&mut context.pool.overflow).as_ptr().as_u16_array()};copy(overflow,at(capture,1616),16);
 let owners=vehicle_debris::Owners{rows:at(s,714),count:3,stride:4};
 let debris_context=vehicle_debris::Context{kinds:unsafe{ptr(0x2480).as_ptr().as_i16_array()},seed:at(s,1554),impact_accumulator:at(s,738),sound:at(s,740)};
 let count=ptr(0x2300)[0u16];let mut frame=0u16;
 while frame<count{
  let f=ptr(0x3000+frame*158);let op=g(f,0).lo_bits();let mut status=fix32::zero();
  vehicle.entity=g(f,14);vehicle.forward=g(f,13);vehicle.contacts=at(f,48);vehicle.basis=at(f,140);vehicle.has_basis=g(f,12)!=fix32::zero();context.frame=g(f,15);
  if op==0 || op==5{
   status=vehicle_smoke::step(s,&vehicle_smoke::Input{dt:g(f,1),player:g(f,2)!=fix32::zero(),special:g(f,3)!=fix32::zero(),velocity:at(f,32),lateral:g(f,4),slip:g(f,5),penetration:g(f,6),materials:at(f,44),contacts:at(f,48),contact_stride:3,corners:at(f,72),corner_stride:1,turret:at(f,80),body:at(f,104)},ptr(0x2400),&mut impacts,&mut vehicle,&mut context);
  }
  if op==1 || (op==5 && status==fix32::zero()){
   status=vehicle_debris::step(at(s,350),&owners,g(f,9).lo_bits(),&vehicle_debris::Input{dt:g(f,1),player:g(f,2)!=fix32::zero(),position:at(f,38),velocity:at(f,32),has_plane:g(f,7),plane_distance:g(f,8),materials:at(f,44),contacts:at(f,48),contact_stride:6,camera_position:at(f,128),camera_forward:at(f,134)},&debris_context);
  }
  if op==2 || (op==5 && status==fix32::zero()){
   status=vehicle_shield::tick(at(s,744),&vehicle_shield::Input{enabled:g(f,10)!=fix32::zero(),bank:g(f,11),dt:g(f,1),position:at(f,38),basis:at(f,140),has_basis:g(f,12)!=fix32::zero()},at(s,1554),context.sine);
  }
  if op==3{vehicle_shield::initialize(at(s,744),at(s,1554));}
  if op==4{vehicle_debris::clear(at(s,350),g(f,2)!=fix32::zero(),at(owners.rows,g(f,9).lo_bits()*8+2));}
  impact_capture::write(&mut impacts,&vehicle,&mut context,capture);
  if read_dseg_page(2)!=138 || read_dseg_page(3)!=137{halt(0x7e90);}
  unsafe{Ptr::from_addr(0xfe).write(0,status.lo_bits());Ptr::from_addr(0xfd).write(0,status.hi_bits());Ptr::from_addr(0xff).write(0,frame+1);}frame+=1;
 }
 unsafe{write_dseg_page(2,previous);write_dseg_page(3,previous3);}halt(0x600d);
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
        for (base, bytes) in [(45056 * 64, &sine), (45376 * 64, &angle)] {
            for (i, pair) in bytes.chunks_exact(2).enumerate() {
                memory[base + i] = u16::from_le_bytes([pair[0], pair[1]]);
            }
        }
        let input = case["input"].as_array().unwrap();
        assert_eq!(input.len(), 4134);
        for (i, v) in input.iter().enumerate() {
            memory[138 * 16384 + i] = v.as_u64().unwrap() as u16;
        }
        memory[138 * 16384 + 4134..139 * 16384].fill(0x79ab);
        let kinds = case["kinds"].as_array().unwrap();
        assert_eq!(kinds.len(), 16);
        for (i, v) in kinds.iter().enumerate() {
            memory[0x2480 + i] = v.as_i64().unwrap() as u16;
        }
        let sockets = case["sockets"].as_array().unwrap();
        assert_eq!(sockets.len(), 36);
        for (i, v) in sockets.iter().enumerate() {
            memory[0x2400 + i * 2..0x2402 + i * 2]
                .copy_from_slice(&words(v.as_i64().unwrap() as i32));
        }
        let controls = case["controls"].as_array().unwrap();
        assert!(controls.len() <= 32);
        memory[0x2300] = controls.len() as u16;
        for (frame, c) in controls.iter().enumerate() {
            assert_eq!(c.as_array().unwrap().len(), 79);
            for (i, v) in c.as_array().unwrap().iter().enumerate() {
                let at = 0x3000 + frame * 158 + i * 2;
                memory[at..at + 2].copy_from_slice(&words(v.as_i64().unwrap() as i32));
            }
        }
        let expected = case["frames"].as_array().unwrap();
        let mut done = 0;
        let mut passed = false;
        let mut started = false;
        for step in 1..=4000000 {
            let result = cpu
                .step()
                .map_err(|e| format!("vehicle-effects case {index}: {e:?}"))?;
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
                assert_eq!(expected.len(), 4136);
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
                        return Err(format!("vehicle-effects case {index} {} call {done} word {word}: {actual} != {want}",case["tag"]).into());
                    }
                    checked += 1;
                }
                assert!(
                    memory[138 * 16384 + 4134..139 * 16384]
                        .iter()
                        .all(|v| *v == 0x79ab)
                );
                for (i, v) in kinds.iter().enumerate() {
                    assert_eq!(memory[0x2480 + i], v.as_i64().unwrap() as u16);
                }
                for (i, v) in sockets.iter().enumerate() {
                    assert_eq!(
                        &memory[0x2400 + i * 2..0x2402 + i * 2],
                        &words(v.as_i64().unwrap() as i32)
                    );
                }
                for (frame, c) in controls.iter().enumerate() {
                    for (i, v) in c.as_array().unwrap().iter().enumerate() {
                        let at = 0x3000 + frame * 158 + i * 2;
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
            return Err(format!("vehicle-effects case {index}: instruction bound").into());
        }
    }
    let report = json!({"cases":cases.len(),"calls":calls,"checked_u16_values":checked,"maximum_integer_error":0,"instruction_steps":instructions,"code_bytes":code.total_code_words()*2,"minimum_stack_word":minimum_stack,"maximum_stack_bytes":(0x8000-usize::from(minimum_stack))*2,"smoke_bytes":700,"debris_banks_bytes":728,"debris_owner_bytes":16,"shield_bytes":1620,"snapshot_bytes":8268,"scope":data["scope"],"backend":"instruction, banked state and immutable math tables without GPU competition"});
    fs::write(
        output.join("vehicle-effects-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
