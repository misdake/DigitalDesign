//! Executed scalar stores are observed independently of the canvas helpers.
//! GPU/Display are controlled completion peers, not a pixel or timing model.
use super::*;
use cpu_v3::StepOutcome;
use std::collections::BTreeMap;

const PAGE: usize = 0x7df800 / 2;
const INIT: &str = r#"
check(gpu_pipeline_init(0)==GPU_IDLE);
let mut atlas:GpuUiAtlas=GpuUiAtlas{sector:0x9800,sectors:512};
check(gpu_persistent_ui_register(view_of(&mut atlas)));
"#;
#[derive(Debug)]
struct Snapshot {
    pages: [Vec<u16>; 2],
    metadata: Vec<u16>,
    version: u16,
    stores: Vec<(usize, usize, u16)>,
    steps: usize,
    remaps: usize,
}
struct Trace {
    markers: BTreeMap<u16, Snapshot>,
    state: Rc<RefCell<State>>,
    remaining_stores: Vec<(usize, usize, u16)>,
}
fn global(program: &rcc_backend::CpuV3Program, name: &str) -> usize {
    match program
        .debug
        .globals
        .iter()
        .find(|v| v.name == name)
        .unwrap()
        .loc
    {
        rcc::VarLoc::Global(address) => usize::from(address),
        ref other => panic!("{name}: {other:?}"),
    }
}
fn execute(globals: &str, body: &str, ranges: Vec<Vec<(u16, u16)>>) -> Trace {
    let program = compile(globals, body);
    let meta = global(&program, "GPU_PUI_PAGES");
    let canvas = global(&program, "GPU_PUI_CANVAS");
    let pipe = global(&program, "GPU_PIPE");
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    cpu.physical_memory_mut()[PAGE..PAGE + 1024].fill(0xa55a);
    let state = attach(&mut cpu, Some(ranges));
    let mut prefix: Option<u16> = None;
    let mut stores = Vec::new();
    let mut markers = BTreeMap::new();
    let mut halted = false;
    let mut remaps = 0;
    for steps in 0..5_000_000 {
        let pc = (usize::from(cpu.code_segment()) << 16) | usize::from(cpu.pc());
        let instruction = program.words[pc];
        let major = instruction >> 12;
        // These fixtures deliberately use only scalar instructions. Do not
        // silently omit a multiword FPU store from the independent journal.
        assert!(!matches!(major, 0xc..=0xe));
        if major == 9 {
            let immediate = prefix.map_or_else(
                || ((instruction as i16) << 12 >> 12) as u16,
                |payload| (payload << 4) | (instruction & 15),
            );
            let logical =
                cpu.registers()[usize::from((instruction >> 4) & 15)].wrapping_add(immediate);
            let physical = (usize::from(cpu.data_segments()[usize::from(logical >> 14)]) << 14)
                | usize::from(logical & 0x3fff);
            if (PAGE..PAGE + 1024).contains(&physical) {
                let page = (physical - PAGE) / 512;
                let word = (physical - PAGE) % 512;
                let memory = cpu.physical_memory_mut();
                assert_eq!(memory[meta + page * 4], 1, "write to non-recording UI page");
                assert_eq!(memory[pipe], 1, "write after freeze");
                assert_eq!(usize::from(memory[pipe + 1]), page + 1, "wrong recorder");
                assert!(word < 400, "unused page tail was touched");
                stores.push((
                    page,
                    word,
                    cpu.registers()[usize::from((instruction >> 8) & 15)],
                ));
            }
        }
        prefix = if major == 15 {
            Some(instruction & 0xfff)
        } else {
            None
        };
        let segments = cpu.data_segments();
        let outcome = cpu.step().unwrap();
        remaps += usize::from(cpu.data_segments() != segments);
        match outcome {
            StepOutcome::Signaled(event) if event.signal_type == 1 => {
                let memory = cpu.physical_memory_mut();
                assert!(
                    markers
                        .insert(
                            event.value,
                            Snapshot {
                                pages: [
                                    memory[PAGE..PAGE + 512].to_vec(),
                                    memory[PAGE + 512..PAGE + 1024].to_vec()
                                ],
                                metadata: memory[meta..meta + 8].to_vec(),
                                version: memory[canvas + 1],
                                stores: std::mem::take(&mut stores),
                                steps,
                                remaps: std::mem::take(&mut remaps),
                            }
                        )
                        .is_none(),
                    "duplicate trace marker"
                );
            }
            StepOutcome::Halted { signal } => {
                assert_eq!(
                    signal,
                    0x600d,
                    "halt at {pc:x}, markers: {:?}",
                    markers.keys()
                );
                halted = true;
                break;
            }
            _ => {}
        }
    }
    assert!(halted, "bounded UI fixture did not complete");
    assert_eq!(cpu.data_segments(), [0, 1, 2, 3]);
    Trace {
        markers,
        state,
        remaining_stores: stores,
    }
}
fn expected_page(cells: &[u16], page: usize) -> Vec<u16> {
    let mut words: Vec<u16> = cells
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| p[0] | (p[1] << 8))
        .collect();
    words.resize(384, 65535);
    words.extend(
        gpu_v2::system::vliw_command::ui_params((PAGE * 2 + page * 1024) as u32, 0x4c0000)
            .unwrap()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes([b[0], b[1]])),
    );
    words.resize(512, 0xa55a);
    words
}

