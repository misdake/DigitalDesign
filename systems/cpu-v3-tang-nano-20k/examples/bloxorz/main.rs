//! Bounded CPU rule replay; tests also cover geometry and separate native GPU replay.
use cpu_v3::{rcc_backend, CpuV3Sim, PhysicalWordAddress, RunOutcome};
#[cfg(test)]
mod batch_scene;
mod geometry;
#[cfg(test)]
mod rcc_geometry;
#[cfg(test)]
mod static_scene;

const PROGRAM: &str = r#"
mod bloxorz_core;
static STATE:Buf<u16,10>=Buf::new([0;10]);
static HISTORY:Buf<u16,1536>=Buf::new([0;1536]);
static HS:Buf<u16,2>=Buf::new([0;2]);
fn main(){
 let level=unsafe{Ptr::from_addr(0x5000).as_u16_array()};
 let input=Ptr::from_addr(0x6000);let output=Ptr::from_addr(0x8000);
 blox_reset(level,STATE.as_array(),HS.as_array());
 let count=unsafe{input.read(0)};
 if count>512 {halt(0xbad);}
 let mut i:u16=0;
 while i<count {
   let command=unsafe{input.read((i+1) as i16)};
   let accepted=blox_dispatch(level,STATE.as_array(),HISTORY.as_array(),HS.as_array(),command);
   unsafe{output.write((i*7) as i16,if accepted{1}else{0});}
   blox_pack(STATE.as_array(),unsafe{output.as_u16_array()},i*7+1);
   i+=1;
 }
 halt(0x600d);
}
"#;

struct Case {
    name: String,
    level: Vec<u16>,
    steps: Vec<[u16; 8]>,
}

fn cases() -> Vec<Case> {
    let mut result: Vec<Case> = Vec::new();
    for line in include_str!("assets/reference.txt").lines() {
        let mut words = line.split_whitespace();
        match words.next() {
            Some("level") => {
                let name = words.next().unwrap().to_owned();
                let _unverified = words.next().unwrap();
                let level: Vec<u16> = words.map(|v| v.parse().unwrap()).collect();
                assert_eq!(level[7], 1);
                assert_eq!(level.len(), 8 + usize::from(level[6]) * 8);
                assert_eq!(level[6], level[0] * level[1]);
                assert!(level[6] <= 256);
                result.push(Case {
                    name,
                    level,
                    steps: Vec::new(),
                });
            }
            Some("step") => result.last_mut().unwrap().steps.push(
                words
                    .map(|v| v.parse().unwrap())
                    .collect::<Vec<_>>()
                    .try_into()
                    .unwrap(),
            ),
            Some("#") | None => {}
            _ => panic!("invalid reference row"),
        }
    }
    result
}

fn compile(source: &str) -> Vec<u16> {
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let parsed =
        rcc::frontend::compile_program_named("Bloxorz rules", source, &options, &mut |name| {
            if name == "bloxorz_core" {
                Ok(include_str!("rcc/bloxorz_core.rs").to_owned())
            } else if name == "bloxorz_scene" {
                Ok(include_str!("rcc/bloxorz_scene.rs").to_owned())
            } else if name == "gpu_transform" {
                Ok(include_str!("../../rcc/gpu_transform.rs").to_owned())
            } else {
                std::fs::read_to_string(
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("rcc")
                        .join(format!("{name}.rs")),
                )
                .map_err(|e| e.to_string())
            }
        })
        .expect("compile RCC frontend");
    rcc_backend::try_compile(parsed, &options, "main")
        .expect("compile CPU V3")
        .words
}

fn replay() -> usize {
    let code = compile(PROGRAM);
    let mut total = 0;
    for case in cases() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code);
        assert!(case.steps.len() <= 512);
        cpu.physical_memory_mut()[0x5000..0x5000 + case.level.len()].copy_from_slice(&case.level);
        cpu.physical_memory_mut()[0x6000] = case.steps.len() as u16;
        for (i, step) in case.steps.iter().enumerate() {
            cpu.physical_memory_mut()[0x6001 + i] = step[0];
        }
        let result = cpu.run(20_000_000).expect("bounded CPU execution");
        assert!(
            matches!(result, RunOutcome::Halted { signal: 0x600d, .. }),
            "{}: {result:?}",
            case.name
        );
        for (i, step) in case.steps.iter().enumerate() {
            for j in 0..7 {
                let actual =
                    cpu.physical_memory(PhysicalWordAddress::new((0x8000 + i * 7 + j) as u32));
                assert_eq!(
                    actual,
                    step[j + 1],
                    "{} step {i} command {} field {j}",
                    case.name,
                    step[0]
                );
            }
        }
        total += case.steps.len();
    }
    total
}

