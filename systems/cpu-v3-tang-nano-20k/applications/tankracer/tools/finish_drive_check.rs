//! RCC controls compared at every call against the original finish-owner path.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("finish-drive.json"))?)?;
    let code = compile(
        "finish-drive",
        r#"
mod finish_drive;
mod finish_routes;
fn ptr(w:u16)->Array<u16>{unsafe{Ptr::from_addr(w).as_u16_array()}}
fn at(v:Array<u16>,w:u16)->Array<u16>{unsafe{v.as_ptr().add(w as i16).as_u16_array()}}
fn g(v:Array<u16>,i:u16)->fix32{attitude::get(v,i)}
fn p(v:Array<u16>,i:u16,x:fix32){attitude::put(v,i,x);}
fn main(){
 let param=ptr(0x1100);let capacity=ptr(0x1ff4)[0u16];let count=ptr(0x1ff2)[0u16];
 let ids=entities::State{header:ptr(0x4000),rows:ptr(0x4100),row_words:4,storage_capacity:capacity};
 let empty=ordered::List{items:ptr(0x1f00),count:0,capacity:0};
 let grid=spatial::Grid{page:65535,origin:ptr(0x1f00),shift_x:0,shift_z:0,items:ptr(0x1f00),ranges:ptr(0x1f00)};
 let banked=ptr(0x1ff8)[0u16]!=0;
 let mut points=ptr(0x2000);let mut indices=ptr(0x1a00);let mut config_values=ptr(0x1200);
 let mut collider_rows=ptr(0x3000);let mut collider_page=65535u16;
 if banked{unsafe{write_dseg_page(2,20);}points=ptr(0x8100);indices=ptr(0x8010);
  config_values=ptr(0x8418);collider_rows=ptr(0x8000);collider_page=21;}
 let world=scene_world::World{colliders:collider_rows,properties:ptr(0x1f00),count:count,
  fixed_count:count,dynamic_page:0,dynamic_capacity:0,grid:grid,deformables:empty};
 let routes=finish_drive::Routes{indices:indices,points:points,count:count,
  point_count:ptr(0x1ff6)[0u16],collider_page:collider_page};
 let race=player_route::Car{route:ptr(0x1300),race:ptr(0x1320)};
 let config=finish_drive::Config{values:config_values,angle:unsafe{Ptr::from_addr(0x6400).as_i16_array()}};
 if banked{
  if finish_drive::load_collider(&routes,&world,count,ptr(0x1900)){halt(0x7f92);}
  if read_dseg_page(2)!=20{halt(0x7f93);}
  if finish_drive::next(&routes,count,0)!=integer::from_i16(-4){halt(0x7f94);}
  let mut gap=0u16;while gap<count&&routes.indices[gap]!=65535{gap+=1;}
  if gap==count||finish_drive::next(&routes,gap,0)!=integer::from_i16(-4){halt(0x7f95);}
 }
 let steps=ptr(0x1ff0)[0u16];let mut i=0u16;
 while i<steps{
  let car=finish_drive::Car{present:g(param,4)!=fix32::zero(),state:ptr(0x1000),controls:ptr(0x1080),
   position:ptr(0x10a0),velocity:ptr(0x10b0),reference:ptr(0x10c0),local_speed:g(param,1),
   horn:ptr(0x1340),laps:g(race.race,1).lo_bits() as i16,visible:g(param,3)!=fix32::zero()};
  let frame=finish_drive::Frame{dt:g(ptr(0x1700),i),seed:ptr(0x1140)};
  let op=g(param,0).lo_bits();let mut result=fix32::zero();
  if op==0{result=finish_drive::throttle(&car,&routes,&ids,&config,&frame);}
  if op==1{result=finish_drive::steer(&car,&routes,&ids,&world,&config,&frame);}
  if op==2{result=finish_drive::reverse(&car,&routes,&ids,&world,&config,&frame);}
  if op==3{result=finish_drive::branch(&routes,&ids,g(param,5),car.laps,frame.seed);}
  if op==4{result=finish_routes::waypoints(&car,&race,&routes,&ids,&world,&frame);}
  if op==5{result=finish_drive::controls(&car,&routes,&ids,&world,&config,&frame);}
  if op==6{result=finish_drive::controls(&car,&routes,&ids,&world,&config,&frame);
   if result==fix32::zero(){result=finish_routes::waypoints(&car,&race,&routes,&ids,&world,&frame);}}
  let out=ptr(0x1800);p(out,0,result);let mut j=0u16;
  while j<30{p(out,1+j,g(car.state,j));j+=1;}
  j=0;while j<3{p(out,31+j,g(car.controls,j));j+=1;}p(out,34,g(frame.seed,0));
  j=0;while j<11{p(out,35+j,g(race.route,j));j+=1;}
  j=0;while j<3{p(out,46+j,g(race.race,j));j+=1;}
  p(out,49,g(car.horn,0));
  i+=1;unsafe{Ptr::from_addr(0x1fee).write(0,read_dseg_page(2));Ptr::from_addr(0xff).write(0,i);}
 }
 halt(0x600d);
}
"#,
        output,
    )?;
    let angle = fs::read(fixture.join("angle.i16.bin"))?;
    assert_eq!(angle.len(), 514);
    let cases = data["cases"].as_array().ok_or("finish drive cases")?;
    let mut calls = 0usize;
    let mut checked = 0usize;
    let mut steps = 0u64;
    let mut minimum_stack = u16::MAX;
    for banked in [false, true] {
        for (index, case) in cases.iter().enumerate() {
            let mut cpu = CpuV3Sim::default();
            cpu.set_boot_window(&code.words);
            let memory = cpu.physical_memory_mut();
            let mut immutable = Vec::new();
            for (name, address, raw) in [
                ("state", 0x1000, false),
                ("controls", 0x1080, false),
                ("position", 0x10a0, false),
                ("velocity", 0x10b0, false),
                ("reference", 0x10c0, false),
                ("params", 0x1100, false),
                ("config", 0x1200, false),
                ("route", 0x1300, false),
                ("race", 0x1320, false),
                ("horn", 0x1340, false),
                ("dt", 0x1700, false),
                ("points", 0x2000, false),
                ("colliders", 0x3000, false),
                ("ids", 0x4100, true),
            ] {
                let values = case[name].as_array().ok_or(format!("missing {name}"))?;
                let mut packed = Vec::new();
                for value in values {
                    if raw {
                        packed.push(value.as_u64().unwrap() as u16);
                    } else {
                        packed.extend(words(value.as_i64().unwrap() as i32));
                    }
                }
                memory[address..address + packed.len()].copy_from_slice(&packed);
                let guard = address + packed.len();
                memory[guard..guard + 4].fill(0x7acb);
                if name != "state"
                    && name != "controls"
                    && name != "route"
                    && name != "race"
                    && name != "horn"
                {
                    immutable.push((address, packed));
                }
                immutable.push((guard, vec![0x7acb; 4]));
            }
            memory[0x1140..0x1142].copy_from_slice(&words(case["seed"].as_u64().unwrap() as i32));
            memory[0x4000..0x4002]
                .copy_from_slice(&words(case["capacity"].as_u64().unwrap() as i32));
            memory[0x1ff0] = case["dt"].as_array().unwrap().len() as u16;
            memory[0x1ff2] = case["count"].as_u64().unwrap() as u16;
            memory[0x1ff4] = case["capacity"].as_u64().unwrap() as u16;
            let point_count = case["count"].as_u64().unwrap() as usize;
            memory[0x1ff6] = point_count as u16;
            let mapping: Vec<u16> = (0..point_count as u16).collect();
            memory[0x1a00..0x1a00 + point_count].copy_from_slice(&mapping);
            immutable.push((0x1a00, mapping));
            if banked {
                let globals = case["object_ids"]
                    .as_array()
                    .ok_or("missing global object IDs")?;
                let count = case["world_count"].as_u64().unwrap() as usize;
                assert_eq!(count, 240);
                assert_eq!(point_count, 36);
                assert_eq!(globals.len(), point_count);
                let asset = 20usize * 16384;
                let colliders = 21usize * 16384;
                memory[asset + 16..asset + 256].fill(65535);
                for (compact, id) in globals.iter().enumerate() {
                    let id = id.as_u64().unwrap() as usize;
                    memory[asset + 16 + id] = compact as u16;
                    let row = memory[0x3000 + compact * 60..0x3000 + (compact + 1) * 60].to_vec();
                    memory[colliders + id * 60..colliders + (id + 1) * 60].copy_from_slice(&row);
                }
                let points = memory[0x2000..0x2000 + point_count * 22].to_vec();
                memory[asset + 256..asset + 256 + points.len()].copy_from_slice(&points);
                let config = memory[0x1200..0x1200 + 124].to_vec();
                memory[asset + 1048..asset + 1172].copy_from_slice(&config);
                let mut ids =
                    memory[0x4100..0x4100 + case["ids"].as_array().unwrap().len()].to_vec();
                for row in ids.chunks_exact_mut(4) {
                    if row[0] == 5 {
                        row[2] = globals[row[2] as usize].as_u64().unwrap() as u16;
                    }
                }
                memory[0x4100..0x4100 + ids.len()].copy_from_slice(&ids);
                // Identity contents change with layout; guards and all other
                // readonly records retain the same verification.
                for (address, values) in &mut immutable {
                    if *address == 0x4100 {
                        *values = ids.clone();
                    }
                }
                immutable.push((asset + 16, memory[asset + 16..asset + 1172].to_vec()));
                immutable.push((
                    colliders,
                    memory[colliders..colliders + count * 60].to_vec(),
                ));
                memory[0x1ff2] = count as u16;
                memory[0x1ff8] = 1;
            }
            assert!(0x4100 + case["ids"].as_array().unwrap().len() + 4 <= 0x6400);
            for i in 0..257 {
                memory[0x6400 + i] = u16::from_le_bytes([angle[i * 2], angle[i * 2 + 1]]);
            }
            immutable.push((0x6400, memory[0x6400..0x6501].to_vec()));
            let expected = case["frames"].as_array().unwrap();
            let mut done = 0;
            let mut passed = false;
            let mut started = false;
            for step in 1..=2_000_000 {
                let result = cpu
                    .step()
                    .map_err(|e| format!("finish-drive {index} {}: {e:?}", case["tag"]))?;
                let sp = cpu.registers()[13];
                if started || sp != 0 {
                    started = true;
                    minimum_stack = minimum_stack.min(sp);
                }
                let memory = cpu.physical_memory_slice();
                if usize::from(memory[0xff]) > done {
                    assert_eq!(usize::from(memory[0xff]), done + 1);
                    let want = expected[done].as_array().unwrap();
                    assert_eq!(want.len(), 50);
                    for (field, v) in want.iter().enumerate() {
                        let offset = 0x1800 + field * 2;
                        let actual = &memory[offset..offset + 2];
                        let wanted = words(v.as_i64().unwrap() as i32);
                        if actual != wanted {
                            return Err(format!("finish-drive {index} {} call {done} field {field}: {actual:?} != {wanted:?}",case["tag"]).into());
                        }
                        checked += 2;
                    }
                    for (address, values) in &immutable {
                        if &memory[*address..*address + values.len()] != values {
                            return Err(format!(
                                "finish-drive {index} {} changed readonly/guard {address:04x}",
                                case["tag"]
                            )
                            .into());
                        }
                    }
                    assert_eq!(memory[0x1fee], if banked { 20 } else { 2 }, "DSEG2 restore");
                    assert_eq!(&memory[0x1000..0x103c], &memory[0x1802..0x183e]);
                    assert_eq!(&memory[0x1080..0x1086], &memory[0x183e..0x1844]);
                    assert_eq!(&memory[0x1300..0x1316], &memory[0x1846..0x185c]);
                    assert_eq!(&memory[0x1320..0x1326], &memory[0x185c..0x1862]);
                    done += 1;
                    calls += 1;
                }
                if let StepOutcome::Halted { signal } = result {
                    assert_eq!(signal, PASS);
                    assert_eq!(done, expected.len());
                    passed = true;
                    steps += step;
                    break;
                }
            }
            if !passed {
                return Err(format!("finish-drive {index} instruction bound").into());
            }
        }
    }
    let report = json!({"cases":cases.len(),"layouts":["resident compact IDs","banked global IDs"],"calls":calls,"checked_u16_values":checked,
        "maximum_integer_error":0,"code_bytes":code.words.len()*2,"steps":steps,
        "maximum_stack_bytes":(0x8000usize-usize::from(minimum_stack))*2,
        "scope":"Original finish-owner controls and ordered look/race/drive checkpoints in resident and banked layouts"});
    fs::write(
        output.join("finish-drive-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
