//! Projectile/car response and private geometry checked after every contact.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("tank-hit.json"))?)?;
    let code = compile(
        "tank-hit",
        r#"
fn at(v:Array<u16>,w:u16)->Array<u16>{unsafe{v.as_ptr().add(w as i16).as_u16_array()}}
fn g(v:Array<u16>,i:u16)->fix32{attitude::get(v,i)}
fn copy(mut to:Array<u16>,from:Array<u16>,count:u16){let mut i=0u16;while i<count{to[i]=from[i];i+=1;}}
fn part(input:Array<u16>,data:Array<u16>,r:Array<u16>,upper:u16)->tank_hit::Part{
 let b=21+upper*6;let pose=33+upper*24;
 tank_hit::Part{present:r[b]!=0,revision:at(data,55+upper*4),
 target:damage::Target{state:at(data,45),player:true,basis:at(r,pose),position:at(r,pose+18),selected:at(r,b+2),
 mesh:damage::Mesh{present:r[b+1]!=0,vertices:unsafe{at(input,input[1+upper*2]).as_ptr().as_i16_array()},vertex_count:input[6+upper*2],
 indices:at(input,input[10+upper]),normals:unsafe{at(input,input[2+upper*2]).as_ptr().as_i16_array()},face_count:input[7+upper*2]}}}
}
fn main(){
 let input=unsafe{Ptr::from_addr(0x0100).as_u16_array()};let out=unsafe{Ptr::from_addr(0x2000).as_u16_array()};let data=at(input,16);let r=at(input,input[5u16]);
 let prefix=at(input,input[12u16]);let temp=at(input,input[13u16]);
 let shared=temporary::State{core:temp,rows:at(temp,82)};
 let pools:Buf<effect_pool::Pool,1>=Buf::new([effect_pool::Pool{page:65535,lines:at(temp,278),count:g(at(temp,276),0),overflow:effect_pool::line_zero(),exhausted:at(temp,1894)}]);
 let mut pool_rows=pools.as_array();let mut pool=&mut pool_rows[0u16];
 copy(unsafe{view_of(&mut pool.overflow).as_ptr().as_u16_array()},at(temp,1878),16);
 let sine:Buf<u16,4>=Buf::new([45056,320,40960,0]);
 let ctx=temporary::Context{pool:pools.as_array(),seed:at(temp,274),sine:unsafe{sine.as_array().as_ptr().as_i16_array()}};
 let parts:Buf<tank_hit::Part,2>=Buf::new([part(input,data,r,0),part(input,data,r,1)]);
 let car=tank_hit::Vehicle{player:r[0u16]!=0,visible:r[1u16]!=0,inventory:at(data,25),timer:at(data,21),velocity:at(data,9),angular:at(data,15),external:at(r,3),has_external:r[2u16]!=0,collision_size:at(data,23),hit_counts:at(data,29),smoke:at(data,41),parts:parts.as_array(),base_position:at(r,106),grip:prefix,temporary:at(prefix,8)};
 let length=unsafe{Ptr::from_addr(0xfe).read(0)};let count=input[length];let mut frame=0u16;
 while frame<count{
  let c=at(input,length+1+frame*36);let mut hit=projectile_sweep::zero();
  copy(hit.world.point.as_array(),at(c,6),6);copy(hit.world.normal.as_array(),at(c,12),6);
  copy(hit.local.point.as_array(),at(c,18),6);copy(hit.local.normal.as_array(),at(c,24),6);
  tank_hit::coefficients(data,g(c,0).lo_bits(),car.player,g(c,2).lo_bits(),at(r,81),at(r,98));
  let mut status=fix32::zero();
  if input[14u16]!=0{
   let shot=tank_hit::Shot{hit:view_of(&mut hit),velocity:at(c,30),kind:g(c,0).lo_bits(),age:g(c,1),time:g(c,1)};
   status=tank_hit::apply(&car,&shot,data,&shared,&ctx);
  }else{status=tank_hit::contact(&car,&hit,at(c,30),g(c,1),data);}
  pool_rows=pools.as_array();pool=&mut pool_rows[0u16];
  attitude::put(at(temp,276),0,pool.count);copy(at(temp,1878),unsafe{view_of(&mut pool.overflow).as_ptr().as_u16_array()},16);
  attitude::put(out,0,status);copy(at(out,2),data,input[0u16]);
  unsafe{Ptr::from_addr(0xff).write(0,frame+1);}frame+=1;
 }
 halt(0x600d);
}
"#,
        output,
    )?;
    let sine = fs::read(fixture.join("sine.i16.bin"))?;
    let cases = data["cases"].as_array().ok_or("tank hit cases")?;
    let mut checked = 0;
    let mut calls = 0;
    let mut steps = 0u64;
    let mut minimum_stack = u16::MAX;
    for (index, case) in cases.iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        for (i, pair) in sine.chunks_exact(2).enumerate() {
            cpu.physical_memory_mut()[0x2c0000 + i] = u16::from_le_bytes([pair[0], pair[1]]);
        }
        let input = case["input"].as_array().ok_or("tank hit input")?;
        assert!(input.len() < 0x1e00);
        cpu.physical_memory_mut()[0xfe] = input.len() as u16;
        for (i, v) in input.iter().enumerate() {
            cpu.physical_memory_mut()[0x100 + i] = v.as_u64().unwrap() as u16;
        }
        let controls = case["controls"].as_array().ok_or("tank hit controls")?;
        cpu.physical_memory_mut()[0x100 + input.len()] = controls.len() as u16;
        for (frame, c) in controls.iter().enumerate() {
            assert_eq!(c.as_array().unwrap().len(), 18);
            for (i, v) in c.as_array().unwrap().iter().enumerate() {
                let at = 0x100 + input.len() + 1 + frame * 36 + i * 2;
                cpu.physical_memory_mut()[at..at + 2]
                    .copy_from_slice(&words(v.as_i64().unwrap() as i32));
            }
        }
        assert!(input.len() + 1 + controls.len() * 36 < 0x1f00);
        let expected = case["frames"].as_array().unwrap();
        let mut done = 0;
        let mut passed = false;
        let mut started = false;
        for step in 1..=20000000 {
            let result = cpu
                .step()
                .map_err(|e| format!("tank hit case {index}: {e:?}"))?;
            let sp = cpu.registers()[13];
            if started || sp != 0 {
                started = true;
                minimum_stack = minimum_stack.min(sp);
            }
            let memory = cpu.physical_memory_slice();
            if usize::from(memory[0xff]) > done {
                assert_eq!(usize::from(memory[0xff]), done + 1);
                for (field, v) in expected[done].as_array().unwrap().iter().enumerate() {
                    let actual = memory[0x2000 + field];
                    let want = v.as_u64().unwrap() as u16;
                    if actual != want {
                        return Err(format!(
                            "tank hit case {index} {} call {done} word {field}: {actual} != {want}",
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
            return Err(format!("tank hit case {index}: instruction bound").into());
        }
    }
    let report = json!({"cases":cases.len(),"calls":calls,"checked_u16_values":checked,"maximum_integer_error":0,
        "instruction_steps":steps,"code_bytes":code.total_code_words()*2,"minimum_stack_word":minimum_stack,"scope":data["scope"],
        "backend":"instruction; direct state and private model geometry, without GPU competition"});
    fs::write(
        output.join("tank-hit-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