#[test]
fn alternating_pages_converge_and_clean_frames_store_nothing() {
    let mut body = format!(
        r#"{INIT}
check(gpu_persistent_ui_set(0,90));
check(gpu_pipeline_begin(0x1111,0x2222)==1);
check(gpu_pipeline_ui_range(0,2));
check(gpu_persistent_ui_field(1,4,IDS.as_array(),2,true));
check(gpu_persistent_ui_set(5,81));check(gpu_pipeline_ui_range(2,8));signal(1,10);
check(gpu_pipeline_seal());signal(1,11);
check(gpu_persistent_ui_set(6,67));check(gpu_pipeline_advance()==GPU_PENDING);
check(gpu_pipeline_begin(0x1112,0x2222)==2);check(gpu_pipeline_ui_range(0,8));
check(gpu_persistent_ui_set(7,68));signal(1,20);check(gpu_pipeline_seal());signal(1,21);
dev_send(7,0,10);
dev_send(7,0,0);check(gpu_pipeline_poll()==GPU_PENDING);
check(gpu_persistent_ui_reference(1)==0 && gpu_persistent_ui_reference(2)==2);
dev_send(7,0,1);check(gpu_pipeline_advance()==GPU_PENDING && dev_recv(7,0)==1);
dev_send(7,0,2);check(gpu_pipeline_advance()==GPU_PENDING && dev_recv(7,0)==2);
"#
    );
    for frame in 3..=7 {
        let slot = (frame + 1) % 2 + 1;
        body.push_str(&format!(
            "check(gpu_pipeline_begin({},0x2222)=={slot});",
            0x1110 + frame
        ));
        if frame == 3 {
            body.push_str("check(gpu_pipeline_ui_range(0,2));IDS[0u16]=88;check(gpu_persistent_ui_field(1,4,IDS.as_array(),1,true));check(gpu_pipeline_ui_range(2,8));");
        } else {
            body.push_str("check(gpu_pipeline_ui_range(0,8));");
        }
        body.push_str(&format!(
            "signal(1,{});check(gpu_pipeline_seal());signal(1,{});",
            frame * 10,
            frame * 10 + 1
        ));
        if frame == 3 {
            body.push_str("check(gpu_persistent_ui_field(1,4,unsafe{Ptr::from_addr(65535).as_u16_array()},1,false));");
        }
        body.push_str(&format!(
            r#"
dev_send(7,0,0);check(gpu_pipeline_poll()==GPU_PENDING);
check(gpu_persistent_ui_reference({})==0 && gpu_persistent_ui_reference({slot})==2);
dev_send(7,0,1);check(gpu_pipeline_advance()==GPU_PENDING && dev_recv(7,0)=={});
dev_send(7,0,2);check(gpu_pipeline_advance()==GPU_PENDING && dev_recv(7,0)=={frame});
"#,
            3 - slot,
            frame - 1
        ));
    }
    body.push_str("dev_send(7,0,0);check(gpu_pipeline_poll()==GPU_PENDING);dev_send(7,0,2);check(gpu_pipeline_poll()==GPU_COMPLETED);check(gpu_pipeline_advance()==GPU_IDLE);signal(1,80);");
    let ranges = (1..=7)
        .map(|f| {
            if f == 1 || f == 3 {
                vec![(0, 2), (2, 8)]
            } else {
                vec![(0, 8)]
            }
        })
        .collect();
    let trace = execute(
        "static IDS:Buf<u16,4>=Buf::new([65,66,255,255]);",
        &body,
        ranges,
    );
    let expected_writes = [400, 400, 3, 2, 1, 0, 0];
    let versions = [4, 6, 8, 9, 9, 9, 9];
    let mut cells = vec![255; 750];
    cells[0] = 90;
    cells[1] = 65;
    cells[2] = 66;
    cells[5] = 81;
    for frame in 1..=7 {
        let page = (frame - 1) % 2;
        let before = &trace.markers[&(frame as u16 * 10)];
        let sealed = &trace.markers[&(frame as u16 * 10 + 1)];
        assert!(
            before.stores.is_empty(),
            "edits or range recording wrote a physical UI page"
        );
        assert_eq!(sealed.stores.len(), expected_writes[frame - 1]);
        assert!(sealed.stores.iter().all(|&(p, _, _)| p == page));
        if frame == 2 {
            cells[6] = 67;
            cells[7] = 68;
        }
        if frame == 3 {
            cells[1] = 88;
            cells[2..5].fill(255);
        }
        if frame == 4 {
            cells[1..5].fill(255);
        }
        assert_eq!(sealed.pages[page], expected_page(&cells, page));
        assert_eq!(
            trace.state.borrow().ui_snapshots[frame - 1],
            sealed.pages[page]
        );
        assert_eq!(sealed.metadata[page * 4], 2);
        assert_eq!(sealed.metadata[page * 4 + 1], 0);
        assert_eq!(sealed.metadata[page * 4 + 2], versions[frame - 1]);
        assert_eq!(sealed.version, versions[frame - 1]);
        if frame > 1 {
            assert_eq!(sealed.pages[1 - page], before.pages[1 - page]);
        }
        eprintln!(
            "UI frame {frame}: {} stores, {} scalar steps to seal",
            sealed.stores.len(),
            sealed.steps - before.steps
        );
    }
    assert!(trace.markers[&80].stores.is_empty());
    assert!(trace.remaining_stores.is_empty());
    let s = trace.state.borrow();
    assert_eq!(
        s.starts.iter().map(|s| s.0).collect::<Vec<_>>(),
        [0x9200, 0xfc00, 0x9200, 0xfc00, 0x9200, 0xfc00, 0x9200]
    );
    assert_eq!(s.maintenance, 7);
    assert_eq!(s.swaps, 7);
    assert_eq!(s.overlap, 1);
}

