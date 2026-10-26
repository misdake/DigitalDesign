//! Actual RCC/CPU execution with explicit GPU and Display completion peers.
//! These tests check software ownership and bytes, not rasterization/scanout.
use cpu_v3::{rcc_backend, CpuV3Sim, RunOutcome};
use gpu_v2::{
    framebuffer::ports::MaterializedSurface,
    system::vliw_command::{target_params, Command, CommandBuffer},
};
use std::{any::Any, cell::RefCell, path::Path, rc::Rc};
#[path = "gpu_persistent_ui.rs"]
mod persistent;

#[derive(Default)]
struct State {
    gpu: u16,
    active: u32,
    staged: u32,
    next: u32,
    format: u16,
    staged_format: u16,
    pending: bool,
    display_error: u16,
    cache_error: bool,
    count: u16,
    starts: Vec<(u16, Vec<u16>, Vec<u16>)>,
    swaps: usize,
    overlap: usize,
    counts: usize,
    maintenance: usize,
    persistent_ranges: Option<Vec<Vec<(u16, u16)>>>,
    ui_snapshots: Vec<Vec<u16>>,
}
struct Peer {
    kind: u8,
    state: Rc<RefCell<State>>,
}
fn image(memory: &[u16], byte: usize, words: usize) -> Vec<u16> {
    memory[byte / 2..byte / 2 + words].to_vec()
}
impl cpu_v3::Device for Peer {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        let s = self.state.borrow();
        match (self.kind, channel) {
            (0, 5) => {
                if s.cache_error {
                    0x8000
                } else {
                    0
                }
            }
            (4, 2) => s.gpu,
            (3, 1) => s.active as u16,
            (3, 2) => (s.active >> 16) as u16,
            (3, 3) => u16::from(s.pending) | s.display_error,
            (3, 4) => s.format,
            (7, 0) => s.starts.len() as u16,
            (7, 1) => s.swaps as u16,
            _ => 0,
        }
    }
    fn write(&mut self, memory: &mut [u16], channel: u8, value: u16) {
        let mut s = self.state.borrow_mut();
        match (self.kind, channel) {
            (4, 0) => {
                assert_eq!(s.gpu, 0);
                assert!(!s.pending);
                s.count = value;
                s.counts += 1;
            }
            (4, 1) => {
                assert_eq!(s.gpu, 0);
                assert!(!s.pending);
                let number = s.starts.len();
                let objects = if value == 0x9200 {
                    0x4a0000
                } else {
                    assert_eq!(value, 0xfc00);
                    0x7f0000
                };
                let offset = if number == 0 && s.persistent_ranges.is_none() {
                    32
                } else {
                    0
                };
                let target = image(memory, objects + offset * 2, 16);
                let mut bytes = Vec::new();
                for w in &target {
                    bytes.extend(w.to_le_bytes());
                }
                let color = if number.is_multiple_of(2) {
                    0x430000
                } else {
                    0x400000
                };
                assert_ne!(color / 2, s.active as usize);
                assert_eq!(
                    bytes,
                    target_params(MaterializedSurface {
                        width: 400,
                        height: 240,
                        color_base_bytes: color as u32,
                        depth_base_bytes: 0x460000
                    })
                    .unwrap()
                );
                let commands = image(memory, usize::from(value) * 128, 64);
                let mut expected = vec![
                    Command::wait(true, true),
                    Command::FLUSH,
                    Command::dma_local((objects + offset * 2) as u32, 276, 32).unwrap(),
                    Command::wait(true, false),
                    Command::set_target(276).unwrap(),
                    Command::clear(false, 0x1111 + number as u16),
                    Command::clear(true, 0x2222),
                ];
                if let Some(ranges) = &s.persistent_ranges {
                    let page = if number.is_multiple_of(2) {
                        0x7df800
                    } else {
                        0x7dfc00
                    };
                    if !ranges[number].is_empty() {
                        expected.extend([
                            Command::wait(true, true),
                            Command::FLUSH,
                            Command::dma_local(page + 768, 276, 32).unwrap(),
                            Command::wait(true, false),
                        ]);
                        for &(first, end) in &ranges[number] {
                            expected.push(if first == 0 && end == 750 {
                                Command::ui_grid(276).unwrap()
                            } else {
                                Command::ui_grid_range(276, first, end).unwrap()
                            });
                        }
                    }
                    s.ui_snapshots.push(image(memory, page as usize, 512));
                }
                expected.extend([Command::wait(true, true), Command::FLUSH, Command::END]);
                assert_eq!(usize::from(s.count), expected.len() - 1);
                let mut golden = CommandBuffer::new();
                for c in expected {
                    golden.push(c);
                }
                let raw: Vec<u8> = commands.iter().flat_map(|w| w.to_le_bytes()).collect();
                assert_eq!(raw, golden.image().unwrap().storage);
                s.starts.push((value, commands, target));
                s.gpu = 1;
            }
            (3, 1) => s.staged = (s.staged & 0xffff0000) | u32::from(value),
            (3, 2) => s.staged = (s.staged & 65535) | (u32::from(value) << 16),
            (3, 4) => s.staged_format = value,
            (3, 3) => {
                assert_eq!(value, 1);
                assert_eq!(s.gpu, 0);
                assert!(!s.pending);
                s.next = s.staged;
                s.pending = true;
                s.swaps += 1;
            }
            (7, 0) => match value {
                0 => s.gpu = 0,
                1 => { /* A frame event alone cannot retire old reads. */ }
                2 => {
                    assert!(s.pending);
                    s.active = s.next;
                    s.format = s.staged_format;
                    s.pending = false;
                }
                3 => s.cache_error = true,
                4 => s.gpu = 2,
                5 => s.display_error = 8,
                10 => {
                    assert_eq!(s.gpu, 1);
                    assert_eq!(s.starts.len(), 1);
                    assert_eq!(image(memory, 0x490000, 64), s.starts[0].1);
                    let target = if s.persistent_ranges.is_some() {
                        0x4a0000
                    } else {
                        0x4a0040
                    };
                    assert_eq!(image(memory, target, 16), s.starts[0].2);
                    s.overlap += 1;
                }
                _ => panic!("peer stimulus {value}"),
            },
            (0, 1) => s.maintenance += 1,
            _ => {}
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

const READY: &str = r#"
 check(gpu_pipeline_init(DISPLAY_RGB565)==GPU_IDLE);
 let mut first=gpu_pipeline_buffer(1);first[0u16].object_words=32;
 check(gpu_pipeline_begin(0x1111,0x2222)==1);check(gpu_pipeline_seal());
"#;

#[test]
fn compiled_handoff_failure_freezes_before_either_send() {
    // Target is already bound, but a failed final publication sends nothing.
    let (_, state) = run(&format!(
        r#"{READY}
 dev_send(7,0,3);check(gpu_pipeline_advance()==GPU_FAULT);
 check(GPU_PIPE[0u16]==3 && GPU_PIPE[2u16]==1 && GPU_PIPE[3u16]==0);
 check(first[0u16].state==GPU_CB_READY);
 check(GPU_PIPE_FAULT[0u16]==12 && GPU_PIPE_FAULT[2u16]==1);
 check(gpu_pipeline_poll()==GPU_FAULT && gpu_pipeline_begin(0,0)==0);
 check(!gpu_pipeline_cancel());check(gpu_pipeline_init(0)==GPU_REJECTED);
 "#
    ));
    let s = state.borrow();
    assert_eq!(s.counts, 0);
    assert!(s.starts.is_empty());
    assert_eq!(s.swaps, 0);
}

#[test]
fn compiled_transport_and_display_faults_retain_both_inputs() {
    for stimulus in [4, 5] {
        let (_, state) = run(&format!(
            r#"{READY}
 check(gpu_pipeline_advance()==GPU_PENDING);
 check(gpu_pipeline_begin(0x1112,0x2222)==2);check(gpu_pipeline_seal());
 if {stimulus}==5 {{ dev_send(7,0,0); }}
 dev_send(7,0,{stimulus});check(gpu_pipeline_poll()==GPU_FAULT);
 check(GPU_PIPE[0u16]==3 && GPU_PIPE[3u16]==1 && GPU_PIPE[2u16]==2);
 check(gpu_pipeline_begin(0,0)==0 && !gpu_pipeline_cancel());
 check(gpu_pipeline_advance()==GPU_FAULT && dev_recv(7,0)==1);
 "#
        ));
        let s = state.borrow();
        assert_eq!(s.counts, 1);
        assert_eq!(s.swaps, 0);
    }
}

#[test]
fn compiled_missing_relocation_cannot_publish_a_placeholder_target() {
    let (_, state) = run(&format!(
        r#"{READY}
 GPU_SLOT_TARGETS[0u16]=0;
 check(gpu_pipeline_advance()==GPU_FAULT && GPU_PIPE_FAULT[0u16]==11);
 check(GPU_PIPE[2u16]==1 && GPU_PIPE[3u16]==0);
 "#
    ));
    let s = state.borrow();
    assert_eq!(s.counts, 0);
    assert!(s.starts.is_empty());
}
fn run(body: &str) -> (CpuV3Sim, Rc<RefCell<State>>) {
    run_with_globals("", body)
}
fn run_with_globals(globals: &str, body: &str) -> (CpuV3Sim, Rc<RefCell<State>>) {
    let program = compile(globals, body);
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    let state = attach(&mut cpu, None);
    let result = cpu.run(5_000_000).unwrap();
    assert!(
        matches!(result, RunOutcome::Halted { signal: 0x600d, .. }),
        "{result:?}"
    );
    assert_eq!(cpu.data_segments(), [0, 1, 2, 3]);
    (cpu, state)
}
fn compile(globals: &str, body: &str) -> rcc_backend::CpuV3Program {
    let source=format!("mod gpu_pipeline; {globals} fn check(ok:bool){{if !ok{{halt(0xbad);}}}} fn main(){{{body}halt(0x600d);}}");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let parsed = rcc::frontend::compile_program_named(
        "internal GPU pipeline",
        &source,
        &options,
        &mut |n| std::fs::read_to_string(root.join(format!("{n}.rs"))).map_err(|e| e.to_string()),
    )
    .unwrap();
    rcc_backend::try_compile(parsed, &options, "main").unwrap()
}
fn attach(
    cpu: &mut CpuV3Sim,
    persistent_ranges: Option<Vec<Vec<(u16, u16)>>>,
) -> Rc<RefCell<State>> {
    let state = Rc::new(RefCell::new(State {
        active: 0x200000,
        persistent_ranges,
        ..State::default()
    }));
    for kind in [0, 3, 4, 7] {
        cpu.attach_device(
            kind,
            Box::new(Peer {
                kind,
                state: state.clone(),
            }),
        );
    }
    state
}
#[test]
fn compiled_two_slots_late_bind_and_distinct_display_retirement() {
    let (_, state) = run(r#"
 check(gpu_pipeline_init(DISPLAY_RGB565)==GPU_IDLE);
 let mut first=gpu_pipeline_buffer(1);first[0u16].object_words=32;
 check(gpu_pipeline_begin(0x1111,0x2222)==1);check(gpu_pipeline_seal());
 check(gpu_pipeline_advance()==GPU_PENDING && dev_recv(7,0)==1);
 check(gpu_pipeline_begin(0,0)==2);check(gpu_pipeline_cancel());
 check(!gpu_pipeline_cancel());
 check(gpu_pipeline_begin(0x1112,0x2222)==2);check(gpu_pipeline_seal());
 dev_send(7,0,10);check(gpu_pipeline_advance()==GPU_PENDING && dev_recv(7,0)==1);
 check(gpu_pipeline_begin(0,0)==0);
 dev_send(7,0,0);check(gpu_pipeline_poll()==GPU_PENDING);
 check(first[0u16].state==GPU_CB_COMPLETED && dev_recv(7,1)==1);
 check(gpu_pipeline_advance()==GPU_PENDING && dev_recv(7,0)==1);
 dev_send(7,0,1);check(gpu_pipeline_advance()==GPU_PENDING && dev_recv(7,0)==1);
 dev_send(7,0,2);check(gpu_pipeline_advance()==GPU_PENDING && dev_recv(7,0)==2);
 check(gpu_pipeline_begin(0x1113,0x2222)==1);check(gpu_pipeline_seal());
 check(gpu_pipeline_advance()==GPU_PENDING && dev_recv(7,0)==2);
 dev_send(7,0,0);check(gpu_pipeline_poll()==GPU_PENDING);dev_send(7,0,2);
 check(gpu_pipeline_advance()==GPU_PENDING && dev_recv(7,0)==3);
 dev_send(7,0,0);check(gpu_pipeline_poll()==GPU_PENDING);dev_send(7,0,2);
 check(gpu_pipeline_poll()==GPU_COMPLETED);check(gpu_pipeline_advance()==GPU_IDLE);
 "#);
    let s = state.borrow();
    assert_eq!(
        s.starts.iter().map(|s| s.0).collect::<Vec<_>>(),
        [0x9200, 0xfc00, 0x9200]
    );
    assert_eq!(s.swaps, 3);
    assert_eq!(s.overlap, 1);
    assert_eq!(s.maintenance, 3);
}

#[test]
fn compiled_second_slot_ui_snapshot_crosses_window_without_touching_running_input() {
    let (mut cpu, state) = run_with_globals(
        "static CELLS:Buf<u16,375>=Buf::new([0x0100;375]);",
        &format!(
            r#"{READY}
 check(gpu_pipeline_advance()==GPU_PENDING);
 let mut second=gpu_pipeline_buffer(2);second[0u16].object_words=16180;
 check(gpu_pipeline_begin(0,0)==2);
 let mut atlas:GpuUiAtlas=GpuUiAtlas{{sector:0x9800,sectors:512}};
 check(gpu_builder_ui(second,view_of(&mut atlas),CELLS.as_array(),0,750,false));
 check(second[0u16].object_words==16656);
 dev_send(7,0,10);check(gpu_pipeline_cancel());
 "#
        ),
    );
    let s = state.borrow();
    assert_eq!(s.starts.len(), 1);
    assert_eq!(s.overlap, 1);
    assert_eq!(s.maintenance, 1);
    let at = 0x7f0000 + 16256 * 2;
    assert_eq!(image(cpu.physical_memory_mut(), at, 375), vec![0x0100; 375]);
    assert_eq!(
        image(cpu.physical_memory_mut(), at + 750, 9),
        vec![65535; 9]
    );
    let actual: Vec<u8> = image(cpu.physical_memory_mut(), at + 768, 16)
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    assert_eq!(
        actual,
        gpu_v2::system::vliw_command::ui_params(at as u32, 0x4c0000).unwrap()
    );
}
