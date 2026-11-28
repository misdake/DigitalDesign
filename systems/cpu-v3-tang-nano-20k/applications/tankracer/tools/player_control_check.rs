//! Direct input/control views compared after each original branch or prefix.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("player-control.json"))?)?;
    let code = compile(
        "player-control",
        r#"
fn at(v:Array<u16>,w:u16)->Array<u16>{unsafe{v.as_ptr().add(w as i16).as_u16_array()}}
fn g(v:Array<u16>,i:u16)->fix32{attitude::get(v,i)}
fn p(v:Array<u16>,i:u16,x:fix32){attitude::put(v,i,x);}
fn ptr(w:u16)->Array<u16>{unsafe{Ptr::from_addr(w).as_u16_array()}}
fn copy(mut d:Array<u16>,s:Array<u16>,n:u16){let mut i=0u16;while i<n{d[i]=s[i];i+=1;}}
fn turret(s:Array<u16>,c:Array<u16>){
 let angle=at(s,36);
 player_control::turret(at(s,22),angle,(g(c,11).lo_bits()>>5)&3,g(c,1));

}
fn main(){
 let s=unsafe{Ptr::from_addr(0x1000).as_u16_array()};let t=unsafe{Ptr::from_addr(0x1100).as_u16_array()};
 let out=unsafe{Ptr::from_addr(0x1800).as_u16_array()};let count=unsafe{Ptr::from_addr(0x1ff0).read(0)};
 let controls=unsafe{Ptr::from_addr(0x2000).as_u16_array()};
 let input=player_control::Input{drive:s,state:ptr(0x1600),countdown:at(s,18),analog_latch:at(s,20)};
 let brake=player_control::Brake{saved_turn:ptr(0x1620),reverse_braking:ptr(0x1640),drive:s,max_turn:at(s,42),yaw_lock:at(s,44)};
 let collision=player_control::Collision{counters:at(s,46),ground_delta:at(s,50),tank_delta:at(s,56),obstacle_impulse:at(s,62)};
 copy(ptr(0x1600),at(s,14),4);copy(ptr(0x1620),at(s,38),2);copy(ptr(0x1640),at(s,40),2);
 let mut frame=0u16;
 while frame<count{
  let c=at(controls,frame*40);let op=g(c,0).lo_bits();let mut result=0u16;
  if op==0 || op==5{result=player_control::input(&input,at(c,2),t,g(c,11).lo_bits(),at(c,24));}
  if op==1{turret(s,c);}
  if op==4 || (op==5 && result&1!=0){player_control::collision_decay(&collision,g(c,1),g(c,14)!=fix32::zero());}
  if op==5 && result&1!=0{p(s,15,g(c,19));turret(s,c);}
  if op==2 || op==3 || (op==5 && result&1!=0){
   let f:Buf<u16,10>=Buf::new([0;10]);let v=f.as_array();let reference:Buf<u16,4>=Buf::new([0;4]);let r=reference.as_array();
   p(v,0,g(c,1));p(v,1,g(c,4));p(v,2,g(c,15));p(v,3,g(c,16));
   let mut down=0u16;if g(c,11).lo_bits()&16!=0{down=1;}if op==5{down=(result>>1)&1;}
   p(v,4,fix32::from_words(down,0));p(r,0,g(c,17));p(r,1,g(c,18));
   if op==3{player_control::restore(&brake,g(c,1),g(c,15),r);}else{player_control::brake(&brake,v,r);}
  }
  copy(at(s,14),ptr(0x1600),4);copy(at(s,38),ptr(0x1620),2);copy(at(s,40),ptr(0x1640),2);
  p(out,0,fix32::from_words(result,0));let mut dest=at(out,2);let mut i=0u16;while i<64{dest[i]=s[i];i+=1;}
  unsafe{Ptr::from_addr(0xff).write(0,frame+1);}frame+=1;
 }
 halt(0x600d);
}
"#,
        output,
    )?;
    let cases = data["cases"].as_array().ok_or("player control cases")?;
    let mut checked = 0;
    let mut calls = 0;
    let mut steps = 0u64;
    let mut minimum_stack = u16::MAX;
    for (index, case) in cases.iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        cpu.physical_memory_mut()[0x1040..0x1080].fill(0x79ab);
        cpu.physical_memory_mut()[0x1842..0x1882].fill(0x79ab);
        cpu.physical_memory_mut()[0x1600..0x1660].fill(0x79ab);
        let input = case["input"].as_array().ok_or("player control input")?;
        assert_eq!(input.len(), 64);
        for (i, v) in input.iter().enumerate() {
            cpu.physical_memory_mut()[0x1000 + i] = v.as_u64().unwrap() as u16;
        }
        let tuning = case["tuning"].as_array().ok_or("player control tuning")?;
        assert_eq!(tuning.len(), 6);
        for (i, v) in tuning.iter().enumerate() {
            cpu.physical_memory_mut()[0x1100 + i * 2..0x1102 + i * 2]
                .copy_from_slice(&words(v.as_i64().unwrap() as i32));
        }
        let controls = case["controls"].as_array().unwrap();
        assert!(controls.len() <= 32);
        cpu.physical_memory_mut()[0x1ff0] = controls.len() as u16;
        for (frame, c) in controls.iter().enumerate() {
            assert_eq!(c.as_array().unwrap().len(), 20);
            for (i, v) in c.as_array().unwrap().iter().enumerate() {
                let offset = 0x2000 + frame * 40 + i * 2;
                cpu.physical_memory_mut()[offset..offset + 2]
                    .copy_from_slice(&words(v.as_i64().unwrap() as i32));
            }
        }
        let expected = case["frames"].as_array().unwrap();
        let mut done = 0;
        let mut passed = false;
        let mut started = false;
        for step in 1..=1000000 {
            let result = cpu
                .step()
                .map_err(|e| format!("player-control case {index}: {e:?}"))?;
            let sp = cpu.registers()[13];
            if started || sp != 0 {
                started = true;
                minimum_stack = minimum_stack.min(sp);
            }
            let memory = cpu.physical_memory_slice();
            if usize::from(memory[0xff]) > done {
                assert_eq!(usize::from(memory[0xff]), done + 1);
                let want = expected[done].as_array().unwrap();
                assert_eq!(want.len(), 66);
                for (field, v) in want.iter().enumerate() {
                    let actual = memory[0x1800 + field];
                    let wanted = v.as_u64().unwrap() as u16;
                    if actual != wanted {
                        return Err(format!("player-control case {index} {} call {done} word {field}: {actual} != {wanted}",case["tag"]).into());
                    }
                    checked += 1;
                }
                assert_eq!(&memory[0x1000..0x1040], &memory[0x1802..0x1842]);
                for (i, v) in tuning.iter().enumerate() {
                    assert_eq!(
                        &memory[0x1100 + i * 2..0x1102 + i * 2],
                        &words(v.as_i64().unwrap() as i32)
                    );
                }
                for address in [0x1040, 0x1842] {
                    assert!(memory[address..address + 64].iter().all(|v| *v == 0x79ab));
                }
                for address in [0x1604, 0x1622, 0x1642] {
                    assert!(memory[address..address + 4].iter().all(|v| *v == 0x79ab));
                }
                for (frame, c) in controls.iter().enumerate() {
                    for (i, v) in c.as_array().unwrap().iter().enumerate() {
                        let offset = 0x2000 + frame * 40 + i * 2;
                        assert_eq!(
                            &memory[offset..offset + 2],
                            &words(v.as_i64().unwrap() as i32)
                        );
                    }
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
            return Err(format!("player-control case {index}: instruction bound").into());
        }
    }
    let report = json!({"cases":cases.len(),"calls":calls,"checked_u16_values":checked,"maximum_integer_error":0,
        "instruction_steps":steps,"code_bytes":code.total_code_words()*2,"minimum_stack_word":minimum_stack,
        "qualified_state_bytes":128,"independent_field_guard_words":12,"maximum_stack_bytes":(0x8000usize-usize::from(minimum_stack))*2,
        "scope":data["scope"],"backend":"instruction, sampled device values without GPU competition"});
    fs::write(
        output.join("player-control-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
