//! Bounded caller qualification; compiled by qualify_api_callers.py with a fixed System loader.
//! Devices model control handshakes; optional cold replay supplies actual GPU/MC outputs.
//! Physical CPU cache timing and Display scanout remain outside this harness.
use crate::resolver;
#[path = "../../rcc/gpu_capture_abi.rs"]
mod abi;
use cpu_v3::{rcc_backend, CpuV3Sim, Device, StepOutcome};
use std::{any::Any, cell::RefCell, path::Path, rc::Rc};

#[derive(Default)]
struct State {
    buttons: usize,
    uart: Vec<u8>,
    gate_actions: Vec<u16>,
    native: Option<Native>,
    publications: usize,
    start_publications: Vec<usize>,
    gate_publications: Vec<usize>,
    count: u16,
    busy: u16,
    frame: u16,
    pending: u16,
    active: u32,
    shadow: u32,
    milliseconds: u16,
    microseconds: u16,
    submissions: Vec<Vec<u16>>,
    presents: Vec<u32>,
}
#[derive(Clone)]
struct Native {
    rtl: std::path::PathBuf,
    build: std::path::PathBuf,
    out: std::path::PathBuf,
    timeout_seconds: u64,
}
fn native_run(memory: &mut [u16], native: &Native, index: usize) {
    let out = native.out.join(format!("native-{index}"));
    let helper = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|p| {
            p.join("systems/cpu-v3-tang-nano-20k/tools/native_api_c_submission.py")
                .is_file()
        })
        .unwrap()
        .join("systems/cpu-v3-tang-nano-20k/tools/native_api_c_submission.py");
    let result = std::process::Command::new("python")
        .arg("-B")
        .arg(helper)
        .arg("--capture")
        .arg(native.out.join(format!("capture-{index}.json")))
        .arg("--build")
        .arg(&native.build)
        .arg("--program")
        .arg(native.out.join("program.bin"))
        .arg("--recipe")
        .arg(native.out.parent().unwrap().join("source-identity.json"))
        .arg("--rtl")
        .arg(&native.rtl)
        .arg("--out")
        .arg(&out)
        .arg("--timeout")
        .arg(native.timeout_seconds.to_string())
        .output()
        .unwrap();
    std::fs::write(
        native.out.join(format!("native-{index}.log")),
        [result.stdout.clone(), result.stderr.clone()].concat(),
    )
    .unwrap();
    assert!(
        result.status.success(),
        "native GPU/MC failed: {}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let handback: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("memory-handback.json")).unwrap()).unwrap();
    assert_eq!(handback["complete"], true);
    for row in handback["writes"].as_array().unwrap() {
        let bytes = std::fs::read(row["file"].as_str().unwrap()).unwrap();
        let address = row["address"].as_u64().unwrap() as usize;
        assert_eq!(bytes.len(), row["bytes"].as_u64().unwrap() as usize);
        for (i, word) in bytes.as_chunks::<2>().0.iter().enumerate() {
            memory[address / 2 + i] = u16::from_le_bytes(*word);
        }
    }
}
struct Peer(u8, Rc<RefCell<State>>);
impl Device for Peer {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        let mut s = self.1.borrow_mut();
        match (self.0, channel) {
            (0, 8) => {
                let values = [0, 2, 2, 0, 0, 1, 1, 0, 0, 2, 2, 0];
                let value = values[s.buttons.min(values.len() - 1)];
                s.buttons += 1;
                value
            }
            (0, 10) => {
                s.milliseconds = s.milliseconds.wrapping_add(40);
                s.milliseconds
            }
            (0, 11) => {
                s.microseconds = s.microseconds.wrapping_add(100);
                s.microseconds
            }
            (3, 0) => {
                s.frame = s.frame.wrapping_add(1);
                s.frame
            }
            (3, 1) => s.active as u16,
            (3, 2) => (s.active >> 16) as u16,
            (3, 3) => {
                if s.pending != 0 {
                    s.pending -= 1;
                    if s.pending == 0 {
                        s.active = s.shadow;
                        let a = s.active;
                        s.presents.push(a);
                    }
                }
                u16::from(s.pending != 0)
            }
            (3, 4) => 0,
            (4, 2) => {
                let busy = s.busy;
                s.busy = s.busy.saturating_sub(1);
                u16::from(busy != 0)
            }
            _ => 0,
        }
    }
    fn write(&mut self, memory: &mut [u16], channel: u8, value: u16) {
        let mut s = self.1.borrow_mut();
        match (self.0, channel) {
            (0, 1) => s.publications += 1,
            (0, 3) => s.uart.push(value as u8),
            (0, 11) => s.microseconds = 0,
            (3, 1) => s.shadow = (s.shadow & 0xffff0000) | u32::from(value),
            (3, 2) => s.shadow = (s.shadow & 65535) | (u32::from(value) << 16),
            (3, 4) => assert_eq!(value, 0),
            (3, 3) => {
                assert_eq!((value, s.pending, s.busy), (1, 0, 0));
                s.pending = 2;
            }
            (4, 0) => {
                assert_eq!(s.busy, 0);
                s.count = value;
            }
            (4, 1) => {
                assert_eq!(s.busy, 0);
                let base = usize::from(value) * 64;
                let words = (usize::from(s.count) + 1) * 2;
                let commands = memory[base..base + words].to_vec();
                assert_eq!(&commands[words - 2..], &[0, 0xf000]);
                if let Some(native) = s.native.clone() {
                    native_run(memory, &native, s.submissions.len());
                }
                let publication = s.publications;
                s.start_publications.push(publication);
                s.submissions.push(commands);
                s.busy = 3;
            }
            (4, _) => panic!("unexpected GPU channel"),
            _ => {}
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
fn location(program: &rcc_backend::CpuV3Program, name: &str) -> Option<usize> {
    program
        .debug
        .globals
        .iter()
        .find(|g| g.name == name)
        .map(|g| match g.loc {
            rcc::VarLoc::Global(a) => usize::from(a),
            _ => panic!("global location"),
        })
}
fn execute(
    program: &rcc_backend::CpuV3Program,
    assets: Option<&Path>,
    out: &Path,
    static_bytes: usize,
    limit: u32,
    native: Option<Native>,
) -> serde_json::Value {
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    if let Some(path) = assets {
        let spec: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        for r in spec["resources"].as_array().unwrap() {
            let bytes =
                std::fs::read(path.parent().unwrap().join(r["file"].as_str().unwrap())).unwrap();
            assert_eq!(bytes.len(), r["bytes"].as_u64().unwrap() as usize);
            let base = r["address"].as_u64().unwrap() as usize;
            for (i, b) in bytes.iter().enumerate() {
                let a = base + i;
                let old = cpu.physical_memory_mut()[a / 2];
                cpu.physical_memory_mut()[a / 2] =
                    (old & !(255 << ((a & 1) * 8))) | (u16::from(*b) << ((a & 1) * 8));
            }
        }
    }
    let state = Rc::new(RefCell::new(State {
        native,
        active: 0x200000,
        shadow: 0x200000,
        ..State::default()
    }));
    for kind in [0, 3, 4, 5] {
        cpu.attach_device(kind, Box::new(Peer(kind, state.clone())));
    }
    let globals = [("GAME", 10usize), ("SESSION", 10), ("MOTION", 7)];
    let trace_locations = globals
        .iter()
        .filter_map(|(n, len)| location(program, n).map(|at| (at, *len)))
        .collect::<Vec<_>>();
    let mut ticks = Vec::new();
    let mut captures = 0usize;
    let mut minimum_sp = 0xc000u16;
    let mut halted = false;
    let mut steps = 0u32;
    assert!((1..=1_000_000_000).contains(&limit));
    while steps < limit {
        steps += 1;
        let sp = cpu.registers()[13];
        if sp != 0 {
            minimum_sp = minimum_sp.min(sp);
        }
        match cpu.step().unwrap() {
            StepOutcome::Halted { signal } => {
                let fault = location(program, "GPU_API_FAULT")
                    .map(|a| cpu.physical_memory_mut()[a..a + 8].to_vec());
                assert_eq!(signal, 0x600d, "fault {fault:?}");
                halted = true;
                break;
            }
            StepOutcome::Signaled(event) if event.signal_type == 13 => {
                state.borrow_mut().gate_actions.push(event.value);
            }
            StepOutcome::Signaled(event) if event.signal_type == 14 => {
                let memory = cpu.physical_memory_mut();
                let header_at = location(program, "GPU_CAPTURE_HEADER").unwrap();
                let ranges_at = location(program, "GPU_CAPTURE_RANGES").unwrap();
                let header =
                    memory[header_at..header_at + usize::from(abi::GPU_CAPTURE_WORDS)].to_vec();
                let count = usize::from(header[usize::from(abi::GPU_CAPTURE_H_RANGE_COUNT)]);
                let ranges = memory
                    [ranges_at..ranges_at + count * usize::from(abi::GPU_CAPTURE_RANGE_WORDS)]
                    .to_vec();
                let blobs = ranges
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .map(|r| {
                        let base = usize::from(r[2]) | (usize::from(r[3]) << 16);
                        let bytes = usize::from(r[4]) | (usize::from(r[5]) << 16);
                        assert_eq!((base & 1, bytes & 1), (0, 0));
                        memory[base / 2..(base + bytes) / 2]
                            .iter()
                            .flat_map(|w| w.to_le_bytes())
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>();
                let mut state = state.borrow_mut();
                assert_eq!(state.submissions.len(), captures);
                let publication = state.publications;
                state.gate_publications.push(publication);
                let capture = serde_json::json!({"header":header,"ranges":ranges,"blobs":blobs,"scope":"actual A/B/C prepared CPU capture before COUNT/START; control completion peer, no rendered preimage chain"});
                std::fs::write(
                    out.join(format!("capture-{captures}.json")),
                    serde_json::to_vec(&capture).unwrap(),
                )
                .unwrap();
                captures += 1;
            }
            StepOutcome::Signaled(event) if event.signal_type == 15 => {
                let memory = cpu.physical_memory_mut();
                ticks.push(
                    trace_locations
                        .iter()
                        .flat_map(|(at, len)| memory[*at..*at + *len].iter().copied())
                        .collect::<Vec<_>>(),
                );
            }
            _ => {}
        }
    }
    if !halted {
        let s = state.borrow();
        let partial = serde_json::json!({"complete":false,"pc":cpu.pc(),"steps":steps,
            "uart_bytes":s.uart.len(),"button_reads":s.buttons,"gate_actions":s.gate_actions,
            "submissions":s.submissions.len(),"minimum_sp":minimum_sp});
        std::fs::write(
            out.join("incomplete.json"),
            serde_json::to_vec_pretty(&partial).unwrap(),
        )
        .unwrap();
    }
    assert!(halted, "bounded caller timeout after {limit} steps");
    let stack_bytes = usize::from(0xc000 - minimum_sp) * 2;
    assert!(
        static_bytes + stack_bytes <= 16384,
        "static+observed stack exceeds low-memory budget"
    );
    std::fs::write(
        out.join("session-ticks.json"),
        serde_json::to_vec(&ticks).unwrap(),
    )
    .unwrap();
    let s = state.borrow();
    if captures != 0 {
        assert_eq!(captures, s.submissions.len());
        let extra = s
            .start_publications
            .iter()
            .zip(&s.gate_publications)
            .map(|(a, b)| a - b)
            .collect::<Vec<_>>();
        assert_eq!(
            extra,
            if s.gate_actions.is_empty() {
                (0..captures)
                    .map(|i| usize::from(i == 1))
                    .collect::<Vec<_>>()
            } else {
                s.gate_actions
                    .iter()
                    .map(|&v| usize::from(v == 1))
                    .collect::<Vec<_>>()
            },
            "resume must republish exactly once"
        );
    }
    std::fs::write(
        out.join("commands.json"),
        serde_json::to_vec(&s.submissions).unwrap(),
    )
    .unwrap();
    std::fs::write(out.join("uart.bin"), &s.uart).unwrap();
    let clock = location(program, "PROFILER_CLOCK")
        .map(|at| cpu.physical_memory_mut()[at..at + 2].to_vec());
    let scope = if s.native.is_some() {
        "actual checked CPU and cold production GPU/MC outputs before completion; controlled Display, no physical CPU cache or scanout"
    } else {
        "actual checked CPU caller; GPU/Display control peers do not render, scan out or qualify physical cache timing"
    };
    serde_json::json!({"profiler_clock":clock,"uart_bytes":s.uart.len(),"halted":true,"steps":steps,"step_limit":limit,"static_bytes":static_bytes,"observed_stack_bytes":stack_bytes,"max_frame_words":program.debug.functions.iter().map(|f|f.frame_size).max().unwrap_or(0),"submissions":s.submissions.len(),"confirmed_fronts":s.presents,"capture_count":captures,"gate_publications":s.gate_publications,"start_publications":s.start_publications,"session_ticks":ticks.len(),"native_replays":if s.native.is_some(){s.submissions.len()}else{0},"scope":scope})
}
pub fn run() {
    let args = std::env::args().collect::<Vec<_>>();
    let source = Path::new(&args[1]);
    let shared = Path::new(&args[2]);
    let local = Path::new(&args[3]);
    let output = Path::new(&args[4]);
    let text = std::fs::read_to_string(source)
        .unwrap()
        .replace("fn main()", "fn interactive_main()")
        + &format!("\nfn main(){{{}();}}\n", args[6]);
    let constants = std::fs::read_to_string(&args[5]).unwrap();
    for debug in [false, true] {
        let out = output.join(if debug { "debug" } else { "release" });
        std::fs::create_dir_all(&out).unwrap();
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = debug;
        let check = rcc::frontend::rustc_check::RustcCheck::new(out.join("checked"));
        let capture_enabled = args.get(9).is_some_and(|v| v != "--no-capture");
        let config = if capture_enabled {
            resolver::RendererCaptureConfiguration::from_manifest(
                Path::new(&args[10]).to_path_buf(),
                &Path::new(&args[9]).join(if debug {
                    "debug/build.json"
                } else {
                    "release/build.json"
                }),
            )
            .unwrap()
        } else {
            resolver::RendererCaptureConfiguration::without_capture(
                resolver::RendererLayoutProfile::Column16,
            )
        };
        let ir = resolver::compile_explicit_gpu_program_checked(
            "C actual caller",
            &text,
            &options,
            local,
            shared,
            &constants,
            &config,
            &check,
            &mut |_| {},
        )
        .unwrap();
        let static_bytes = ir.debug.static_data_end * 2;
        let program = rcc_backend::try_compile(ir, &options, "main").unwrap();
        let bytes = program
            .words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>();
        std::fs::write(out.join("program.bin"), &bytes).unwrap();
        let mut result = serde_json::json!({"checked":true,"static_bytes":static_bytes,"code_bytes":bytes.len(),"entry":args[6],"gpu_debug":debug,"capture_enabled":capture_enabled,"scope":if capture_enabled{"fixed actual A/B/C enabled capture; control peers, not final G3"}else{"fixed actual System loader/root; capture disabled; not G3"}});
        if args.get(7).is_some_and(|arg| arg != "--compile-only") {
            let assets = if args[7] == "--session" {
                None
            } else {
                Some(Path::new(&args[7]))
            };
            result["execution"] = execute(
                &program,
                assets,
                &out,
                static_bytes,
                args.get(8)
                    .map(|v| v.parse().unwrap())
                    .unwrap_or(20_000_000),
                args.get(11).map(|rtl| Native {
                    rtl: Path::new(rtl).to_path_buf(),
                    build: Path::new(&args[9]).join(if debug {
                        "debug/build.json"
                    } else {
                        "release/build.json"
                    }),
                    out: out.clone(),
                    timeout_seconds: args.get(12).map(|v| v.parse().unwrap()).unwrap_or(300),
                }),
            );
        }
        std::fs::write(
            out.join("result.json"),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
        println!("{result}");
    }
}