#[test]
fn validation_capacity_cancel_and_version_wrap_preserve_page_history() {
    let body = format!(
        r#"{INIT}
// Registration is immutable and idempotent; none of these write high memory.
check(gpu_persistent_ui_register(view_of(&mut atlas)));
atlas.sector=0xfe00;check(!gpu_persistent_ui_register(view_of(&mut atlas)));
atlas.sector=0x9a00;check(!gpu_persistent_ui_register(view_of(&mut atlas)));atlas.sector=0x9800;
check(!gpu_persistent_ui_set(750,1) && !gpu_persistent_ui_set(0,256));
check(!gpu_persistent_ui_field(749,2,IDS.as_array(),2,true));
check(!gpu_persistent_ui_field(0,1,IDS.as_array(),2,true));
check(!gpu_persistent_ui_field(0,2,IDS.as_array(),2,true));
check(!gpu_persistent_ui_field(0,2,unsafe{{Ptr::from_addr(0xbfff).as_u16_array()}},2,true));
check(GPU_PUI_CANVAS[1u16]==0 && GPU_PUI_DESIRED[0u16]==65535);
check(GPU_PUI_PAGES[1u16]==375 && GPU_PUI_PAGES[5u16]==375);
check(gpu_persistent_ui_set(748,65) && gpu_persistent_ui_set(749,66));
check(gpu_persistent_ui_set(749,66) && GPU_PUI_CANVAS[1u16]==2);
check(GPU_PUI_DESIRED[374u16]==0x4241 && GPU_PUI_DIRTY[23u16]==127);
signal(1,1);
// All command capacity is preflighted before a page is referenced or emitted.
check(gpu_pipeline_begin(0x1111,0x2222)==1);
let mut cb=gpu_pipeline_buffer(1);let words=cb[0u16].command_words;
let cursor=GPU_BUILD[5u16];let objects=cb[0u16].object_words;
cb[0u16].command_words=32762;
check(!gpu_pipeline_ui_range(0,750));
check(gpu_persistent_ui_reference(1)==0 && GPU_BUILD[5u16]==cursor);
check(cb[0u16].command_words==32762 && cb[0u16].object_words==objects);
cb[0u16].command_words=words;
check(!gpu_pipeline_ui_range(2,1) && !gpu_pipeline_ui_range(0,751));
check(gpu_pipeline_ui_range(750,750) && gpu_persistent_ui_reference(1)==0);
check(gpu_pipeline_ui_range(0,750));check(gpu_pipeline_cancel());
check(gpu_persistent_ui_reference(1)==0 && GPU_PUI_CANVAS[1u16]==2);signal(1,2);
check(gpu_pipeline_begin(0x1111,0x2222)==1 && gpu_pipeline_ui_range(0,750));
check(gpu_persistent_ui_prepare(1));signal(1,3);
check(gpu_pipeline_cancel() && gpu_persistent_ui_reference(1)==0);signal(1,4);
check(gpu_pipeline_begin(0x1111,0x2222)==1 && gpu_pipeline_ui_range(0,750));
signal(1,9);check(gpu_persistent_ui_prepare(1));signal(1,10);
check(gpu_pipeline_seal());signal(1,5);
check(!gpu_pipeline_cancel() && !gpu_persistent_ui_prepare(1));
check(gpu_pipeline_advance()==GPU_PENDING);
// Dirty masks, not equality of wrapped version values, decide what to copy.
GPU_PUI_CANVAS[1u16]=65535;check(gpu_persistent_ui_set(749,88));
check(GPU_PUI_CANVAS[1u16]==0 && GPU_PUI_PAGES[1u16]==1);
check(gpu_pipeline_begin(0x1112,0x2222)==2 && gpu_pipeline_ui_range(0,750));
check(gpu_pipeline_seal());signal(1,6);
dev_send(7,0,0);check(gpu_pipeline_poll()==GPU_PENDING);dev_send(7,0,2);
check(gpu_pipeline_advance()==GPU_PENDING);
// The old page version is deliberately equal, but it still needs one word.
GPU_PUI_PAGES[2u16]=0;
check(gpu_pipeline_begin(0x1113,0x2222)==1 && gpu_pipeline_ui_range(0,750));
check(gpu_pipeline_seal());signal(1,7);
dev_send(7,0,0);check(gpu_pipeline_poll()==GPU_PENDING);dev_send(7,0,2);
check(gpu_pipeline_advance()==GPU_PENDING);
dev_send(7,0,0);check(gpu_pipeline_poll()==GPU_PENDING);dev_send(7,0,2);
check(gpu_pipeline_poll()==GPU_COMPLETED);check(gpu_pipeline_advance()==GPU_IDLE);signal(1,8);
"#
    );
    let t = execute(
        "static IDS:Buf<u16,2>=Buf::new([65,256]);",
        &body,
        vec![vec![(0, 750)]; 3],
    );
    for marker in [1, 2, 4, 5, 8, 9, 10] {
        assert!(t.markers[&marker].stores.is_empty());
    }
    assert_eq!(t.markers[&3].stores.len(), 400);
    assert_eq!(t.markers[&10].remaps, 0, "clean prepare remapped DSEG");
    assert_eq!(t.markers[&6].stores.len(), 400);
    assert_eq!(t.markers[&7].stores, [(0, 374, 0x5841)]);
    assert_eq!(t.markers[&3].pages[0][374], 0x4241);
    assert_eq!(t.markers[&5].pages[0], t.markers[&3].pages[0]);
    assert_eq!(t.markers[&6].pages[0], t.markers[&5].pages[0]);
    assert_eq!(t.markers[&7].pages[0][..384], t.markers[&6].pages[1][..384]);
    assert_eq!(t.state.borrow().maintenance, 3);
    assert!(t.remaining_stores.is_empty());
}

