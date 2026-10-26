//! Actual caller round2: immutable native snapshots and bounded CPU preparation.
use super::*;
use cpu_v3::{Device, StepOutcome};

fn application(name: &str) -> (rcc_backend::CpuV3Program, Vec<(usize, Vec<u8>)>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let example = root.join("examples/gpu_renderer");
    let out = root
        .join("../../target/gpu-renderer-round2-20261026")
        .join(name)
        .join("assets");
    assert!(std::process::Command::new("python")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .arg(example.join("tools/assets.py"))
        .arg("--output")
        .arg(&out)
        .status()
        .unwrap()
        .success());
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("assets.json")).unwrap()).unwrap();
    let mut config =
        String::from("format 2\nrenderer\napplication examples/gpu_renderer/rcc/changes.rs\n");
    let mut resources = Vec::new();
    for r in manifest["resources"].as_array().unwrap() {
        let name = r["file"].as_str().unwrap();
        let address = r["address"].as_u64().unwrap() as usize;
        config.push_str(&format!(
            "copy GPU_RENDERER_{} {address} {name}\n",
            name.trim_end_matches(".bin").to_uppercase()
        ));
        resources.push((address, std::fs::read(out.join(name)).unwrap()));
    }
    let project = cpu_v3_tang_nano_20k::boot::parse_boot_project(&config).unwrap();
    let constants =
        project.constants() + &std::fs::read_to_string(out.join("renderer_assets.rs")).unwrap();
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let source = std::fs::read_to_string(example.join("rcc/changes.rs")).unwrap();
    let ir = resolver::compile_renderer_program(
        "actual renderer changes",
        &source,
        &options,
        &example.join("rcc"),
        &root.join("rcc"),
        &constants,
        &mut |_| {},
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let program = rcc_backend::try_compile(ir, &options, "renderer_changed_frames").unwrap();
    (program, resources)
}
fn preload(memory: &mut [u16], resources: &[(usize, Vec<u8>)]) {
    for (address, data) in resources {
        for (i, byte) in data.iter().enumerate() {
            let at = address + i;
            let word = &mut memory[at / 2];
            *word = (*word & !(255 << ((at % 2) * 8))) | (u16::from(*byte) << ((at % 2) * 8));
        }
    }
}
#[test]
fn actual_changed_application_records_completed_sequential_passes() {
    let (program, resources) = application("functional");
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    preload(cpu.physical_memory_mut(), &resources);
    let state = attach(&mut cpu, Some(vec![]));
    {
        let mut s = state.borrow_mut();
        s.automatic = true;
        s.capture_dynamic = true;
        s.expected_depth = Some(65535);
    }
    let meta = global(&program, "GPU_PUI_PAGES");
    let work = global(&program, "GPU_SCENE_WORK");
    let mut held: [Option<Vec<u16>>; 2] = [None, None];
    let mut markers = Vec::new();
    let mut counters = Vec::new();
    let mut halted = false;
    for _ in 0..10_000_000 {
        let result = cpu.step().unwrap();
        let memory = cpu.physical_memory_mut();
        for (slot, snapshot) in held.iter_mut().enumerate() {
            let page = &memory[PAGE + slot * 512..PAGE + (slot + 1) * 512];
            if memory[meta + slot * 4] >= 2 {
                if let Some(old) = snapshot {
                    assert_eq!(page, old, "modified recorded/inflight UI page{slot}");
                } else {
                    *snapshot = Some(page.to_vec());
                }
            } else {
                *snapshot = None;
            }
        }
        match result {
            StepOutcome::Signaled(e) => {
                markers.push((e.signal_type, e.value));
                if e.signal_type == 3 {
                    counters.push(memory[work..work + 8].to_vec());
                }
            }
            StepOutcome::Halted { signal } => {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
            _ => {}
        }
    }
    assert!(halted);
    assert_eq!(markers.len(), 33);
    for (frame, events) in markers.as_chunks::<3>().0.iter().enumerate() {
        assert_eq!(
            events,
            &[(1, frame as u16), (2, frame as u16), (3, frame as u16)]
        );
    }
    assert_eq!(counters[0], counters[1], "static scene recomputed");
    assert_eq!(counters[2], [1, 2, 1, 1, 2, 4, 2, 1]);
    assert_eq!(counters[3], counters[2]);
    assert_eq!(counters[4], [3, 4, 2, 3, 3, 8, 6, 1]);
    assert_eq!(counters[5], [3, 4, 2, 4, 3, 9, 7, 1]);
    for counter in &counters[6..] {
        assert_eq!(
            counter, &counters[5],
            "UI-only/static frame recomputed scene"
        );
    }
    assert_eq!(cpu.data_segments(), [0, 1, 2, 3]);
    check_native_frames(&state.borrow());
}

const PAGE: usize = 0x7df800 / 2;
fn global(program: &rcc_backend::CpuV3Program, name: &str) -> usize {
    match program
        .debug
        .globals
        .iter()
        .find(|v| v.name == name)
        .unwrap()
        .loc
    {
        rcc::VarLoc::Global(a) => usize::from(a),
        _ => panic!("global"),
    }
}
#[derive(Clone)]
struct Draw {
    camera: scene::Case,
    far: u32,
    light: [f64; 3],
    rgba: [u8; 4],
    shininess: u32,
    specular: u32,
}
fn draw_case(frame: usize, right: bool, new_model: bool, new_view: bool) -> Draw {
    let changed_view = frame >= 5 || new_view;
    let changed_material = frame >= 5 || new_model;
    Draw {
        camera: scene::Case {
            eye: if frame >= 2 { [0.25, 0., 0.] } else { [0.; 3] },
            camera: if changed_view {
                [0., 0.25, 0.]
            } else if frame == 4 {
                [0., 0.125, 0.]
            } else {
                [0.; 3]
            },
            position: if right {
                [1., 0., -4.]
            } else if changed_material {
                [0.5, 0., -3.]
            } else {
                [-1., 0., -3.]
            },
            angles: if !right && changed_material {
                [0., 0., 0.125]
            } else {
                [0.; 3]
            },
            scale: if !right && changed_material {
                [1.25, 1., 1.]
            } else {
                [1.; 3]
            },
            focal: if changed_view { 144. } else { 200. },
        },
        far: if changed_view { 180 } else { 199 },
        light: if frame >= 5 { [1.; 3] } else { [0., 0., 1.] },
        rgba: if right {
            [255, 128, 64, 128]
        } else if changed_material {
            [32, 160, 255, 255]
        } else {
            [255; 4]
        },
        shininess: if !right && changed_material { 8 } else { 12 },
        specular: if !right && changed_material {
            0x804020
        } else {
            0xa0a0a0
        },
    }
}
fn expected_draws(frame: usize) -> Vec<Draw> {
    let mut draws = vec![
        draw_case(frame, false, false, false),
        draw_case(frame, true, false, false),
    ];
    if frame == 4 {
        draws.push(draw_case(frame, false, true, false));
        draws.push(draw_case(frame, true, false, true));
    }
    draws
}
fn words32(words: &[u16]) -> Vec<u32> {
    words
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u32::from(p[0]) | (u32::from(p[1]) << 16))
        .collect()
}
fn check_draw(expected: &Draw, global: &[u16], render: &[u16], instance: &[u16]) {
    let g = words32(global);
    let r = words32(render);
    let i = words32(instance);
    assert_eq!(g[0], expected.far << 16);
    assert_eq!(g[4], 64 | (192 << 16));
    assert_eq!(g[5], 3 | (expected.shininess << 2));
    assert!(g[6..].iter().all(|x| *x == 0));
    let rotation = scene::rotation(expected.camera.camera);
    let length = expected.light.iter().map(|v| v * v).sum::<f64>().sqrt();
    for axis in 0..3 {
        let component = (0..3)
            .map(|k| rotation[k][axis] * expected.light[k] / length)
            .sum::<f64>();
        let actual = global[2 + axis] as i16;
        assert!(
            (f64::from(actual) - component * 16384.).abs() < 10.,
            "light {axis}: {actual} vs {component}"
        );
    }
    assert!((f64::from(global[5] as i16) + 100. / expected.camera.focal * 16384.).abs() < 3.);
    assert!((f64::from(global[6] as i16) + 60. / expected.camera.focal * 16384.).abs() < 3.);
    assert_eq!(global[7], 8192);
    assert_eq!(r[0], 0x9620 | (2 << 21));
    assert_eq!(r[1], expected.specular);
    assert_eq!(r[2], if expected.rgba[3] == 255 { 9 } else { 17 });
    assert!(r[3..].iter().all(|x| *x == 0));
    let (position, normal) = scene::reference(expected.camera);
    for row in 0..3 {
        for (column, value) in position[row].iter().enumerate() {
            let actual = scene::fixed(instance, 2 * (row * 4 + column));
            assert!(
                (actual - value).abs() < 0.0012,
                "position {row},{column}: {actual} vs {value}"
            );
        }
        for (column, value) in normal[row].iter().enumerate() {
            let actual = scene::fixed(instance, 24 + 2 * (row * 3 + column));
            assert!(
                (actual - value).abs() < 0.0006,
                "rotation {row},{column}: {actual} vs {value}"
            );
        }
    }
    assert_eq!(i[21], 65536);
    assert_eq!(i[22], u32::from_le_bytes(expected.rgba));
    assert!(i[23..].iter().all(|x| *x == 0));
}
fn check_native_frames(s: &State) {
    assert_eq!((s.starts.len(), s.swaps, s.maintenance), (11, 11, 11));
    for frame in 0..11 {
        let (base, words, _) = &s.starts[frame];
        let object_base = if *base == 0x9200 { 0x4a0000 } else { 0x7f0000 };
        let page = if frame % 2 == 0 { 0x7df800 } else { 0x7dfc00 };
        let objects = &s.object_snapshots[frame];
        let ui = &s.ui_snapshots[frame];
        let valid = words32(words);
        let end = valid.iter().position(|w| *w == Command::END.0).unwrap();
        let commands = &valid[..=end];
        assert_eq!(
            end + 1,
            s.valid_command_lengths[frame],
            "END must be last valid command"
        );
        assert_eq!(
            commands.iter().filter(|w| **w >> 28 == 8).count(),
            2,
            "Pass reopened a clear"
        );
        assert_eq!(
            commands.iter().filter(|w| **w >> 28 == 10).count(),
            1,
            "Pass reopened a target"
        );
        assert_eq!(
            &commands[commands.len() - 3..],
            &[
                Command::wait(true, true).0,
                Command::FLUSH.0,
                Command::END.0
            ]
        );
        assert!(commands.contains(&Command::clear(false, 0x1111 + frame as u16).0));
        let mut local = vec![0u16; 1536];
        let mut global = vec![];
        let mut render = vec![];
        let mut dma_pending = false;
        let mut render_wait = true;
        let mut mesh_ready = false;
        let expected = expected_draws(frame);
        let mut draws = 0;
        let mut ranges = Vec::new();
        for (at, &command) in commands.iter().enumerate() {
            match command >> 28 {
                1 => {
                    let source = (((command >> 10) & 0x3ffff) * 32) as usize;
                    let destination = (((command >> 3) & 127) * 16) as usize;
                    let size = (32 << ((command >> 1) & 3)) / 2;
                    let data = if (object_base..object_base + 65536).contains(&source) {
                        &objects
                            [(source - object_base) / 2..(source - object_base) / 2 + size as usize]
                    } else {
                        assert!((page..page + 1024).contains(&source));
                        &ui[(source - page) / 2..(source - page) / 2 + size as usize]
                    };
                    if destination == 264 * 4 || destination == 272 * 4 {
                        assert!(render_wait);
                        assert_eq!(
                            commands[at - 1],
                            Command::wait(true, true).0,
                            "binding overwrite without full wait"
                        );
                    }
                    local[destination..destination + size as usize].copy_from_slice(data);
                    dma_pending = true;
                }
                2 => {
                    assert_eq!(command, Command::dma_meshlet(0x4b0080, 0, 128).unwrap().0);
                    mesh_ready = true;
                    dma_pending = true;
                }
                3 => {
                    assert!(!dma_pending && mesh_ready);
                    assert_eq!(command, Command::draw_meshlet(0).unwrap().0);
                    check_draw(&expected[draws], &global, &render, &local[1280..1344]);
                    draws += 1;
                    render_wait = false;
                }
                4 => {
                    if command & 1 != 0 {
                        dma_pending = false;
                    }
                    if command & 2 != 0 {
                        render_wait = true;
                    }
                }
                5 | 6 => {
                    assert!(!dma_pending && render_wait);
                    let start = ((command & 127) * 16) as usize;
                    if command >> 28 == 5 {
                        global = local[start..start + 32].to_vec();
                    } else {
                        render = local[start..start + 16].to_vec();
                    }
                }
                11 => {
                    assert!(!dma_pending && render_wait);
                    let start = ((command & 127) * 16) as usize;
                    let desc: Vec<u8> = local[start..start + 16]
                        .iter()
                        .flat_map(|w| w.to_le_bytes())
                        .collect();
                    assert_eq!(
                        desc,
                        gpu_v2::system::vliw_command::ui_params(page as u32, 0x4c0000).unwrap()
                    );
                    ranges.push((
                        ((command >> 7) & 1023) as u16,
                        (((command >> 17) & 1023) + 1) as u16,
                    ));
                    mesh_ready = false;
                }
                0 | 7 | 8 | 10 | 15 => {}
                op => panic!("unexpected opcode{op}"),
            }
        }
        assert_eq!(draws, expected.len());
        assert_eq!(
            ranges,
            if frame == 5 {
                vec![(10, 18)]
            } else {
                vec![(0, 8), (10, 18)]
            }
        );
        let mut cells = vec![255u8; 750];
        let number: &[u8] = if frame < 4 {
            b"12345"
        } else if frame == 4 {
            b"7"
        } else if frame == 5 {
            b""
        } else {
            b"42"
        };
        cells[..number.len()].copy_from_slice(number);
        let stats: &[u8] = if frame < 2 { b"OBS --.-" } else { b"OBS 0.0" };
        cells[10..10 + stats.len()].copy_from_slice(stats);
        let packed: Vec<u16> = cells
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u16::from_le_bytes([p[0], p[1]]))
            .collect();
        assert_eq!(&ui[..375], packed);
        assert!(ui[375..384].iter().all(|x| *x == 65535));
    }
}

