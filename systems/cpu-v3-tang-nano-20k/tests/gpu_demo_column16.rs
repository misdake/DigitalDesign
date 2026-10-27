//! Actual serial demo caller on CPU V3; peers certify software bytes/ownership only.
use cpu_v3::{rcc_backend, CpuV3Sim, Device, StepOutcome};
use std::{any::Any, cell::RefCell, collections::BTreeMap, path::Path, rc::Rc};
#[path = "../build_support/rcc_modules.rs"]
#[allow(dead_code)]
mod resolver;

#[derive(Default)]
struct State {
    us: u64,
    clear: u64,
    gpu_until: u64,
    present_until: u64,
    active: u32,
    shadow: u32,
    format: u16,
    shadow_format: u16,
    pending: bool,
    count: u16,
    starts: usize,
    swaps: usize,
    phase: u16,
    draws: Vec<usize>,
    ui: Vec<usize>,
    command_words: Vec<usize>,
    meshes: Vec<(usize, usize)>,
    inflight: Option<(Vec<u16>, Vec<u16>)>,
}
struct Peer {
    kind: u8,
    state: Rc<RefCell<State>>,
}
fn bytes(memory: &[u16], at: usize, count: usize) -> Vec<u8> {
    memory[at / 2..(at + count) / 2]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect()
}
fn observe(memory: &[u16], s: &mut State) {
    let cb = bytes(memory, 0x4c0000, (usize::from(s.count) + 1) * 4);
    let commands = cb
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect::<Vec<_>>();
    let mut scratch = BTreeMap::<usize, Vec<u8>>::new();
    let mut seen_meshes = vec![];
    let mut draws = 0;
    let mut targets = 0;
    let mut clears = 0;
    let mut ui = 0;
    for &c in &commands {
        match c >> 28 {
            1 => {
                let address = ((c >> 10) & 0x3ffff) as usize * 32;
                let length = 32usize << ((c >> 1) & 3);
                assert!((0x4d0000..0x4e0000).contains(&address));
                assert!(address + length <= 0x4e0000);
                scratch.insert(
                    ((c >> 3) & 127) as usize * 32,
                    bytes(memory, address, length),
                );
            }
            2 => {
                let at = ((c >> 12) & 65535) as usize * 128;
                let length = (((c >> 1) & 7) as usize + 1) * 128;
                seen_meshes.push((at, length));
            }
            3 => draws += 1,
            6 => {
                let value = scratch.get(&((c & 127) as usize * 32)).unwrap();
                let word = u32::from_le_bytes(value[..4].try_into().unwrap());
                assert_eq!(word & 65535, 0x500000 / 128);
                assert_eq!((word >> 16) & 15, 6);
                assert_eq!((word >> 20) & 1, 1);
                assert_eq!(word & (1 << 23), 0, "no RTT descriptor bit");
            }
            8 => clears += 1,
            11 => {
                let value = scratch.get(&((c & 127) as usize * 32)).unwrap();
                let descriptor = u32::from_le_bytes(value[..4].try_into().unwrap());
                let grid = (descriptor & 65535) * 128;
                assert!((0x4d0000..=0x4e0000 - 768).contains(&grid));
                assert_eq!(descriptor >> 16, 0x4e);
                assert!(value[4..].iter().all(|&b| b == 0));
                ui += 1;
            }
            10 => {
                let value = scratch.get(&((c & 127) as usize * 32)).unwrap();
                let word = u64::from_le_bytes(value[..8].try_into().unwrap());
                let color = if s.starts == 0 { 0x440000 } else { 0x400000 };
                assert_eq!(
                    word,
                    ((color >> 9) as u64) | (((0x480000 >> 9) as u64) << 14)
                );
                assert!(value[8..].iter().all(|&b| b == 0));
                targets += 1;
            }
            _ => {}
        }
    }
    assert_eq!(draws, 24);
    assert_eq!(seen_meshes, s.meshes);
    assert_eq!((targets, clears), (1, 2));
    assert_eq!(*commands.last().unwrap(), 15 << 28);
    assert_eq!(commands[commands.len() - 2], 7 << 28);
    assert_eq!(commands[commands.len() - 3], (4 << 28) | 3);
    s.draws.push(draws);
    s.ui.push(ui);
    s.command_words.push(commands.len());
    s.inflight = Some((
        memory[0x4c0000 / 2..0x4d0000 / 2].to_vec(),
        memory[0x4d0000 / 2..0x4e0000 / 2].to_vec(),
    ));
}
impl Device for Peer {
    fn read(&mut self, memory: &mut [u16], ch: u8) -> u16 {
        let mut s = self.state.borrow_mut();
        if self.kind == 3 && s.pending && s.us >= s.present_until {
            s.active = s.shadow;
            s.format = s.shadow_format;
            s.pending = false;
        }
        match (self.kind, ch) {
            (0, 10) => (s.us / 1000) as u16,
            (0, 11) => (s.us - s.clear).min(65535) as u16,
            (3, 0) => (s.us / 16667) as u16,
            (3, 1) => s.active as u16,
            (3, 2) => (s.active >> 16) as u16,
            (3, 3) => u16::from(s.pending),
            (3, 4) => s.format,
            (4, 2) => {
                if let Some((cb, obj)) = &s.inflight {
                    assert_eq!(&memory[0x4c0000 / 2..0x4d0000 / 2], cb);
                    assert_eq!(&memory[0x4d0000 / 2..0x4e0000 / 2], obj);
                }
                if s.us < s.gpu_until {
                    1
                } else {
                    s.inflight = None;
                    0
                }
            }
            _ => 0,
        }
    }
    fn write(&mut self, memory: &mut [u16], ch: u8, value: u16) {
        let mut s = self.state.borrow_mut();
        match (self.kind, ch) {
            (0, 11) if value & 1 != 0 => s.clear = s.us,
            (4, 0) => {
                assert!(s.inflight.is_none() && !s.pending);
                s.count = value;
            }
            (4, 1) => {
                assert_eq!(value, 0x9800);
                assert!(s.inflight.is_none() && !s.pending);
                observe(memory, &mut s);
                s.starts += 1;
                s.gpu_until = s.us + 2000;
            }
            (3, 1) => s.shadow = (s.shadow & 0xffff0000) | u32::from(value),
            (3, 2) => s.shadow = (s.shadow & 65535) | (u32::from(value) << 16),
            (3, 4) => s.shadow_format = value,
            (3, 3) => {
                assert!(!s.pending && s.inflight.is_none() && s.us >= s.gpu_until);
                assert_eq!(s.starts, s.swaps + 1);
                s.swaps += 1;
                s.pending = true;
                s.present_until = s.us + 100;
            }
            (7, 1) => s.phase = value,
            _ => {}
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
#[test]
fn actual_classic_serial_caller_uses_column_assets_and_targets() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = root.join("../../target/column-scene/host");
    let assets = out.join("assets");
    std::fs::create_dir_all(&out).unwrap();
    let p = std::process::Command::new("python")
        .args(["-B"])
        .arg(root.join("examples/gpu_demo/tools/assets.py"))
        .args(["--profile", "column16", "--output"])
        .arg(&assets)
        .output()
        .unwrap();
    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
    let m: serde_json::Value =
        serde_json::from_slice(&std::fs::read(assets.join("assets.json")).unwrap()).unwrap();
    let constants = std::fs::read_to_string(assets.join("demo_assets.rs")).unwrap();
    let source = include_str!("../examples/gpu_demo/rcc/main.rs");
    let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    options.stack_init = cpu_v3_tang_nano_20k::boot::SINGLE_BOOT_STACK;
    let ir = rcc::frontend::compile_program_named(
        "actual column classic caller",
        source,
        &options,
        &mut |n| {
            resolver::load_native_module(
                &root.join("examples/gpu_demo/rcc"),
                &root.join("rcc"),
                &constants,
                n,
                resolver::NativeLayoutProfile::Column16,
                &mut |_| {},
            )
        },
    )
    .unwrap();
    let program = rcc_backend::try_compile(ir, &options, "demo_native_main").unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    for r in m["resources"].as_array().unwrap() {
        let at = r["address"].as_u64().unwrap() as usize;
        let data = std::fs::read(assets.join(r["file"].as_str().unwrap())).unwrap();
        for (i, b) in data.as_chunks::<2>().0.iter().enumerate() {
            cpu.physical_memory_mut()[at / 2 + i] = u16::from_le_bytes([b[0], b[1]]);
        }
    }
    let meshes = m["mesh"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["address"].as_u64().unwrap() as usize,
                r["size"].as_u64().unwrap() as usize - 128,
            )
        })
        .collect();
    let state = Rc::new(RefCell::new(State {
        active: 0x200000,
        meshes,
        ..Default::default()
    }));
    for kind in [0, 3, 4, 5, 7] {
        cpu.attach_device(
            kind,
            Box::new(Peer {
                kind,
                state: state.clone(),
            }),
        );
    }
    let mut halted = false;
    let mut steps = 0;
    for i in 0..12_000_000 {
        state.borrow_mut().us = i / 54;
        if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
            assert_eq!(signal, 0x600d, "phase={}", state.borrow().phase);
            halted = true;
            steps = i;
            break;
        }
    }
    let s = state.borrow();
    assert!(
        halted,
        "caller watchdog phase={} starts={}",
        s.phase, s.starts
    );
    assert_eq!((s.starts, s.swaps), (2, 2));
    assert_eq!(s.draws, [24, 24]);
    assert_eq!(s.ui, [0, 1]);
    assert!(!s.pending && s.inflight.is_none());
    assert_eq!(cpu.data_segments(), [0, 1, 2, 3]);
    for r in m["resources"].as_array().unwrap() {
        let at = r["address"].as_u64().unwrap() as usize;
        let data = std::fs::read(assets.join(r["file"].as_str().unwrap())).unwrap();
        assert_eq!(bytes(cpu.physical_memory_mut(), at, data.len()), data);
    }
    std::fs::write(out.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({
        "status":"pass","frames":2,"draws":s.draws,"ui_calls":s.ui,"command_counts":s.command_words,"cpu_steps":steps,
        "targets":["0x440000","0x400000"],"depth":"0x480000","cb":"0x4c0000","objects":"0x4d0000",
        "source_entry":"demo_native_main","bound":12000000,"assets_unchanged":true,
        "boundary":"Actual RCC/CPU caller and controlled GPU/Display completion peers. No rasterization, pixel correctness or physical timing claim."
    })).unwrap()).unwrap();
}

