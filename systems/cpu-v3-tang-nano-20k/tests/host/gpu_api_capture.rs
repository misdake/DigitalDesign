//! C implementation against the frozen source12/facts4 contract, not A ownership.
use cpu_v3::{rcc_backend, CpuV3Sim, Device, RunOutcome};
use gpu_v2::system::vliw_command::Command;
use std::{any::Any, cell::RefCell, collections::BTreeMap, path::Path, rc::Rc};

const SHIM: &str = r#"use crate::dsl_rt::*;
pub fn gpu_execute_capture_health(slot:u16)->u16{dev_recv(7,0)}
pub fn gpu_present_capture_state(out:Array<u16>)->u16{dev_recv(7,1)}
pub fn gpu_window_read_words(sector:u16,word:u16,mut dst:Array<u16>,count:u16)->bool{
 if count>64{return false;}if dev_recv(7,12)!=0{return false;}
 dev_send(7,3,sector);dev_send(7,4,word);dev_send(7,5,count);
 let mut i:u16=0;while i<count{dst[i]=dev_recv(7,2);i+=1;}true}
"#;
const APP: &str = r#"mod gpu_api_capture;use gpu_api_capture::*;
fn check(ok:bool){if !ok{halt(0xbad);}}
fn main(){let mut s:Buf<u16,12>=Buf::new([1,0,0x7000,64,20,64,0x7100,64,16,2,4,2]);
let mut f:Buf<u16,16>=Buf::new([0x2000,2048,5,2,0x3000,2048,6,2,0x4000,2048,5,2,0x5000,2048,6,2]);
let mode=dev_recv(7,8);
if mode==1{s[0u16]=2;}if mode==2{s[1u16]=2;}
if mode==3{s[4u16]=21;}if mode==4{s[5u16]=128;}
if mode==5{f[1u16]=1599;}if mode==6{f[3u16]=1;}
if mode==7{f[1u16]=65535;}if mode==8{s[9u16]=65;}
if mode==9{s[8u16]=15;}
if mode==20{s[4u16]=16;}
if mode==21{s[4u16]=6;s[8u16]=0;s[9u16]=0;}
if mode==24{s[1u16]=1;}if mode==25{f[0u16]=0;}
if mode==26{f[0u16]=0xf800;}
if mode==27{s[3u16]=32768;s[6u16]=0x7300;}
if mode==29{s[4u16]=38;s[8u16]=32;s[9u16]=4;}
if mode>=1&&mode<=9 || mode>=30{
 check(!gpu_capture_begin(s.as_array(),f.as_array()));check(gpu_capture_health()!=0);
 check(gpu_capture_gate()==2);halt(0x600d);}
check(gpu_capture_begin(s.as_array(),f.as_array()));
// Source belongs to the caller; begin must not retain its pointer.
s[1u16]=65535;s[2u16]=0;s[4u16]=0;f[0u16]=65535;f[2u16]=65535;
if mode==22{check(gpu_capture_read_value(2,36864,1)==0);check(gpu_capture_health()==2);halt(0x600d);}
if mode==23{check(!gpu_capture_begin(s.as_array(),f.as_array()));check(gpu_capture_health()==8);halt(0x600d);}
check(gpu_capture_gate()==dev_recv(7,13));
if dev_recv(7,13)!=2{gpu_capture_started();gpu_capture_submitted();check(gpu_capture_health()==1);}
if mode==28{s[1u16]=0;s[2u16]=0x7000;s[4u16]=20;f[0u16]=0x2000;f[2u16]=5;
 check(gpu_capture_begin(s.as_array(),f.as_array()));check(gpu_capture_gate()==0);gpu_capture_started();gpu_capture_submitted();}
halt(0x600d);}"#;
const GATE: &str = r#"use crate::dsl_rt::*;mod gpu_api_capture;use gpu_api_capture::*;
pub fn profiler_gate(d:Array<u16>)->u16{
 if dev_recv(7,8)!=21{
  if gpu_capture_read_value(2,0,0)!=0xabcd{halt(0xca01);}
  if gpu_capture_read_value(2,36863,1)!=0xdcba{halt(0xca02);}
  if dev_recv(7,8)==26&&gpu_capture_read_value(3,28671,0)!=0xfeed{halt(0xca03);}
 }
 dev_send(7,6,1);if dev_recv(7,10)!=0{dev_send(7,11,1);}dev_recv(7,9)}"#;