fn main() {
    if std::env::args().any(|arg| arg == "--geometry") {
        geometry::report();
        return;
    }
    println!(
        "Bloxorz CPU rule replay: {} reference transitions passed",
        replay()
    );
}

#[cfg(test)]
mod tests {
    #[test]
    fn history_wrap_move_limit_and_snapshot_roundtrip() {
        let source = r#"
mod bloxorz_core;
fn check(ok:bool){if !ok{halt(0xbad);}}
static HISTORY:Buf<u16,1536>=Buf::new([0;1536]);
fn main(){
 let mut state:Buf<u16,10>=Buf::new([0;10]);let mut hs:Buf<u16,2>=Buf::new([0;2]);
 let level=unsafe{Ptr::from_addr(0x5000).as_u16_array()};
 blox_reset(level,state.as_array(),hs.as_array());
 let mut i:u16=0;
 while i<300{check(blox_dispatch(level,state.as_array(),HISTORY.as_array(),hs.as_array(),i&1));i+=1;}
 check(hs[1u16]==256&&state[5u16]==300);
 i=0;while i<256{check(blox_dispatch(level,state.as_array(),HISTORY.as_array(),hs.as_array(),5));i+=1;}
 check(state[5u16]==44&&hs[1u16]==0);
 check(!blox_dispatch(level,state.as_array(),HISTORY.as_array(),hs.as_array(),5));
 state[5u16]=65535;check(!blox_dispatch(level,state.as_array(),HISTORY.as_array(),hs.as_array(),0));
 check(!blox_dispatch(level,state.as_array(),HISTORY.as_array(),hs.as_array(),99));
 check(hs[1u16]==0&&state[5u16]==65535);
 state[0u16]=65535;state[1u16]=65534;state[2u16]=2;state[3u16]=4095;state[4u16]=2;state[6u16]=65533;state[7u16]=65532;state[8u16]=1;state[9u16]=1;
 blox_pack(state.as_array(),HISTORY.as_array(),0);
 let mut restored:Buf<u16,10>=Buf::new([0;10]);blox_unpack(restored.as_array(),HISTORY.as_array(),0);
 i=0;while i<10{check(state[i]==restored[i]);i+=1;}
 check(blox_dispatch(level,state.as_array(),HISTORY.as_array(),hs.as_array(),4));check(state[5u16]==0&&hs[1u16]==0);
 halt(0x600d);
}
"#;
        let mut cpu = super::CpuV3Sim::default();
        cpu.set_boot_window(&super::compile(source));
        let level = super::cases().remove(0).level;
        cpu.physical_memory_mut()[0x5000..0x5000 + level.len()].copy_from_slice(&level);
        let result = cpu.run(2_000_000).unwrap();
        assert!(
            matches!(result, super::RunOutcome::Halted { signal: 0x600d, .. }),
            "{result:?}"
        );
    }
    #[test]
    fn off_board_rest_is_a_fall() {
        let source = r#"
mod bloxorz_core;
fn main(){
 let mut state:Buf<u16,10>=Buf::new([65535,1,1,0,0,1,0,0,0,0]);
 let level=unsafe{Ptr::from_addr(0x5000).as_u16_array()};
 blox_rest(level,state.as_array());
 halt(state[4u16]);
}
"#;
        let mut cpu = super::CpuV3Sim::default();
        cpu.set_boot_window(&super::compile(source));
        let level = super::cases().remove(0).level;
        cpu.physical_memory_mut()[0x5000..0x5000 + level.len()].copy_from_slice(&level);
        let result = cpu.run(100_000).unwrap();
        assert!(
            matches!(result, super::RunOutcome::Halted { signal: 2, .. }),
            "{result:?}"
        );
    }
    #[test]
    fn rcc_rules_match_original_game_step_by_step() {
        assert!(super::replay() == 7705);
    }
}
