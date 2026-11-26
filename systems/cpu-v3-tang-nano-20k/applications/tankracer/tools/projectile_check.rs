//! Launch/flight/trail/sweep answers checked at every completed original call.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("projectile.json"))?)?;
    let code = compile(
        "projectile",
        r#"
fn at(v:Array<u16>,w:u16)->Array<u16>{unsafe{v.as_ptr().add(w as i16).as_u16_array()}}
fn g(v:Array<u16>,i:u16)->fix32{attitude::get(v,i)}
fn p(v:Array<u16>,i:u16,x:fix32){attitude::put(v,i,x);}
fn n(v:i16)->fix32{integer::from_i16(v)}
fn copy(mut to:Array<u16>,from:Array<u16>,count:u16){let mut i=0u16;while i<count{to[i]=from[i];i+=1;}}
fn car(data:Array<u16>)->projectile_sweep::Car{
 projectile_sweep::Car{present:data[0u16]!=0,disabled:data[1u16]!=0,visible:data[2u16]!=0,ai_disabled:data[3u16]!=0,entity:g(at(data,4),0),
 position:at(data,6),basis:at(data,12),turret:at(data,30),velocity:at(data,48),bounds:at(data,54),radius:g(at(data,66),0)}
}
fn pack_action(mut out:Array<u16>,action:&projectile_flight::Action){
 out[0u16]=action.kind;out[1u16]=0;out[14u16]=0;
 if action.has_mark{out[1u16]=1;copy(at(out,8),action.mark.as_array(),6);}
 if action.kind==4{copy(at(out,2),action.origin.as_array(),6);}
 if action.has_contact{
  out[14u16]=1;p(at(out,15),0,action.contact.vehicle);
  copy(at(out,17),action.contact.world.point.as_array(),6);copy(at(out,23),action.contact.world.normal.as_array(),6);
  copy(at(out,29),action.contact.local.point.as_array(),6);copy(at(out,35),action.contact.local.normal.as_array(),6);
 }
}
fn main(){
 let input=unsafe{Ptr::from_addr(0x0100).as_u16_array()};let out=unsafe{Ptr::from_addr(0x1800).as_u16_array()};
 let cells=static_assets::AssetBytes{first_sector:16384,sector_count:16384,byte_len_lo:0,byte_len_hi:32};
 let directory:Buf<u16,64>=Buf::new([0;64]);let mut indices=directory.as_array();let mut i=0u16;while i<64{indices[i]=i;i+=1;}
 let mut map=ground::World::begin(cells,fix32::zero(),indices);
 let sine:Buf<u16,4>=Buf::new([45056,320,40960,0]);let angle:Buf<u16,4>=Buf::new([45376,5,514,0]);
 let state=projectile::view(input);let pose=projectile::Pose{basis:at(input,664),position:at(input,682)};
 let ctx=projectile::Context{shared:projectile::shared(at(input,610)),resources:projectile::Resources{tuning:at(input,695),main:at(input,725),trail:at(input,737),homing_tail:at(input,749),cup:input[755u16]},
 tables:projectile::Tables{sine:unsafe{sine.as_array().as_ptr().as_i16_array()},angle:unsafe{angle.as_array().as_ptr().as_i16_array()}},seed:at(input,688)};
 let owner=projectile::Owner{entity:g(at(input,756),0),turret:at(input,758),gun:at(input,776),velocity:at(input,782)};
 let records:Buf<projectile_sweep::Car,3>=Buf::new([car(at(input,788)),car(at(input,856)),car(at(input,924))]);
 let count=input[1002u16];let mut frame=0u16;
 while frame<count{
  let c=at(input,1003+frame*9);let op=c[0u16];let dt=g(at(c,2),0);let number=g(at(c,4),0);let entity=g(at(c,6),0);let mut status=n(0);
  let cars=projectile_sweep::Cars{records:records.as_array(),count:3,slots:at(input,992),present:input[1001u16]!=0};
  let mut action=projectile_flight::zero();
  if op==0{status=projectile::launch(&state,&pose,&owner,c[1u16],&ctx);if status==n(0){projectile::sprite(&state,&ctx);}}
  else if op==1{status=projectile_trail::emit(&state,&pose,&ctx,dt);if status==n(0){status=projectile_trail::fade(&state,dt);}}
  else if op==2{status=projectile_trail::fade(&state,dt);}
  else if op==3 || op==4{
   let step=projectile_flight::Frame{dt:dt,number:number,entity:entity,carrier_present:c[8u16]&1!=0,carrier_basis:owner.turret,carrier_position:owner.gun,carrier_released:c[8u16]&2!=0,cars:cars,overlay:at(input,690)};
   status=projectile_flight::advance(&state,&pose,&step,&ctx,&mut map,&mut action);
   if status==n(0) && op==4 && (action.kind==1 || action.kind==2){status=projectile_trail::fade(&state,dt);}
  }else if op==5{status=projectile::select_target(&state,&cars,&ctx.tables);}
  else if op==6{status=projectile_ricochet::reflect(&state,&pose,c[8u16],&cars,&ctx);}
  else if op==7{status=projectile_ricochet::ownerless(&state,entity,&ctx);}
  else if op==8{
   let mut sweep=projectile_sweep::Frame{dt:dt,hit:view_of(&mut action.contact)};
   status=projectile_sweep::cars(&cars,g(state.core,1),g(state.core,0)!=n(0),owner.gun,pose.position,&mut sweep);
   if status==n(1){action.kind=4;action.has_contact=true;copy(action.origin.as_array(),owner.gun,6);status=n(0);}
  }
  let mut clear=out;i=0;while i<43{clear[i]=0;i+=1;}
  p(out,0,status);pack_action(at(out,2),&action);copy(at(out,43),input,695);
  if g(state.core,0)==n(0){p(at(out,43),1,n(0));}
  if g(state.core,14)==n(0){p(at(out,43),15,n(0));}
  if g(state.core,25)==n(0){p(at(out,43),26,n(0));}
  if g(state.core,20)==n(0){i=0;while i<18{clear[43+60+i]=0;i+=1;}}
  if input[690u16]==0{i=0;while i<4{clear[43+691+i]=0;i+=1;}}
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
    let cases = data["cases"].as_array().ok_or("projectile cases")?;
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
        let input = case["input"].as_array().ok_or("projectile input")?;
        assert_eq!(input.len(), 1002);
        for (i, v) in input.iter().enumerate() {
            cpu.physical_memory_mut()[0x100 + i] = v.as_u64().unwrap() as u16;
        }
        let controls = case["controls"].as_array().ok_or("projectile controls")?;
        cpu.physical_memory_mut()[0x100 + 1002] = controls.len() as u16;
        for (step, c) in controls.iter().enumerate() {
            let c = c.as_array().unwrap();
            let mut values = vec![c[0].as_i64().unwrap() as u16, c[1].as_i64().unwrap() as u16];
            for v in &c[2..5] {
                values.extend(words(v.as_i64().unwrap() as i32));
            }
            values.push(c[5].as_i64().unwrap() as u16);
            let at = 0x100 + 1003 + step * 9;
            cpu.physical_memory_mut()[at..at + 9].copy_from_slice(&values);
        }
        let expected = case["frames"].as_array().unwrap();
        let mut done = 0;
        let mut passed = false;
        let mut started = false;
        for step in 1..=20000000 {
            let result = cpu
                .step()
                .map_err(|e| format!("projectile case {index}: {e:?}"))?;
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
                        return Err(format!("projectile case {index} {} call {done} word {field}: {actual} != {want}",case["tag"]).into());
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
            return Err(format!("projectile case {index}: instruction bound").into());
        }
    }
    let report = json!({"cases":cases.len(),"calls":calls,"checked_u16_values":checked,"maximum_integer_error":0,"instruction_steps":steps,"code_bytes":code.total_code_words()*2,"minimum_stack_word":minimum_stack,
        "projectile_record_bytes":1220,"projectile_pose_bytes":48,"shared_flight_bytes":108,"scope":data["scope"],"boundary":"Methods and real terrain/vehicle queries in instruction sim; entity construction, impact callbacks, live scheduling and native loading remain separate"});
    fs::write(
        output.join("projectile-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