#[derive(Default)]
struct State {
    words: BTreeMap<usize, u16>,
    mode: u16,
    health: u16,
    display: u16,
    action: u16,
    expected: u16,
    poison_after_gate: u16,
    window_failure: u16,
    sector: usize,
    offset: usize,
    remaining: usize,
    gates: usize,
    reads: usize,
    native: bool,
    commands: u16,
    push_words: u16,
    facts: Vec<u16>,
    fact_cursor: usize,
    fail_expected: u16,
}
struct Peer(Rc<RefCell<State>>);
impl Device for Peer {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        let mut s = self.0.borrow_mut();
        match channel {
            0 => s.health,
            1 => s.display,
            2 => {
                assert!(s.remaining > 0);
                let value = *s.words.get(&(s.sector * 64 + s.offset)).unwrap_or(&0);
                s.offset += 1;
                s.remaining -= 1;
                s.reads += 1;
                value
            }
            8 => {
                if s.native {
                    (s.facts.len() / 4) as u16
                } else {
                    s.mode
                }
            }
            9 => {
                if s.native {
                    let value = s.facts[s.fact_cursor];
                    s.fact_cursor += 1;
                    value
                } else {
                    s.action
                }
            }
            10 => {
                if s.native {
                    s.fail_expected
                } else {
                    s.poison_after_gate
                }
            }
            12 => s.window_failure,
            13 => s.expected,
            14 => s.commands,
            15 => s.push_words,
            _ => panic!("unexpected contract read {channel}"),
        }
    }
    fn write(&mut self, _: &mut [u16], channel: u8, value: u16) {
        let mut s = self.0.borrow_mut();
        match channel {
            3 => s.sector = value.into(),
            4 => s.offset = value.into(),
            5 => s.remaining = value.into(),
            6 => s.gates += 1,
            11 => s.health = value,
            _ => panic!("unexpected contract write {channel}"),
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
struct Forbidden;
impl Device for Forbidden {
    fn read(&mut self, _: &mut [u16], _: u8) -> u16 {
        panic!("C accessed raw Device3/4")
    }
    fn write(&mut self, _: &mut [u16], _: u8, _: u16) {
        panic!("C sent Device3/4 command")
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
fn compile(enabled: bool, app: &str, gate: &str, debug: bool) -> rcc_backend::CpuV3Program {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    options.gpu_debug = debug;
    let parsed = rcc::frontend::compile_program_named("C contract fixture", app, &options, &mut |name| {
        Ok(match name {
            "gpu_execute" => SHIM.into(),
            "gpu_storage" | "gpu_present" => String::new(),
            "gpu_capture_access" => "mod gpu_api_capture;use gpu_api_capture::*;pub fn gpu_capture_error()->u16{gpu_capture_health()}pub fn gpu_capture_range_word(r:u16,f:u16)->u16{gpu_capture_range_value(r,f)}pub fn gpu_capture_read_word(r:u16,l:u16,h:u16)->u16{gpu_capture_read_value(r,l,h)}".into(),
            "profiler_gate" => gate.into(),
            "gpu_capture_config" => format!("pub const GPU_CAPTURE_CONFIG_ENABLED:u16={};pub const GPU_CAPTURE_CONFIG_LAYOUT_ABI:u16=2;pub fn gpu_capture_input_poll()->u16{{0}}pub fn gpu_capture_input_reset(){{}}pub fn gpu_capture_input_arm(){{}}pub fn gpu_capture_manifest_word(index:u16)->u16{{0x3412}}",u16::from(enabled)),
            _ => std::fs::read_to_string(root.join(format!("{name}.rs"))).map_err(|e|e.to_string())?,
        })
    }).unwrap();
    rcc_backend::try_compile(parsed, &options, "main").unwrap()
}
fn fixture(mode: u16) -> State {
    let mut s = State {
        mode,
        ..State::default()
    };
    let mut commands = vec![
        Command::wait(true, true),
        Command::FLUSH,
        Command::dma_local(if mode == 27 { 0x398000 } else { 0x388000 }, 276, 32).unwrap(),
        Command::wait(true, false),
        Command::set_target(276).unwrap(),
    ];
    if mode != 20 {
        commands.extend([Command::clear(false, 0x1357), Command::clear(true, 0x2468)]);
    }
    commands.extend([Command::wait(true, true), Command::FLUSH, Command::END]);
    if mode == 29 {
        commands.pop();
        commands.extend([
            Command::wait(true, true),
            Command::FLUSH,
            Command::dma_local(0x388020, 276, 32).unwrap(),
            Command::wait(true, false),
            Command::set_target(276).unwrap(),
            Command::clear(false, 0x1234),
            Command::clear(true, 0x5678),
            Command::wait(true, true),
            Command::FLUSH,
            Command::END,
        ]);
    }
    if mode == 21 {
        commands = vec![Command::wait(true, true), Command::FLUSH, Command::END];
    }
    for (i, c) in commands.iter().enumerate() {
        s.words.insert(0x7000 * 64 + i * 2, c.0 as u16);
        s.words.insert(0x7000 * 64 + i * 2 + 1, (c.0 >> 16) as u16);
    }
    // Independent native Target32 bit-field oracle; no A implementation copied.
    let color = match mode {
        25 => 0,
        26 => 0xf800,
        _ => 0x2000,
    };
    let push = if mode == 27 { 0x7300 } else { 0x7100 };
    let target = ((color as u32 * 128) >> 9) | ((0x180000u32 >> 9) << 14);
    s.words.insert(push * 64, target as u16);
    s.words.insert(push * 64 + 1, (target >> 16) as u16);
    s.words.insert(color * 64, 0xabcd);
    s.words.insert(color * 64 + 102399, 0xdcba);
    if mode == 26 {
        s.words.insert(0x3fffff, 0xfeed);
    }
    if mode == 29 {
        let second = (0x200000u32 >> 9) | ((0x280000u32 >> 9) << 14);
        s.words.insert(0x7100 * 64 + 16, second as u16);
        s.words.insert(0x7100 * 64 + 17, (second >> 16) as u16);
    }
    match mode {
        30 => {
            s.words.insert(0x7000 * 64 + 19, 0x3000);
        }
        31 => {
            s.words.insert(0x7000 * 64 + 63, 1);
        }
        32 => s.health = 1,
        33 => s.display = 65535,
        34 => s.window_failure = 1,
        _ => {}
    }
    s
}
fn run(p: &rcc_backend::CpuV3Program, state: State) -> (CpuV3Sim, Rc<RefCell<State>>) {
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&p.words);
    let state = Rc::new(RefCell::new(state));
    cpu.attach_device(7, Box::new(Peer(state.clone())));
    cpu.attach_device(3, Box::new(Forbidden));
    cpu.attach_device(4, Box::new(Forbidden));
    let result = cpu.run(2_000_000).unwrap();
    assert!(
        matches!(result, RunOutcome::Halted { signal: 0x600d, .. }),
        "{result:?}, mode={}",
        state.borrow().mode
    );
    (cpu, state)
}
#[test]
fn explicit_clear_load_and_bounded_failures_use_real_c_implementation() {
    for debug in [false, true] {
        let p = compile(true, APP, GATE, debug);
        for mode in [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33,
            34,
        ] {
            let (mut cpu, state) = run(&p, fixture(mode));
            if [0, 20, 21, 24, 25, 26, 27, 28, 29].contains(&mode) {
                let header = super::global(&mut cpu, &p, "GPU_CAPTURE_HEADER", 64);
                assert_eq!((header[0], header[39], header[40]), (2, 2, 4));
                assert_eq!(header[30..32], [0, 0]);
                assert_eq!(header[8], 0); // No fabricated Frame.
                assert_eq!(state.borrow().gates, if mode == 28 { 2 } else { 1 });
                assert_eq!(
                    (header[4], header[6]),
                    if mode == 28 { (2, 2) } else { (1, 1) }
                );
                assert_eq!(header[9], u16::from(mode == 24));
                let n = usize::from(header[36]);
                let ranges = super::global(&mut cpu, &p, "GPU_CAPTURE_RANGES", n * 8);
                if mode == 21 {
                    assert_eq!((header[2], n), (1, 1));
                } else {
                    assert_eq!((header[2], n), (3, if mode == 29 { 10 } else { 6 }));
                    assert_eq!(ranges[2 * 8], if mode == 20 { 11 } else { 26 });
                    assert_eq!(ranges[3 * 8], 9);
                    assert_eq!(ranges[5 * 8], 9);
                    assert_eq!((ranges[2 * 8 + 4], ranges[2 * 8 + 5]), (8192, 3));
                    if mode == 29 {
                        assert_eq!((header[26], header[27]), (0, 32));
                        assert_eq!(ranges[6 * 8], 26);
                    }
                }
                let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/api-c/fixtures");
                std::fs::create_dir_all(&out).unwrap();
                std::fs::write(
                    out.join(format!("mode-{mode}-debug-{debug}.json")),
                    serde_json::to_vec_pretty(
                        &serde_json::json!({"scope":"C contract fixture, not actual A owner/GPU",
                        "header":header,"ranges":ranges}),
                    )
                    .unwrap(),
                )
                .unwrap();
            } else {
                assert_eq!(state.borrow().gates, 0);
            }
        }
    }
}
#[test]
fn thin_gate_rechecks_health_and_disabled_capture_has_no_io() {
    let p = compile(true, APP, GATE, false);
    for (action, poison, expected) in [(0, 0, 0), (1, 0, 1), (2, 0, 2), (7, 0, 2), (0, 1, 2)] {
        let (mut cpu, s) = run(
            &p,
            State {
                action,
                poison_after_gate: poison,
                expected,
                ..fixture(0)
            },
        );
        assert_eq!(s.borrow().gates, 1);
        let c = super::global(&mut cpu, &p, "GPU_CAPTURE_CONTROL", 16);
        assert_eq!(c[0], u16::from(expected == 2));
        if expected == 2 {
            assert_ne!(c[1], 0);
        }
    }
    let app="mod gpu_api_capture;fn main(){let s:Buf<u16,1>=Buf::new([0]);if !gpu_capture_begin(s.as_array(),s.as_array())||gpu_capture_gate()!=0{halt(0xbad);}gpu_capture_started();gpu_capture_submitted();halt(0x600d);}";
    let p = compile(false, app, GATE, false);
    let (_, s) = run(&p, fixture(0));
    assert_eq!((s.borrow().gates, s.borrow().reads), (0, 0));
}
#[test]
fn wire_range_capacity_is_bounded_independently_of_record_facts() {
    let app = r#"mod gpu_api_capture;
fn main(){let s:Buf<u16,12>=Buf::new([1,0,0x7000,64,6,64,0x7100,64,0,0,4,2]);
let f:Buf<u16,1>=Buf::new([0]);if !gpu_capture_begin(s.as_array(),f.as_array()){halt(0xbad);}
let mut i:u16=0;while i<63{if gpu_capture_range_add(10,1,0x5000+i*2,128,0)==0{halt(0xbad);}i+=1;}
if gpu_capture_range_add(10,1,0x5000,128,0)!=2{halt(0xbad);}
if gpu_capture_range_add(10,1,0x6000,128,0)!=0||gpu_capture_health()!=7{halt(0xbad);}halt(0x600d);}"#;
    let p = compile(true, app, GATE, false);
    let (_, s) = run(&p, fixture(21));
    assert_eq!(s.borrow().gates, 0);
}

const NATIVE_APP: &str = r#"mod gpu_api_capture;
static CONTRACT_FACTS:Buf<u16,256>=Buf::new([0;256]);
fn main(){let mut s:Buf<u16,12>=Buf::new([1,0,0x7000,512,0,0,0x7300,1024,0,0,4,2]);
s[4u16]=dev_recv(7,14);s[5u16]=(s[4u16]+63)&65472;s[8u16]=dev_recv(7,15);s[9u16]=dev_recv(7,8);
let mut i:u16=0;while i<s[9u16]*4{CONTRACT_FACTS[i]=dev_recv(7,9);i+=1;}
let ok=gpu_capture_begin(s.as_array(),CONTRACT_FACTS.as_array());
if dev_recv(7,10)==0{if !ok||gpu_capture_gate()!=0{halt(0xbad);}gpu_capture_started();gpu_capture_submitted();}
else{if ok||gpu_capture_health()==0||gpu_capture_gate()!=2{halt(0xbad);}}halt(0x600d);}"#;

fn put(s: &mut State, address: usize, bytes: &[u8]) {
    for (i, word) in bytes.as_chunks::<2>().0.iter().enumerate() {
        s.words
            .insert(address / 2 + i, u16::from_le_bytes([word[0], word[1]]));
    }
}
fn fact(s: &mut State, first: u16, count: u16, kind: u16, access: u16) {
    s.facts.extend([first, count, kind, access]);
}
fn native_fixture(case: &str) -> State {
    use gpu_v2::system::vliw_command::{memcpy_params, ui_params};
    let mut s = State {
        native: true,
        push_words: 192,
        ..State::default()
    };
    let target = (0x200000u32 >> 9) | ((0x280000u32 >> 9) << 14);
    put(&mut s, 0x398000, &target.to_le_bytes());
    fact(&mut s, 0x4000, 2048, 5, 2);
    fact(&mut s, 0x5000, 2048, 6, 2);
    let mut commands = vec![
        Command::wait(true, true),
        Command::FLUSH,
        Command::dma_local(0x398000, 276, 32).unwrap(),
        Command::wait(true, false),
        Command::set_target(276).unwrap(),
        Command::clear(false, 0x1234),
        Command::clear(true, 0xffff),
    ];
    if case.starts_with("ui_load") {
        commands.truncate(5);
        for i in 0..102400 {
            s.words
                .insert(0x4000 * 64 + i, (i as u16).wrapping_mul(73) ^ 0x5a3c);
            s.words.insert(0x5000 * 64 + i, 0xabcd);
        }
    }
    if case.starts_with("ui") {
        s.push_words = 48;
        fact(&mut s, 0x2000, 6, 4, 1);
        fact(&mut s, 0x1000, 512, 3, 1);
        if case == "ui"
            || case == "ui_multi"
            || case == "ui_load_visible"
            || case == "ui_load_multi_target"
        {
            put(
                &mut s,
                0x80000,
                &[0xf801u16; 128]
                    .into_iter()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            );
        }
        put(&mut s, 0x398020, &ui_params(0x100000, 0x80000).unwrap());
        commands.extend([
            Command::dma_local(0x398020, 276, 32).unwrap(),
            Command::wait(true, false),
            Command::wait(true, true),
            Command::FLUSH,
            Command::ui_grid_range(276, 128, 129).unwrap(),
        ]);
        if case == "ui_multi" {
            fact(&mut s, 0x2100, 6, 4, 1);
            put(&mut s, 0x398040, &ui_params(0x108000, 0x80000).unwrap());
            commands.extend([
                Command::dma_local(0x398040, 276, 32).unwrap(),
                Command::wait(true, false),
                Command::ui_grid_range(276, 260, 261).unwrap(),
            ]);
        }
    } else if case == "memcpy" || case == "mutable_descriptor" {
        s.facts.clear();
        s.push_words = 16;
        let destination = if case == "memcpy" { 0x308000 } else { 0x398000 };
        fact(&mut s, 0x6000, 8, 7, 1);
        fact(&mut s, (destination / 128) as u16, 8, 7, 2);
        put(
            &mut s,
            0x398000,
            &memcpy_params(0x300000, destination, 1024).unwrap(),
        );
        for i in 0..512 {
            s.words.insert(0x6000 * 64 + i, (i as u16).wrapping_mul(37));
        }
        commands = vec![
            Command::wait(true, true),
            Command::FLUSH,
            Command::dma_local(0x398000, 264, 32).unwrap(),
            Command::wait(true, false),
            Command::memcpy(264).unwrap(),
        ];
        if case == "mutable_descriptor" {
            commands.push(Command::dma_local(0x398000, 276, 32).unwrap());
            s.fail_expected = 1;
        }
    } else {
        let rgb = case == "draw_rgb";
        let texture_flags = match case {
            "mip_chain" => 5 | 16 | 64,
            "rtt" => 9 | 128 | 32,
            _ => 0,
        };
        let texture_sectors = match case {
            "mip_chain" => 36,
            "rtt" => 2048,
            _ => 4,
        };
        put(
            &mut s,
            0x398080,
            &((0x3000u32) | ((texture_flags as u32) << 16)).to_le_bytes(),
        );
        // Metadata has three vertices, one triangle, uniform20, T offset5.
        let metadata = (3u32 | (1 << 8)) | ((20u32 | (5 << 8)) << 16);
        put(&mut s, 0x80000, &metadata.to_le_bytes());
        if rgb {
            put(&mut s, 0x80008, &16u16.to_le_bytes());
        }
        if case != "missing_mesh" {
            fact(&mut s, 0x1000, 2, 1, 1);
        }
        if !rgb && case != "missing_texture" {
            fact(&mut s, 0x3000, texture_sectors, 2, 1);
        }
        commands.extend([
            Command::dma_local(0x398040, 264, 64).unwrap(),
            Command::wait(true, false),
            Command::set_global(264).unwrap(),
            Command::dma_local(0x398080, 272, 32).unwrap(),
            Command::wait(true, false),
            Command::set_render(272).unwrap(),
        ]);
        if case != "missing_uniform" {
            let mut source = 0x398100;
            if case == "copied_uniform" {
                source = 0x398200;
                s.push_words = 320;
                fact(&mut s, 0x6000, 4, 7, 1);
                fact(&mut s, 0x7304, 4, 7, 2);
                put(
                    &mut s,
                    0x398180,
                    &memcpy_params(0x300000, 0x398200, 512).unwrap(),
                );
                commands.extend([
                    Command::wait(true, true),
                    Command::FLUSH,
                    Command::dma_local(0x398180, 280, 32).unwrap(),
                    Command::wait(true, false),
                    Command::memcpy(280).unwrap(),
                ]);
            }
            commands.extend([
                Command::dma_local(source, 320, 128).unwrap(),
                Command::wait(true, false),
            ]);
        }
        if case == "bound_render_survives_staging_overwrite" {
            s.push_words = 272;
            put(&mut s, 0x398200, &0xffffffffu32.to_le_bytes());
            commands.extend([
                Command::dma_local(0x398200, 272, 32).unwrap(),
                Command::wait(true, false),
            ]);
        }
        if case == "rtt" {
            fact(&mut s, 0x3000, 2048, 5, 2);
            fact(&mut s, 0x2000, 2048, 6, 2);
            let producer = (0x180000u32 >> 9) | ((0x100000u32 >> 9) << 14);
            put(&mut s, 0x398020, &producer.to_le_bytes());
            let mut prefix = vec![
                Command::wait(true, true),
                Command::FLUSH,
                Command::dma_local(0x398020, 276, 32).unwrap(),
                Command::wait(true, false),
                Command::set_target(276).unwrap(),
                Command::clear(false, 0x2468),
                Command::clear(true, 0xffff),
                Command::wait(true, true),
                Command::FLUSH,
                Command::TEX_INVALIDATE,
            ];
            prefix.extend(commands);
            commands = prefix;
        }
        commands.extend([
            Command::dma_meshlet(0x80000, 0, 128).unwrap(),
            Command::wait(true, false),
            Command::draw_meshlet(0).unwrap(),
        ]);
        if case.starts_with("missing_") {
            s.fail_expected = 1;
        }
    }
    commands.extend([Command::wait(true, true), Command::FLUSH, Command::END]);
    if case == "ui_load_multi_target" {
        s.push_words = 80;
        fact(&mut s, 0x3000, 1600, 5, 3);
        fact(&mut s, 0x6800, 1600, 6, 3);
        for i in 0..102400 {
            s.words.insert(0x3000 * 64 + i, 0x2468);
            s.words.insert(0x6800 * 64 + i, 0xbeef);
        }
        let first = (0x180000u32 >> 9) | ((0x340000u32 >> 9) << 14);
        put(&mut s, 0x398080, &first.to_le_bytes());
        let mut prefix = vec![
            Command::wait(true, true),
            Command::FLUSH,
            Command::dma_local(0x398080, 276, 32).unwrap(),
            Command::wait(true, false),
            Command::set_target(276).unwrap(),
            Command::wait(true, true),
            Command::FLUSH,
            Command::dma_local(0x398020, 276, 32).unwrap(),
            Command::wait(true, false),
            Command::ui_grid_range(276, 128, 129).unwrap(),
            Command::wait(true, true),
            Command::FLUSH,
        ];
        prefix.extend(commands);
        commands = prefix;
    }
    s.commands = (commands.len() * 2) as u16;
    for (i, c) in commands.iter().enumerate() {
        put(&mut s, 0x380000 + i * 4, &c.0.to_le_bytes());
    }
    s
}

#[test]
fn short_ui_atlas_captures_declared_extent_and_rejects_actual_glyph_overrun() {
    let gate = "use crate::dsl_rt::*;pub fn profiler_gate(d:Array<u16>)->u16{dev_send(7,6,1);0}";
    let mut results = Vec::new();
    for debug in [false, true] {
        let program = compile(true, NATIVE_APP, gate, debug);
        for (sectors, glyph, accepted, written_grid) in [
            (512, 0, true, false),
            (6, 0, true, false),
            (6, 2, true, false),
            (6, 255, true, false),
            (6, 3, false, false),
            (1, 0, false, false),
            (6, 0, false, true),
        ] {
            let mut state = native_fixture("ui_load_visible");
            for row in state.facts.as_chunks_mut::<4>().0 {
                if row[2] == 3 {
                    row[1] = sectors;
                }
            }
            // Only byte128 is selected. Its paired neighbor is deliberately
            // beyond the short atlas and must remain masked out.
            state.words.insert(0x100000 / 2 + 64, glyph | (250 << 8));
            put(&mut state, 0x80000 + 2 * 256, &[0xc1, 0x07].repeat(128));
            if written_grid {
                use gpu_v2::system::vliw_command::memcpy_params;
                fact(&mut state, 0x6000, 4, 7, 1);
                fact(&mut state, 0x2000, 4, 7, 2);
                put(
                    &mut state,
                    0x398040,
                    &memcpy_params(0x300000, 0x100000, 512).unwrap(),
                );
                let mut commands = vec![
                    Command::dma_local(0x398040, 280, 32).unwrap(),
                    Command::wait(true, false),
                    Command::memcpy(280).unwrap(),
                    Command::wait(true, true),
                ];
                commands.extend((0..usize::from(state.commands) / 2).map(|i| {
                    let low = state.words[&(0x380000 / 2 + i * 2)];
                    let high = state.words[&(0x380000 / 2 + i * 2 + 1)];
                    Command(u32::from(low) | (u32::from(high) << 16))
                }));
                state.commands = (commands.len() * 2) as u16;
                for (i, c) in commands.iter().enumerate() {
                    put(&mut state, 0x380000 + i * 4, &c.0.to_le_bytes());
                }
            }
            state.fail_expected = u16::from(!accepted);
            let (mut cpu, state) = run(&program, state);
            let control = super::global(&mut cpu, &program, "GPU_CAPTURE_CONTROL", 24);
            assert_eq!(
                (control[0], control[1]),
                if accepted { (0, 0) } else { (1, 9) }
            );
            assert_eq!(state.borrow().gates, usize::from(accepted));
            results.push(serde_json::json!({"debug":debug,"atlas_sectors":sectors,"glyph":glyph,
                "written_grid":written_grid,"accepted":accepted,"live":control[0],"error":control[1],
                "gates":state.borrow().gates}));
            if accepted {
                let header = super::global(&mut cpu, &program, "GPU_CAPTURE_HEADER", 64);
                let ranges = super::global(
                    &mut cpu,
                    &program,
                    "GPU_CAPTURE_RANGES",
                    usize::from(header[36]) * 8,
                );
                let atlas = ranges
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .find(|row| row[1] == 6)
                    .unwrap();
                assert_eq!(
                    (atlas[2], atlas[3], atlas[4], atlas[5]),
                    (0, 8, sectors << 7, sectors >> 9)
                );
                if sectors == 6 && glyph == 2 {
                    let data = state.borrow();
                    let blobs: Vec<Vec<u8>> = ranges
                        .as_chunks::<8>()
                        .0
                        .iter()
                        .map(|row| {
                            let base = usize::from(row[2]) | (usize::from(row[3]) << 16);
                            let bytes = usize::from(row[4]) | (usize::from(row[5]) << 16);
                            (0..bytes / 2)
                                .flat_map(|i| {
                                    data.words
                                        .get(&(base / 2 + i))
                                        .copied()
                                        .unwrap_or(0)
                                        .to_le_bytes()
                                })
                                .collect()
                        })
                        .collect();
                    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/api-c");
                    let bytes=serde_json::to_vec(&serde_json::json!({"header":header,"ranges":ranges,"blobs":blobs,
                        "scope":"actual C short-atlas producer with contract peers; not full public A or board qualification"})).unwrap();
                    for directory in [
                        root.join("native-fixtures"),
                        root.join("short-atlas-fix/producer"),
                    ] {
                        std::fs::create_dir_all(&directory).unwrap();
                        std::fs::write(directory.join(format!("ui_short-{debug}.json")), &bytes)
                            .unwrap();
                    }
                }
            }
        }
    }
    let out =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/api-c/short-atlas-fix/cases.json");
    std::fs::write(out, serde_json::to_vec_pretty(&results).unwrap()).unwrap();
}

#[test]
fn native_draw_rtt_ui_and_memcpy_dependencies_are_ordered_and_bounded() {
    for debug in [false, true] {
        let p = compile(
            true,
            NATIVE_APP,
            "use crate::dsl_rt::*;pub fn profiler_gate(d:Array<u16>)->u16{0}",
            debug,
        );
        for case in [
            "draw_uv",
            "draw_rgb",
            "mip_chain",
            "rtt",
            "ui",
            "ui_load",
            "ui_load_visible",
            "ui_load_multi_target",
            "ui_multi",
            "memcpy",
            "mutable_descriptor",
            "missing_mesh",
            "missing_texture",
            "missing_uniform",
            "bound_render_survives_staging_overwrite",
            "copied_uniform",
        ] {
            let state = native_fixture(case);
            let negative = state.fail_expected != 0;
            let (mut cpu, state) = run(&p, state);
            if negative {
                continue;
            }
            let header = super::global(&mut cpu, &p, "GPU_CAPTURE_HEADER", 64);
            let ranges = super::global(
                &mut cpu,
                &p,
                "GPU_CAPTURE_RANGES",
                usize::from(header[36]) * 8,
            );
            let rows: Vec<_> = ranges.as_chunks::<8>().0.iter().collect();
            let textures: Vec<_> = rows.iter().filter(|r| r[1] == 5).collect();
            if case == "draw_rgb" || case.starts_with("ui") || case == "memcpy" {
                assert!(textures.is_empty());
            } else {
                assert!(!textures.is_empty());
            }
            if case == "rtt" {
                assert_eq!(textures.len(), 2);
                assert!(textures.iter().all(|r| r[0] == 9));
            }
            if case == "mip_chain" {
                assert_eq!(textures[0][4], 36 * 128);
            }
            if case.starts_with("ui") {
                assert!(header[2] & 8 != 0);
                assert_eq!(header[24], 128);
            }
            if case == "memcpy" {
                assert_eq!(header[2], 1);
                assert!(rows.iter().any(|r| r[1] == 10 && r[0] == 10));
            }
            let blobs: Vec<_> = rows
                .iter()
                .map(|r| {
                    let address = usize::from(r[2]) | (usize::from(r[3]) << 16);
                    let bytes = usize::from(r[4]) | (usize::from(r[5]) << 16);
                    (0..bytes / 2)
                        .flat_map(|i| {
                            state
                                .borrow()
                                .words
                                .get(&(address / 2 + i))
                                .copied()
                                .unwrap_or(0)
                                .to_le_bytes()
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            let out =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/api-c/native-fixtures");
            std::fs::create_dir_all(&out).unwrap();
            std::fs::write(out.join(format!("{case}-{debug}.json")),serde_json::to_vec(&serde_json::json!({
                "scope":"actual C producer, contract peers, host-copy preimages; not UART/A/GPU qualification",
                "header":header,"ranges":ranges,"blobs":blobs})).unwrap()).unwrap();
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let result = std::process::Command::new("python")
        .args([
            "-B",
            "-m",
            "unittest",
            "test_recorded.ProducerFixtures",
            "-q",
        ])
        .current_dir(root.join("tools/gpu_profiler/tests"))
        .env(
            "GPU_API_CAPTURE_FIXTURE_DIR",
            root.join("../../target/api-c/native-fixtures")
                .canonicalize()
                .unwrap(),
        )
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn aggregate_capture_limit_is_checked_before_range_commit() {
    let app = r#"mod gpu_api_capture;
fn main(){let s:Buf<u16,12>=Buf::new([1,0,0x7000,64,6,64,0x7100,64,0,0,4,2]);
let f:Buf<u16,1>=Buf::new([0]);if !gpu_capture_begin(s.as_array(),f.as_array()){halt(0xbad);}
if gpu_capture_range_add(10,1,0,65408,127)==0||gpu_capture_range_add(5,1,0,0,128)==0{halt(0xbad);}
if gpu_capture_range_add(10,1,0,65408,127)!=2{halt(0xbad);}
if gpu_capture_range_add(10,1,1,128,0)!=0||gpu_capture_health()!=7{halt(0xbad);}halt(0x600d);}"#;
    let p = compile(true, app, GATE, false);
    run(&p, fixture(21));
}

#[derive(Default)]
struct UartState {
    bytes: Vec<u8>,
    buttons: usize,
    ms: u16,
    status_reads: usize,
    ready: bool,
}
struct UartPeer(Rc<RefCell<UartState>>);
impl Device for UartPeer {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        let mut s = self.0.borrow_mut();
        match channel {
            3 => {
                s.status_reads += 1;
                s.ready = !s.status_reads.is_multiple_of(7);
                u16::from(!s.ready)
            }
            8 => {
                let sequence = [0, 2, 2, 0, 0, 1, 1, 0, 0, 2, 2];
                let value = sequence[s.buttons.min(sequence.len() - 1)];
                s.buttons += 1;
                value
            }
            10 => {
                s.ms = s.ms.wrapping_add(25);
                s.ms
            }
            _ => panic!("unexpected UART control read {channel}"),
        }
    }
    fn write(&mut self, _: &mut [u16], channel: u8, value: u16) {
        let mut s = self.0.borrow_mut();
        match channel {
            2 => assert!([0, 2, 4].contains(&value)),
            3 => {
                assert!(s.ready, "serializer ignored UART busy");
                s.ready = false;
                s.bytes.push(value as u8);
            }
            _ => panic!("unexpected UART control write {channel}"),
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
#[test]
fn production_uart_runtime_pauses_dumps_v2_and_resumes_without_gpu_sends() {
    let prefix = NATIVE_APP.split_once("let ok=").unwrap().0;
    let app = format!(
        r#"{prefix}let mut round:u16=0;while round<3{{
if !gpu_capture_begin(s.as_array(),CONTRACT_FACTS.as_array()){{halt(0xca01);}}
let action=gpu_capture_gate();if (round<2&&action!=0)||(round==2&&action!=1){{halt(0xca10+round*4+action);}}
gpu_capture_started();gpu_capture_submitted();round+=1;}}halt(0x600d);}}"#
    );
    let p = compile(
        true,
        &app,
        include_str!("../../rcc/profiler_gate.rs"),
        false,
    );
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&p.words);
    let state = Rc::new(RefCell::new(native_fixture("memcpy")));
    let uart = Rc::new(RefCell::new(UartState::default()));
    cpu.attach_device(7, Box::new(Peer(state.clone())));
    cpu.attach_device(0, Box::new(UartPeer(uart.clone())));
    cpu.attach_device(3, Box::new(Forbidden));
    cpu.attach_device(4, Box::new(Forbidden));
    let result = cpu.run(20_000_000).unwrap();
    assert!(
        matches!(result, RunOutcome::Halted { signal: 0x600d, .. }),
        "{result:?}"
    );
    assert!(uart.borrow().bytes.len() > 1024);
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/api-c/uart-v2");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("capture.bin"), &uart.borrow().bytes).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let result = std::process::Command::new("python")
        .args(["-B", "-m", "unittest", "test_recorded.UartFixture", "-q"])
        .current_dir(root.join("tools/gpu_profiler/tests"))
        .env(
            "GPU_API_UART_CAPTURE",
            out.join("capture.bin").canonicalize().unwrap(),
        )
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn diagnostic_active_clock_removes_pauses_across_wrap_without_touching_wall_or_input() {
    struct Wall(u16);
    impl Device for Wall {
        fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
            assert_eq!(channel, 10);
            let value = self.0;
            self.0 += 1;
            value
        }
        fn write(&mut self, _: &mut [u16], _: u8, _: u16) {
            panic!("diagnostic clock mutated a device");
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }
    let app = r#"mod profiler_clock;use profiler_clock::*;
static CLOCK_RESULTS:Buf<u16,6>=Buf::new([0;6]);
fn main(){
 if profiler_resume_epoch()!=0{halt(1);}
 profiler_clock_record_resume(65000,9000);
 CLOCK_RESULTS[0u16]=profiler_active_milliseconds();CLOCK_RESULTS[1u16]=profiler_resume_epoch();
 // A complete wall-clock wrap changes the epoch while its modulo offset is zero.
 profiler_clock_record_resume(12345,12345);
 CLOCK_RESULTS[2u16]=profiler_active_milliseconds();CLOCK_RESULTS[3u16]=profiler_resume_epoch();
 profiler_clock_record_resume(10000,50000);
 CLOCK_RESULTS[4u16]=profiler_active_milliseconds();CLOCK_RESULTS[5u16]=profiler_resume_epoch();
 halt(0x600d);}"#;
    for debug in [false, true] {
        let program = compile(false, app, GATE, debug);
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        cpu.attach_device(0, Box::new(Wall(10000)));
        assert!(matches!(
            cpu.run(100_000).unwrap(),
            RunOutcome::Halted { signal: 0x600d, .. }
        ));
        assert_eq!(
            super::global(&mut cpu, &program, "CLOCK_RESULTS", 6),
            [464, 1, 465, 2, 26002, 3]
        );
    }
}
