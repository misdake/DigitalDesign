//! Exact inverse-basis and closing movement qualification through RCC.
use super::*;
pub fn run(output: &Path, fixture: &Path) -> Result<Value, Box<dyn Error>> {
    let data: Value = serde_json::from_slice(&fs::read(fixture.join("closer.json"))?)?;
    let code = compile(
        "closer",
        r#"
fn at(data:Array<u16>, words:i16)->Array<u16>{unsafe{data.as_ptr().add(words).as_u16_array()}}
fn copy(mut to:Array<u16>, from:Array<u16>, count:u16){let mut i=0u16;while i<count{to[i]=from[i];i+=1;}}
fn capture_list(out:Array<u16>, index:u16, list:&ordered::List){
 attitude::put(out,index,fix32::from_words(list.count,0));let mut i=0u16;
 while i<8{let mut value=integer::from_i16(-1);if i<list.count{value=attitude::get(list.items,i);}
 attitude::put(out,index+1+i,value);i+=1;}
}
fn main(){
 let input=unsafe{Ptr::from_addr(0x2000).as_u16_array()};let out=unsafe{Ptr::from_addr(0x1000).as_u16_array()};
 let sine=unsafe{Ptr::from_addr(0x3000).as_i16_array()};let cosine=unsafe{Ptr::from_addr(0x9000).as_i16_array()};
 let kind=attitude::get(input,0);let mut status=fix32::zero();
 if kind==fix32::zero(){
  if !inverse_basis::angles(at(out,2),at(input,2),sine,cosine){status=integer::from_i16(-2);}
 }else if kind==integer::from_i16(1){
  let state=at(input,20);
  closer::poll(state,attitude::get(input,1),attitude::get(input,2),at(input,8),at(input,14),attitude::get(input,3)!=fix32::zero());
  copy(at(out,2),state,6);
 }else{
  let state=at(input,8);let matrix=at(input,30);
  let flags:Buf<u16,32>=Buf::new([0;32]);copy(flags.as_array(),at(input,48),32);
  let notifications:Buf<u16,16>=Buf::new([0;16]);copy(notifications.as_array(),at(input,82),16);
  let destruction:Buf<u16,16>=Buf::new([0;16]);copy(destruction.as_array(),at(input,100),16);
  let order:Buf<u16,4>=Buf::new([1,0,2,0]);let slot:Buf<u16,2>=Buf::new([12,0]);let seed:Buf<u16,2>=Buf::new([0,0]);
  let empty:Buf<u16,2>=Buf::new([0;2]);
  let mut runtime=scene_world::Runtime{
   ticks:lifecycle::Ticks{flag_words:2,flags:flags.as_array(),flag_count:16,order:ordered::List{items:order.as_array(),count:2,capacity:2},next:integer::from_i16(2)},
   queues:lifecycle::Queues{
    notifications:ordered::List{items:notifications.as_array(),count:attitude::get(input,40).lo_bits(),capacity:8},
    destruction:ordered::List{items:destruction.as_array(),count:attitude::get(input,49).lo_bits(),capacity:8}},
   slot:slot.as_array(),seed:seed.as_array(),ordinary:ordered::List{items:empty.as_array(),count:0,capacity:0},special:ordered::List{items:empty.as_array(),count:0,capacity:0}};
  let step=closer::Step{dt:attitude::get(input,1),entity:attitude::get(input,2),limit:attitude::get(input,3)};
  let tables=closer::Tables{sine:sine,inverse_cosine:cosine};
  status=closer::advance(state,matrix,&mut runtime,&step,&tables);
  copy(at(out,2),state,6);copy(at(out,8),matrix,18);copy(at(out,26),flags.as_array(),32);
  capture_list(out,29,&runtime.queues.notifications);capture_list(out,38,&runtime.queues.destruction);
  attitude::put(out,47,attitude::get(runtime.slot,0));attitude::put(out,48,runtime.ticks.next);
  attitude::put(out,49,fix32::from_words(runtime.ticks.order.count,0));copy(at(out,100),runtime.ticks.order.items,4);
 }
 attitude::put(out,0,status);halt(0x600d);
}
"#,
        output,
    )?;
    let sine = fs::read(fixture.join("sine.i16.bin"))?;
    let cosine = fs::read(fixture.join("cosine-angle.bin"))?;
    assert_eq!(cosine.len(), 8192);
    let cases = data["cases"].as_array().ok_or("closer cases")?;
    let mut instructions = 0u64;
    let mut checked = 0usize;
    for (index, case) in cases.iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        let memory = cpu.physical_memory_mut();
        for (base, bytes) in [(0x3000, &sine), (0x9000, &cosine)] {
            for (i, pair) in bytes.chunks_exact(2).enumerate() {
                memory[base + i] = u16::from_le_bytes([pair[0], pair[1]]);
            }
        }
        let input = case["input"].as_array().ok_or("closer input")?;
        for (i, v) in input.iter().enumerate() {
            memory[INPUT + i * 2..INPUT + i * 2 + 2]
                .copy_from_slice(&words(v.as_i64().unwrap() as i32));
        }
        let expected = case["expected"].as_array().ok_or("closer expected")?;
        for i in 0..expected.len() {
            memory[RESULT + i * 2..RESULT + i * 2 + 2].copy_from_slice(&words(0x12345678));
        }
        let mut passed = false;
        for step in 1..=1000000 {
            if let StepOutcome::Halted { signal } = cpu.step().expect("closer instruction") {
                assert_eq!(signal, PASS, "closer case {index}");
                instructions += step;
                passed = true;
                break;
            }
        }
        assert!(passed, "closer case {index} exceeded bound");
        let memory = cpu.physical_memory_mut();
        for (i, v) in expected.iter().enumerate() {
            let actual =
                i32::from(memory[RESULT + i * 2]) | (i32::from(memory[RESULT + i * 2 + 1]) << 16);
            assert_eq!(
                i64::from(actual),
                v.as_i64().unwrap(),
                "closer case {index}, operation {}, field {i}",
                case["input"][0]
            );
            checked += 1;
        }
    }
    Ok(
        json!({"cases":cases.len(),"checked_i32_values":checked,"maximum_integer_error":0,"instruction_steps":instructions,"code_bytes":code.total_code_words()*2,"scope":data["scope"],"boundary":"RCC modules, RAM tables, CPU instruction backend; scene method dispatch and native loading remain separate"}),
    )
}
