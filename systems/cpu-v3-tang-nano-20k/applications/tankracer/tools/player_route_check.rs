//! Live map/identity route queries with direct and phase-banked metadata views.
use super::*;
fn values(v: &Value) -> Vec<u16> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_u64().unwrap() as u16)
        .collect()
}
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("player-route.json"))?)?;
    let code = compile(
        "player-route",
        r#"
fn ptr(a:u16)->Array<u16>{unsafe{Ptr::from_addr(a).as_u16_array()}}
fn at(v:Array<u16>,w:u16)->Array<u16>{unsafe{v.as_ptr().add(w as i16).as_u16_array()}}
fn g(v:Array<u16>,i:u16)->fix32{attitude::get(v,i)}
fn p(v:Array<u16>,i:u16,x:fix32){attitude::put(v,i,x);}
fn main(){
 let s=ptr(0x100);let meta=ptr(0x180);let controls=ptr(0x4000);let out=ptr(0x5000);
 let mut base=ptr(0x2000);if meta[3u16]!=0{base=ptr(0xc000);}
 let routes=player_route::Routes{header:base,points:at(base,0x100),count:meta[2u16],ranges:at(base,0x1000),items:at(base,0x1100)};
 let ids=entities::State{row_words:6,header:ptr(0x1000),rows:ptr(0x100c),storage_capacity:512};
 let world=scene_world::World{colliders:ptr(0x8000),properties:ptr(0),count:meta[1u16],fixed_count:meta[1u16],dynamic_page:121,dynamic_capacity:0,grid:spatial::Grid{page:65535,origin:ptr(0),shift_x:0,shift_z:0,items:ptr(0),ranges:ptr(0)},deformables:ordered::List{items:ptr(0),count:0,capacity:0}};
 let car=player_route::Car{route:s,race:at(s,22)};let result=at(s,46);let mut frame=0u16;
 unsafe{write_dseg_page(2,120);write_dseg_page(3,119);}
 while frame<meta[0u16]{
  let c=at(controls,frame*20);let op=g(c,0).lo_bits();let position=at(c,4);let mut status=fix32::zero();
  if meta[3u16]!=0{unsafe{write_dseg_page(3,135);}}
  if op==0{status=player_route::cross(&routes,&world,&ids,&car,position,result);}
  else if op==1{status=player_route::progress(&routes,&world,&ids,&car,&player_route::Frame{position,entity:g(c,5),number:g(c,6),dt:g(c,1),limit:g(c,7)});}
  else if op==2{status=player_route::search(&routes,&world,&ids,&player_route::Query{position,current:fix32::zero(),lap:fix32::zero(),limit:g(c,7),nearest:true,result});}
  else if op==3{p(s,7,g(c,8));player_route::warning(&car,g(c,5),g(c,6),g(c,1));}
  else if op==4{player_route::clock(at(s,40),at(s,22),3,g(c,1));p(s,25,player_route::physics_dt(g(c,1)));}
  let mut wanted=119u16;if meta[3u16]!=0{wanted=135;}if read_dseg_page(2)!=120 || read_dseg_page(3)!=wanted{halt(0x7e8d);}
  unsafe{write_dseg_page(3,119);}
  p(out,0,status);let mut dest=at(out,2);let mut i=0u16;while i<52{dest[i]=s[i];i+=1;}
  unsafe{Ptr::from_addr(0xff).write(0,frame+1);}frame+=1;
 }
 halt(0x600d);
}
"#,
        output,
    )?;
    let cases = data["cases"].as_array().unwrap();
    let mut calls = 0;
    let mut checked = 0;
    let mut instructions = 0u64;
    let mut minimum_stack = u16::MAX;
    let mut route_bytes = 0;
    for (index, case) in cases.iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        let memory = cpu.physical_memory_mut();
        let banked = case["banked"].as_bool().unwrap();
        let base = if banked { 135 * 16384 } else { 0x2000 };
        memory[base..base + 0x1800].fill(0x79ab);
        memory[0x134..0x174].fill(0x79ab);
        memory[0x5036..0x5076].fill(0x79ab);
        let placements = [
            (0x100, "state"),
            (0x1000, "identity"),
            (base, "header"),
            (base + 0x100, "points"),
            (base + 0x1000, "ranges"),
            (base + 0x1100, "items"),
            (120 * 16384, "colliders"),
        ];
        let mut bytes = 0;
        for (address, name) in placements {
            let v = values(&case[name]);
            let capacity = match name {
                "state" => 52,
                "identity" => 12 + 512 * 6,
                "header" => 18,
                "points" => 0xf00,
                "ranges" => 128,
                "items" => 0x700,
                _ => 16384,
            };
            assert!(v.len() <= capacity, "route {name}");
            memory[address..address + v.len()].copy_from_slice(&v);
            if matches!(name, "header" | "points" | "ranges" | "items") {
                bytes += v.len() * 2;
            }
        }
        route_bytes = route_bytes.max(bytes);
        let colliders = values(&case["colliders"]);
        memory[120 * 16384 + colliders.len()..121 * 16384].fill(0x79ab);
        let controls = case["controls"].as_array().unwrap();
        memory[0x180] = controls.len() as u16;
        memory[0x181] = (colliders.len() / 60) as u16;
        memory[0x182] = (values(&case["points"]).len() / 14) as u16;
        memory[0x183] = u16::from(banked);
        let route_image = memory[base..base + 0x1800].to_vec();
        for (frame, c) in controls.iter().enumerate() {
            assert_eq!(c.as_array().unwrap().len(), 10);
            for (i, v) in c.as_array().unwrap().iter().enumerate() {
                let address = 0x4000 + frame * 20 + i * 2;
                memory[address..address + 2].copy_from_slice(&words(v.as_i64().unwrap() as i32));
            }
        }
        let expected = case["frames"].as_array().unwrap();
        let mut done = 0;
        let mut passed = false;
        let mut started = false;
        for step in 1..=5000000 {
            let result = cpu
                .step()
                .map_err(|e| format!("player-route case {index}: {e:?}"))?;
            let sp = cpu.registers()[13];
            if started || sp != 0 {
                started = true;
                minimum_stack = minimum_stack.min(sp);
                assert!(sp >= 0x7000);
            }
            let memory = cpu.physical_memory_slice();
            if usize::from(memory[0xff]) > done {
                assert_eq!(usize::from(memory[0xff]), done + 1);
                let want = values(&expected[done]);
                assert_eq!(want.len(), 54);
                for (word, &v) in want.iter().enumerate() {
                    let actual = memory[0x5000 + word];
                    if actual != v {
                        return Err(format!(
                            "player-route case {index} {} call {done} word {word}: {actual} != {v}",
                            case["tag"]
                        )
                        .into());
                    }
                    checked += 1;
                }
                for (address, name) in placements {
                    if name != "state" {
                        let want = values(&case[name]);
                        assert_eq!(
                            &memory[address..address + want.len()],
                            want.as_slice(),
                            "route immutable {name}"
                        );
                    }
                }
                for address in [0x134, 0x5036] {
                    assert!(memory[address..address + 64].iter().all(|v| *v == 0x79ab));
                }
                assert_eq!(&memory[base..base + 0x1800], route_image.as_slice());
                for (frame, c) in controls.iter().enumerate() {
                    for (i, v) in c.as_array().unwrap().iter().enumerate() {
                        let address = 0x4000 + frame * 20 + i * 2;
                        assert_eq!(
                            &memory[address..address + 2],
                            &words(v.as_i64().unwrap() as i32)
                        );
                    }
                }
                assert!(
                    memory[120 * 16384 + colliders.len()..121 * 16384]
                        .iter()
                        .all(|v| *v == 0x79ab)
                );
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
            return Err(format!("player-route case {index}: instruction bound").into());
        }
    }
    let report = json!({"cases":cases.len(),"calls":calls,"checked_u16_values":checked,"maximum_integer_error":0,"instruction_steps":instructions,"code_bytes":code.total_code_words()*2,"maximum_stack_bytes":(0x8000-usize::from(minimum_stack))*2,"minimum_stack_word":minimum_stack,"route_metadata_bytes":route_bytes,"player_route_bytes":44,"racer_bytes":12,"clock_bytes":12,"scope":data["scope"],"backend":"instruction, live ENGLAND collider/identity/route views without GPU competition"});
    fs::write(
        output.join("player-route-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