#[test]
fn column_guards_reject_old_spans_and_alignment() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = r#"mod gpu;
fn check(ok:bool){if !ok{halt(0xbad);}}
fn main(){
 check(!gpu_target_valid(0x9000,0x9600));
 check(!gpu_asset_extent(0x9700,1));
 GPU_RENDER_PREVIOUS.texture_sector=0xa000;
 GPU_RENDER_PREVIOUS.texture_sectors=99;
 GPU_RENDER_PREVIOUS.size_log2=6;GPU_RENDER_PREVIOUS.full_mip=1;
 check(!gpu_render_valid(view_of(&mut GPU_RENDER_PREVIOUS)));
 GPU_RENDER_PREVIOUS.texture_sectors=100;
 check(gpu_render_valid(view_of(&mut GPU_RENDER_PREVIOUS)));
 GPU_RENDER_PREVIOUS.texture_sector=0xa001;
 check(!gpu_render_valid(view_of(&mut GPU_RENDER_PREVIOUS)));
 halt(0x600d);
}"#;
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let ir = rcc::frontend::compile_program_named(
        "column guard boundaries",
        source,
        &options,
        &mut |n| {
            resolver::load_native_module(
                &root.join("rcc"),
                &root.join("rcc"),
                "",
                n,
                resolver::NativeLayoutProfile::Column16,
                &mut |_| {},
            )
        },
    )
    .unwrap();
    let p = rcc_backend::try_compile(ir, &options, "main").unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&p.words);
    assert!(matches!(
        cpu.run(100000).unwrap(),
        cpu_v3::RunOutcome::Halted { signal: 0x600d, .. }
    ));
}
