//! The production RCC collision math against independently exported Rust answers.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("collision.json"))?)?;
    let code = compile(
        "collision",
        &format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            r#"fn main(){
 let input=unsafe{Ptr::from_addr(0x2000).as_u16_array()};let out=unsafe{Ptr::from_addr(0x1000).as_u16_array()};
 let sine=unsafe{Ptr::from_addr(0x3000).as_i16_array()};let angle=unsafe{Ptr::from_addr(0x8000).as_i16_array()};
 let kind=unsafe{Ptr::from_addr(0x1ffe).read(0)};let mut status=0u16;
 if kind==0{let config=unsafe{Ptr::from_addr(0x200c).as_u16_array()};status=collision::respond(input,config,sine,angle,out);}
 else if kind==1{collision::exchange(input,out);}
 else if kind==2{
  attitude::put(out,3,collision::distance(input));attitude::put(out,4,fix32::from_words(collision::direction(attitude::get(input,0),attitude::get(input,2)),0));
  collision::normalize(input);let mut i=0u16;while i<3{attitude::put(out,i,attitude::get(input,i));i+=1;}
 }else if kind==3{attitude::put(out,0,collision::one_point_seven(attitude::get(input,0)));}
 else if kind==4{let found=collision::approaching(input,attitude::get(input,12));let mut value=fix32::zero();if found{value=integer::from_i16(1);}attitude::put(out,0,value);}
 else if kind==5{
  let cfg=unsafe{Ptr::from_addr(0x2008).as_u16_array()};let binding:Buf<u16,10>=Buf::new([0;10]);let mut bindings=binding.as_array();
  let vertices:Buf<i16,6>=Buf::new([0;6]);let mut mesh=vertices.as_array();let mut i=0u16;
  while i<10{bindings[i]=attitude::get(input,11+i).lo_bits();i+=1;}
  i=0;while i<6{mesh[i]=attitude::get(input,21+i).lo_bits() as i16;i+=1;}
  status=body::articulate(input,cfg,mesh,bindings);i=0;while i<4{attitude::put(out,i,attitude::get(input,i));i+=1;}
  i=0;while i<6{attitude::put(out,4+i,integer::from_i16(mesh[i]));i+=1;}
 }else if kind==6{
  let accel=unsafe{Ptr::from_addr(0x2008).as_u16_array()};let cfg=unsafe{Ptr::from_addr(0x200c).as_u16_array()};body::lean(input,accel,cfg);
  let mut i=0u16;while i<4{attitude::put(out,i,attitude::get(input,i));i+=1;}
 }else if kind==7{
  let prev=unsafe{Ptr::from_addr(0x2006).as_u16_array()};let contacts=unsafe{Ptr::from_addr(0x200c).as_u16_array()};let raw:Buf<u16,37>=Buf::new([0;37]);let result=raw.as_array();body::hull(result,input,prev,contacts);
  let mut i=0u16;while i<12{attitude::put(out,i,attitude::get(result,i));i+=1;}
  i=0;while i<12{attitude::put(out,12+i,integer::from_i16(result[24+i] as i16));i+=1;}attitude::put(out,24,fix32::from_words(result[36u16],0));
 }else if kind==8{status=run_box(input,out);}
 else if kind==9{status=run_objects(input,out);}
 else if kind==10{status=run_tanks(input,out);}
 else if kind==12{status=run_deform_hit(input,out);}
 else if kind==13{run_surface_sample(input,out);}
 else if kind==14{status=run_damage(input,out);}
 else if kind==15{run_impact(input,out,sine,angle);}
 else if kind==16{run_ground_response(input,out,sine,angle);}
 else if kind==17{run_tank_response(input,out,sine,angle,false);}
 else if kind==29{run_tank_response(input,out,sine,angle,true);}
 else if kind==30{run_car_deform(input,out,sine,angle);}
 else if kind==31{run_car_scenery(input,out,sine,angle);}
 else if kind==32{run_entities(input,out);}
 else if kind==33{
  let words=attitude::get(input,7).lo_bits();
  let asset=static_assets::AssetBytes{first_sector:40960,sector_count:(words+63)>>6,byte_len_lo:words<<1,byte_len_hi:words>>15};
  run_scene_lifecycle(input,out,asset);
 }
 else if kind==18{run_lifecycle(input,out);}
 else if kind==19{run_crush(input,out);}
 else if kind==20{run_spatial(input,out);}
 else if kind==21{run_falling(input,out,sine,angle);}
 else if kind==22{run_blast(input,out);}
 else if kind==23{run_scene_world(input,out,sine);}
 else if kind==24{run_scene_contact(input,out,sine,angle);}
 else if kind==25{run_fall_hit(input,out);}
 else if kind==26{run_bounce_hit(input,out,sine);}
 else if kind==27{run_bounce_tick(input,out,sine,angle);}
 else if kind==28{run_live_contact(input,out,sine,angle);}
 else{
  let origin=unsafe{Ptr::from_addr(0x2006).as_u16_array()};let index=object_query::cell(input,origin,attitude::get(input,5).lo_bits(),attitude::get(input,6).lo_bits());attitude::put(out,0,fix32::from_words(index,0));
 }
 unsafe{Ptr::from_addr(0x1fff).write(0,status);}halt(0x600d);
}
fn run_box(input:Array<u16>,out:Array<u16>)->u16{
  let b=unsafe{Ptr::from_addr(0x2006).as_u16_array()};let bounds=unsafe{Ptr::from_addr(0x200c).as_u16_array()};let mut hit=object_query::SegmentHit{point:Buf::new([0;6]),normal:Buf::new([0;6])};
  let raw=unsafe{view_of(&mut hit).as_ptr().as_u16_array()};let mut values=raw;let mut i=0u16;while i<12{values[i]=input[24+i];i+=1;}
  let status=object_query::segment_box(input,b,bounds,&mut hit);let mut result=out;i=0;while i<12{result[i]=values[i];i+=1;}
status
}
fn run_damage(input:Array<u16>,out:Array<u16>)->u16{
 let flags:Buf<u16,10>=Buf::new([0;10]);let mut state=flags.as_array();let selected:Buf<u16,4>=Buf::new([0;4]);let mut sel=selected.as_array();
 let vertices:Buf<i16,12>=Buf::new([0;12]);let mut v=vertices.as_array();let indices:Buf<u16,9>=Buf::new([0;9]);let mut ix=indices.as_array();let normal_buffer:Buf<i16,9>=Buf::new([0;9]);let mut normals=normal_buffer.as_array();
 let mut i=0u16;while i<10{state[i]=attitude::get(input,15+i).lo_bits();i+=1;}i=0;while i<4{sel[i]=attitude::get(input,25+i).lo_bits();i+=1;}i=0;while i<12{v[i]=attitude::get(input,29+i).lo_bits() as i16;i+=1;}i=0;while i<9{ix[i]=attitude::get(input,41+i).lo_bits();normals[i]=attitude::get(input,50+i).lo_bits() as i16;i+=1;}
 let mut target=damage::Target{state:state,player:attitude::get(input,59)!=fix32::zero(),basis:input,position:unsafe{Ptr::from_addr(0x2012).as_u16_array()},selected:sel,mesh:damage::Mesh{present:attitude::get(input,60)!=fix32::zero(),vertices:v,vertex_count:attitude::get(input,63).lo_bits(),indices:ix,normals:normals,face_count:attitude::get(input,64).lo_bits()}};
 let world=unsafe{Ptr::from_addr(0x2018).as_u16_array()};let op=attitude::get(input,61);let mut status=0u16;i=0;while i<attitude::get(input,62).lo_bits(){if op>=integer::from_i16(2){status=damage::track(&mut target,op,world);}else{status=damage::hit(&mut target,world,op!=fix32::zero());}if status>=2{break;}i+=1;}
 i=0;while i<10{attitude::put(out,i,fix32::from_words(state[i],0));i+=1;}i=0;while i<12{attitude::put(out,10+i,integer::from_i16(v[i]));i+=1;}i=0;while i<9{attitude::put(out,22+i,integer::from_i16(normals[i]));i+=1;}status
}
fn run_deform_hit(input:Array<u16>,out:Array<u16>)->u16{
 let mut collider=object_query::collider_zero();let mut raw=unsafe{view_of(&mut collider).as_ptr().as_u16_array()};let mut i=0u16;while i<60{raw[i]=input[i];i+=1;}
 let cfg=deformation::HitConfig{floor:attitude::get(input,30),compression:attitude::get(input,31),drift_shift:attitude::get(input,32).lo_bits(),material:attitude::get(input,33).lo_bits() as i16,has_sound:attitude::get(input,34)!=fix32::zero()};
 let mut state=deformation::HitState{dirty:attitude::get(input,35),last_vehicle:attitude::get(input,36),sound_timer:attitude::get(input,37)};
 let mut vehicle=deformation::Vehicle{entity:attitude::get(input,38),position:unsafe{Ptr::from_addr(0x204e).as_u16_array()},material:unsafe{Ptr::from_addr(0x2054).as_u16_array()},seed:unsafe{Ptr::from_addr(0x2056).as_u16_array()},dt:attitude::get(input,44)};
 let vertices:Buf<i16,9>=Buf::new([0;9]);let mut v=vertices.as_array();i=0;while i<9{v[i]=attitude::get(input,45+i).lo_bits() as i16;i+=1;}
 let flags:Buf<u16,3>=Buf::new([0;3]);let mut f=flags.as_array();i=0;while i<3{f[i]=attitude::get(input,54+i).lo_bits();i+=1;}
 let mesh=deformation::Mesh{vertices:v,vertex_count:3,flags:f,flag_count:attitude::get(input,57).lo_bits()};
 let status=deformation::hit(&mut state,&cfg,&collider,&mesh,&mut vehicle);
 attitude::put(out,0,state.dirty);attitude::put(out,1,state.last_vehicle);attitude::put(out,2,state.sound_timer);attitude::put(out,3,attitude::get(vehicle.material,0));attitude::put(out,4,attitude::get(vehicle.seed,0));
 i=0;while i<9{attitude::put(out,5+i,integer::from_i16(v[i]));i+=1;}status
}
fn run_surface_sample(input:Array<u16>,mut out:Array<u16>){
 let vertices:Buf<i16,9>=Buf::new([0;9]);let mut v=vertices.as_array();let mut i=0u16;while i<9{v[i]=attitude::get(input,26+i).lo_bits() as i16;i+=1;}
 let records:Buf<deformation::Record,3>=Buf::new([deformation::record_zero(),deformation::record_zero(),deformation::record_zero()]);let mut raw=unsafe{records.as_array().as_ptr().as_u16_array()};i=0;while i<66{raw[i]=input[70+i];i+=1;}
 let mut surface=deformation::Surface{enabled:true,origin:input,basis:unsafe{Ptr::from_addr(0x2006).as_u16_array()},bounds:unsafe{Ptr::from_addr(0x2018).as_u16_array()},edges:unsafe{Ptr::from_addr(0x2020).as_u16_array()},cap:attitude::get(input,20),vertices:v,vertex_count:3,records:records.as_array(),record_count:3,attached:attitude::get(input,21)!=fix32::zero(),dirty:attitude::get(input,22).lo_bits()};
 let world=unsafe{Ptr::from_addr(0x202e).as_u16_array()};let mut first=deformation::sample(&mut surface,world);let mut second=deformation::sample(&mut surface,world);
 let a=unsafe{view_of(&mut first).as_ptr().as_u16_array()};let b=unsafe{view_of(&mut second).as_ptr().as_u16_array()};i=0;while i<20{out[i]=a[i];out[20+i]=b[i];i+=1;}
 attitude::put(out,20,fix32::from_words(surface.dirty,0));i=0;while i<66{out[42+i]=raw[i];i+=1;}
}
fn run_objects(input:Array<u16>,out:Array<u16>)->u16{
  let objects:Buf<object_query::Collider,2>=Buf::new([object_query::collider_zero(),object_query::collider_zero()]);let mut raw=unsafe{objects.as_array().as_ptr().as_u16_array()};
  let mut i=0u16;while i<120{raw[i]=input[20+i];i+=1;}
  let ids:Buf<u16,2>=Buf::new([0;2]);let mut id=ids.as_array();id[0u16]=attitude::get(input,70).lo_bits();id[1u16]=attitude::get(input,71).lo_bits();
  let q=object_query::Query{start:input,end:unsafe{Ptr::from_addr(0x2006).as_u16_array()},self_entity:attitude::get(input,6),object_count:attitude::get(input,9).lo_bits(),deformable:attitude::get(input,7)!=fix32::zero()};
  let mut hit=object_query::hit_zero();let mut raw_hit=unsafe{view_of(&mut hit).as_ptr().as_u16_array()};i=0;while i<18{raw_hit[i]=input[144+i];i+=1;}
  let status=object_query::query_list(id,attitude::get(input,8).lo_bits(),objects.as_array(),&q,&mut hit);
  let mut result=out;i=0;while i<18{result[i]=raw_hit[i];i+=1;}
status
}
fn run_tanks(input:Array<u16>,out:Array<u16>)->u16{
  let objects:Buf<object_query::Tank,3>=Buf::new([object_query::tank_zero(),object_query::tank_zero(),object_query::tank_zero()]);let mut raw=unsafe{objects.as_array().as_ptr().as_u16_array()};
  let mut i=0u16;while i<312{raw[i]=input[20+i];i+=1;}
  let slots:Buf<u16,9>=Buf::new([65535;9]);let mut ids=slots.as_array();i=0;while i<9{ids[i]=attitude::get(input,166+i).lo_bits();i+=1;}
  let q=object_query::Query{start:input,end:unsafe{Ptr::from_addr(0x2006).as_u16_array()},self_entity:attitude::get(input,6),object_count:attitude::get(input,9).lo_bits(),deformable:false};
  let mut hit=object_query::hit_zero();let mut raw_hit=unsafe{view_of(&mut hit).as_ptr().as_u16_array()};i=0;while i<18{raw_hit[i]=input[350+i];i+=1;}
  let status=object_query::tanks(&objects[0u16],objects.as_array(),ids,&q,&mut hit);
  let mut result=out;i=0;while i<18{result[i]=raw_hit[i];i+=1;}
status
}"#,
            include_str!("impact_case.rs"),
            include_str!("ground_response_case.rs"),
            include_str!("tank_response_case.rs"),
            include_str!("lifecycle_case.rs"),
            include_str!("crush_case.rs"),
            include_str!("spatial_case.rs"),
            include_str!("falling_case.rs"),
            include_str!("blast_case.rs"),
            include_str!("scene_world_case.rs"),
            include_str!("scene_contact_case.rs"),
            include_str!("fall_hit_case.rs"),
            include_str!("bounce_hit_case.rs"),
            include_str!("bounce_tick_case.rs"),
            include_str!("deform_contact_case.rs"),
            include_str!("scenery_contact_case.rs"),
            include_str!("entities_case.rs"),
            include_str!("scene_lifecycle_case.rs")
        ),
        output,
    )?;
    let sine = fs::read(fixture.join("sine.bin"))?;
    let angle = fs::read(fixture.join("angle.bin"))?;
    let cells = fs::read(fixture.join("collision-cells.bin"))?;
    let scene = if data["cases"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| matches!(c["kind"].as_u64(), Some(30 | 31 | 33)))
    {
        fs::read(fixture.join("car-deform-scene.bin"))?
    } else {
        Vec::new()
    };
    assert_eq!(cells.len(), 64 * 32768);
    let mut instructions = 0u64;
    let mut checked = 0usize;
    let mut runs = 0usize;
    let mut banked_world_cases = 0usize;
    for (index, case) in data["cases"]
        .as_array()
        .ok_or("collision cases")?
        .iter()
        .enumerate()
    {
        let layouts = match case["kind"].as_u64() {
            Some(23) => 3,
            Some(28) => 2,
            _ => 1,
        };
        for layout in 0..layouts {
            let mut cpu = CpuV3Sim::default();
            cpu.set_boot_window(&code.words);
            let memory = cpu.physical_memory_mut();
            if matches!(case["kind"].as_u64(), Some(27 | 31)) {
                for (i, pair) in cells.chunks_exact(2).enumerate() {
                    memory[0x100000 + i] = u16::from_le_bytes([pair[0], pair[1]]);
                }
            }
            if case["kind"].as_u64() == Some(30) {
                for (i, pair) in scene.chunks_exact(2).enumerate() {
                    memory[0x100000 + i] = u16::from_le_bytes([pair[0], pair[1]]);
                }
            }
            if matches!(case["kind"].as_u64(), Some(31 | 33)) {
                for (i, pair) in scene.chunks_exact(2).enumerate() {
                    memory[0x280000 + i] = u16::from_le_bytes([pair[0], pair[1]]);
                }
            }
            for (base, bytes) in [(0x3000, &sine), (0x8000, &angle)] {
                for (i, pair) in bytes.chunks_exact(2).enumerate() {
                    memory[base + i] = u16::from_le_bytes([pair[0], pair[1]]);
                }
            }
            memory[0x1ffd] = layout;
            memory[0x1ffe] = case["kind"].as_u64().unwrap() as u16;
            let values = case["input"].as_array().unwrap();
            for (i, v) in values.iter().enumerate() {
                memory[0x2000 + i * 2..0x2002 + i * 2]
                    .copy_from_slice(&words(v.as_i64().unwrap() as i32));
            }
            let expected = case["expected"].as_array().unwrap();
            for i in 0..expected.len() {
                memory[0x1000 + i * 2..0x1002 + i * 2].copy_from_slice(&words(0x12345678));
            }
            let mut passed = false;
            for step in 1..=1000000 {
                if let StepOutcome::Halted { signal } = cpu.step().expect("collision instruction") {
                    assert_eq!(signal, PASS, "case {index}, layout {layout}");
                    instructions += step;
                    passed = true;
                    break;
                }
            }
            assert!(passed, "collision case {index} exceeded bound");
            let memory = cpu.physical_memory_mut();
            assert_eq!(
                u64::from(memory[0x1fff]),
                case["status"].as_u64().unwrap(),
                "case {index}, layout {layout} status"
            );
            for (i, v) in expected.iter().enumerate() {
                let actual = i32::from_le_bytes([
                    memory[0x1000 + i * 2] as u8,
                    (memory[0x1000 + i * 2] >> 8) as u8,
                    memory[0x1001 + i * 2] as u8,
                    (memory[0x1001 + i * 2] >> 8) as u8,
                ]);
                assert_eq!(
                    i64::from(actual),
                    v.as_i64().unwrap(),
                    "case {index}, kind {}, field {i}",
                    case["kind"]
                );
                checked += 1;
            }
            runs += 1;
            if layout != 0 {
                banked_world_cases += 1;
            }
        }
    }
    Ok(
        json!({"cases":data["cases"].as_array().unwrap().len(),"executions":runs,"banked_world_cases":banked_world_cases,"checked_i32_values":checked,"maximum_integer_error":0,"instruction_steps":instructions,"code_bytes":code.total_code_words()*2,"scope":data["scope"],"boundary":"RCC modules with RAM trig and original ENGLAND terrain fixtures; instruction correctness, not normal loading or system timing"}),
    )
}