#[test]
fn publication_failure_sends_nothing_and_retains_recorded_ui() {
    let t = execute(
        "",
        &format!(
            r#"{INIT}
check(gpu_pipeline_begin(0x1111,0x2222)==1 && gpu_pipeline_ui_range(0,750));
check(gpu_pipeline_seal());signal(1,1);
dev_send(7,0,3);check(gpu_pipeline_advance()==GPU_FAULT);
check(GPU_PIPE_FAULT[0u16]==12 && gpu_persistent_ui_reference(1)==2);
check(!gpu_persistent_ui_set(0,65) && !gpu_pipeline_cancel());
check(!gpu_persistent_ui_prepare(1) && gpu_pipeline_begin(0,0)==0);
check(!gpu_persistent_ui_register(view_of(&mut atlas)));signal(1,2);
"#
        ),
        vec![],
    );
    assert_eq!(t.markers[&1].stores.len(), 400);
    assert!(t.markers[&2].stores.is_empty());
    assert_eq!(t.markers[&1].pages, t.markers[&2].pages);
    assert_eq!(t.markers[&2].metadata[0], 2);
    let s = t.state.borrow();
    assert_eq!(s.counts, 0);
    assert!(s.starts.is_empty());
    assert_eq!(s.swaps, 0);
}

#[test]
fn terminal_faults_freeze_running_and_queued_ui_references() {
    for stimulus in [4, 5] {
        let t = execute(
            "",
            &format!(
                r#"{INIT}
check(gpu_pipeline_begin(0x1111,0x2222)==1 && gpu_pipeline_ui_range(0,750));
check(gpu_pipeline_seal());check(gpu_pipeline_advance()==GPU_PENDING);
check(gpu_persistent_ui_set(0,65));
check(gpu_pipeline_begin(0x1112,0x2222)==2 && gpu_pipeline_ui_range(0,750));
check(gpu_pipeline_seal());signal(1,1);
if {stimulus}==5 {{dev_send(7,0,0);}}
dev_send(7,0,{stimulus});check(gpu_pipeline_poll()==GPU_FAULT);
check(gpu_persistent_ui_reference(1)==3 && gpu_persistent_ui_reference(2)==2);
check(!gpu_pipeline_cancel() && !gpu_persistent_ui_prepare(1) && !gpu_persistent_ui_prepare(2));
check(!gpu_persistent_ui_set(0,66));check(gpu_pipeline_advance()==GPU_FAULT);signal(1,2);
"#
            ),
            vec![vec![(0, 750)]],
        );
        assert_eq!(t.markers[&1].stores.len(), 800);
        assert!(t.markers[&2].stores.is_empty());
        assert_eq!(t.markers[&1].pages, t.markers[&2].pages);
        assert_eq!(t.markers[&2].metadata[0], 3);
        assert_eq!(t.markers[&2].metadata[4], 2);
        assert_eq!(t.state.borrow().counts, 1);
        assert_eq!(t.state.borrow().swaps, 0);
    }
}