// Production core/FPU, separate instruction ROM and a one-cycle word-data peer.
// No fetch queue, cache, SDRAM, real GPU or Display timing is represented here.
struct ObservedCore {
    state: Rc<RefCell<cpu_v3::CpuV3CoreState>>,
    input: cpu_v3::CpuV3CoreInput,
    output: cpu_v3::CpuV3CoreOutput,
}
impl digital_design_circuit::External for ObservedCore {
    fn execute(&mut self, circuit: &mut digital_design_circuit::CircuitWires) {
        use digital_design_hardware::Module;
        cpu_v3::CpuV3Core::execute_emu(
            &mut self.state.borrow_mut(),
            circuit,
            &self.input,
            &self.output,
        );
    }
    fn clock(&mut self, circuit: &mut digital_design_circuit::CircuitWires) {
        use digital_design_hardware::Module;
        cpu_v3::CpuV3Core::clock_emu(
            &mut self.state.borrow_mut(),
            circuit,
            &self.input,
            &self.output,
        );
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
#[derive(Clone, Copy, Debug)]
struct CostMarker {
    kind: u8,
    frame: u16,
    cycle: usize,
    retired: u32,
    stores: usize,
    ui: usize,
}
#[test]
fn actual_compiled_changes_run_on_the_production_core_and_report_bounded_recording_costs() {
    use digital_design_hardware::ModuleIo;
    let (program, resources) = application("core");
    let mut memory = vec![0u16; 1 << 22];
    preload(&mut memory, &resources);
    let meta = global(&program, "GPU_PUI_PAGES");
    let pipe = global(&program, "GPU_PIPE");
    let state = Rc::new(RefCell::new(State {
        active: 0x200000,
        automatic: true,
        capture_dynamic: true,
        persistent_ranges: Some(vec![]),
        expected_depth: Some(65535),
        ..Default::default()
    }));
    let mut peers: Vec<Peer> = (0..8)
        .map(|kind| Peer {
            kind,
            state: state.clone(),
        })
        .collect();
    let core = Rc::new(RefCell::new(cpu_v3::CpuV3CoreState::default()));
    let (mut circuit, (input, output)) = digital_design_circuit::build_circuit(|| {
        let input = cpu_v3::CpuV3CoreInput::allocate();
        let output = cpu_v3::CpuV3CoreOutput::allocate();
        digital_design_circuit::external(ObservedCore {
            state: core.clone(),
            input: input.clone(),
            output: output.clone(),
        });
        (input, output)
    });
    let mut instruction_response = None;
    let mut data_response = None;
    let mut markers = Vec::new();
    let mut stores = 0;
    let mut ui = 0;
    let mut halted = false;
    for cycle in 0..12_000_000 {
        let mut values = cpu_v3::CpuV3CoreInputValue {
            reset: false,
            hold: false,
            instruction_request_ready: true,
            instruction_response_valid: instruction_response.is_some(),
            instruction_data: instruction_response.unwrap_or(0),
            instruction_error: false,
            data_request_ready: true,
            data_response_valid: data_response.is_some(),
            data_read_data: data_response.unwrap_or(0),
            data_error: false,
            data_line_copy_ready: true,
            data_cache_command_error: false,
            device_read_data: 0,
        };
        input.drive(&mut circuit, &values);
        circuit.execute_gates();
        let request = output.sample(&circuit);
        if request.device_read_enable {
            values.device_read_data = u64::from(
                peers[request.device_index as usize]
                    .read(&mut memory, request.device_channel as u8),
            );
            input.drive(&mut circuit, &values);
            circuit.execute_gates();
        }
        let value = output.sample(&circuit);
        assert!(
            !value.fault,
            "core fault={} pc={}",
            value.fault_code, value.fault_pc
        );
        if value.halted {
            assert_eq!(value.halt_signal, 0x600d);
            halted = true;
            break;
        }
        assert!(
            !value.data_line_copy_valid && !value.data_line_clean_valid,
            "fixed-port cost fixture does not model LCOPY/LCLEAN"
        );
        if value.device_write_enable {
            peers[value.device_index as usize].write(
                &mut memory,
                value.device_channel as u8,
                value.device_write_data as u16,
            );
        }
        let next_instruction = value.instruction_request_valid.then(|| {
            assert_eq!(value.instruction_address >> 16, 0);
            u64::from(program.words[value.instruction_address as usize])
        });
        let next_data = value.data_request_valid.then(|| {
            let address = value.data_address as usize;
            assert!(address < memory.len());
            if value.data_write {
                if (PAGE..PAGE + 1024).contains(&address) {
                    let slot = (address - PAGE) / 512;
                    let word = (address - PAGE) % 512;
                    assert_eq!(
                        memory[meta + slot * 4],
                        1,
                        "core wrote a non-recording UI page"
                    );
                    assert_eq!(usize::from(memory[pipe + 1]), slot + 1);
                    assert!(word < 400);
                    ui += 1;
                }
                memory[address] = value.data_write_data as u16;
                stores += 1;
                0
            } else {
                u64::from(memory[address])
            }
        });
        circuit.clock_tick();
        instruction_response = next_instruction;
        data_response = next_data;
        let event = core.borrow_mut().take_signal_event();
        if let Some(event) = event {
            markers.push(CostMarker {
                kind: event.signal_type,
                frame: event.value,
                cycle: cycle + 1,
                retired: core.borrow().retired_words(),
                stores,
                ui,
            });
        }
    }
    assert!(halted, "core exceeded bounded cost fixture");
    assert_eq!(markers.len(), 33);
    assert_eq!(core.borrow().data_segments(), [0, 1, 2, 3]);
    check_native_frames(&state.borrow());
    let mut rows = Vec::new();
    let mut page_stores = Vec::new();
    for (frame, m) in markers.as_chunks::<3>().0.iter().enumerate() {
        assert_eq!([m[0].kind, m[1].kind, m[2].kind], [1, 2, 3]);
        assert!(m.iter().all(|x| usize::from(x.frame) == frame));
        let word_stores = m[2].ui - m[0].ui;
        page_stores.push(word_stores);
        rows.push(serde_json::json!({"frame":frame,"record_cycles":m[1].cycle-m[0].cycle,"seal_advance_cycles":m[2].cycle-m[1].cycle,"record_retired_words":m[1].retired-m[0].retired,"seal_advance_retired_words":m[2].retired-m[1].retired,"record_stores":m[1].stores-m[0].stores,"seal_advance_stores":m[2].stores-m[1].stores,"ui_stores":word_stores}));
    }
    assert_eq!(page_stores, [400, 400, 2, 2, 3, 3, 1, 1, 0, 0, 0]);
    let out = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-renderer-round2-20261026/cpu-costs.json");
    std::fs::write(out,serde_json::to_string_pretty(&serde_json::json!({"model":"production CPU/FPU core emulator; separate instruction ROM and one-cycle data/control peers; no caches/SDRAM/GPU/Display timing","window":"record: after renderer_frame to before frame_complete; seal_advance: frame_complete through its return; includes model-only SIGNAL markers","frames":rows})).unwrap()).unwrap();
}
