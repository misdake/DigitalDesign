//! Round3 actual Bloxorz-shaped caller, independent geometry and fixed-port costs.
use super::changes::{global, preload, run_core};
use super::*;
use cpu_v3::StepOutcome;
#[allow(dead_code)]
#[path = "../../examples/bloxorz/geometry.rs"]
mod geometry;
fn application(name: &str) -> (rcc_backend::CpuV3Program, Vec<(usize, Vec<u8>)>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let example = root.join("examples/gpu_renderer_bloxorz");
    let out = root
        .join("../../target/gpu-renderer-round3-20261026")
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
        String::from("format 2\nrenderer\napplication examples/gpu_renderer_bloxorz/rcc/main.rs\n");
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
    let source = std::fs::read_to_string(example.join("rcc/main.rs")).unwrap();
    let mut dependencies = Vec::new();
    let ir = resolver::compile_renderer_program_with_sources(
        "actual Bloxorz renderer",
        &source,
        &options,
        &example.join("rcc"),
        &root.join("rcc"),
        &constants,
        &[root.join("examples/bloxorz/rcc")],
        &mut |p| dependencies.push(p.to_owned()),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(dependencies.contains(&root.join("examples/bloxorz/rcc/bloxorz_core.rs")));
    let program = rcc_backend::try_compile(ir, &options, "renderer_game_frames").unwrap();
    (program, resources)
}

const FRAMES: usize = 22;
fn canceled(t: usize) -> bool {
    t == 8 || t == 17
}
fn stimulus(t: usize, s: &mut State) {
    // Independent host events advance time; peripheral read count never does.
    s.now_ms = (t * 25) as u16;
    let buttons: u16 = match t {
        1 | 7 | 9 => 32,
        12 | 15 => 8,
        13 | 16 => 4096,
        _ => 0,
    };
    let raw = buttons ^ 65535;
    s.input_status = if canceled(t) {
        0
    } else if t == 10 || t >= 20 {
        1
    } else {
        3
    };
    s.input_bytes = [128, 128, 0x73, 0x5a, raw & 255, raw >> 8, 128, 128];
}
fn state_expected(t: usize) -> [u16; 10] {
    let (x, orientation, mask, moves) = if (5..11).contains(&t) {
        (1, 1, 1, 1)
    } else if t == 11 {
        (3, 0, 3, 2)
    } else {
        (0, 0, 0, 0)
    };
    [x, 1, orientation, mask, 0, moves, 0, 0, 0, 0]
}
fn check_state(memory: &[u16], program: &rcc_backend::CpuV3Program, t: usize) {
    let at = global(program, "GAME");
    assert_eq!(&memory[at..at + 10], &state_expected(t), "game tick{t}");
    let motion = global(program, "MOTION");
    assert_eq!(
        memory[motion + 6],
        u16::from((13..16).contains(&t)),
        "level tick{t}"
    );
    let input = global(program, "GPU_RENDERER_INPUT");
    assert_eq!(memory[input], u16::from(!canceled(t)));
    assert_eq!(
        memory[input + 2],
        match t {
            1 | 7 | 9 | 10 => 32,
            12 | 15 => 8,
            13 | 16 => 4096,
            _ => 0,
        },
        "held buttons tick{t}"
    );
    assert_eq!(
        memory[input + 3],
        match t {
            1 | 7 => 32,
            12 | 15 => 8,
            13 | 16 => 4096,
            _ => 0,
        },
        "observed edge tick{t}"
    );
}
fn mesh_entries(resources: &[(usize, Vec<u8>)]) -> Vec<(u16, u16)> {
    let data = &resources[0].1;
    // Count from the first record's sector, independently of the catalog table.
    let first = u16::from_le_bytes([data[0], data[1]]) as usize * 128 - 0x4b0000;
    (0..first / 4)
        .map(|i| {
            (
                u16::from_le_bytes([data[4 * i], data[4 * i + 1]]),
                u16::from_le_bytes([data[4 * i + 2], data[4 * i + 3]]),
            )
        })
        .take_while(|e| e.0 != 0)
        .collect()
}
fn pose(t: usize) -> scene::Case {
    let (orient, x, fraction) = if (1..5).contains(&t) {
        (0, 0, Some((t - 1) as f64 / 4.))
    } else if (7..11).contains(&t) {
        (1, 1, Some((t - 7) as f64 / 4.))
    } else {
        let s = state_expected(t);
        (s[2] as u8, s[0], None)
    };
    let m = geometry::roll(orient, 0, fraction.unwrap_or(0.), 0.);
    let size = match orient {
        0 => [1., 2., 1.],
        1 => [2., 1., 1.],
        _ => [1., 1., 2.],
    };
    let angle = -std::f64::consts::FRAC_PI_2 * fraction.unwrap_or(0.);
    scene::Case {
        eye: [0., 5., 12.],
        camera: [-0.5, 0., 0.],
        focal: 200.,
        position: [m[0][3] + f64::from(x) - 4., m[1][3], m[2][3] + 1. - 1.5],
        angles: [0., 0., angle],
        scale: size,
    }
}
fn check_instance(words: &[u16], case: scene::Case) {
    let (position, normal) = scene::reference(case);
    for (r, (pr, nr)) in position.iter().zip(&normal).enumerate() {
        for (c, expected) in pr.iter().enumerate() {
            let actual = scene::fixed(words, 2 * (r * 4 + c));
            assert!(
                (actual - expected).abs() < 0.005,
                "position {r},{c}: {actual} != {}",
                expected
            );
        }
        for (c, expected) in nr.iter().enumerate() {
            let actual = scene::fixed(words, 24 + 2 * (r * 3 + c));
            assert!(
                (actual - expected).abs() < 0.0012,
                "normal {r},{c}: {actual} != {}",
                expected
            );
        }
    }
    assert_eq!(&words[42..46], &[0, 1, 65535, 65535]);
    assert!(words[46..].iter().all(|x| *x == 0));
}
fn check_frames(state: &State, resources: &[(usize, Vec<u8>)]) {
    let entries = mesh_entries(resources);
    // Batching by 3x3 cells, maximum five cells/meshlet: 5 floor0, 6 floor1, bridge, block.
    assert_eq!(entries.len(), 13);
    assert_eq!(
        (state.starts.len(), state.swaps, state.maintenance),
        (20, 20, 20)
    );
    let mut sent = 0;
    for t in 0..FRAMES {
        if canceled(t) {
            continue;
        }
        let (base, words, _) = &state.starts[sent];
        assert_eq!(*base, if sent % 2 == 0 { 0x9200 } else { 0xfc00 });
        let objbase = if *base == 0x9200 { 0x4a0000 } else { 0x7f0000 };
        let page = if sent % 2 == 0 { 0x7df800 } else { 0x7dfc00 };
        let objs = &state.object_snapshots[sent];
        let ui = &state.ui_snapshots[sent];
        let commands: Vec<u32> = words
            .as_chunks::<2>()
            .0
            .iter()
            .map(|x| u32::from(x[0]) | (u32::from(x[1]) << 16))
            .collect();
        let count = state.valid_command_lengths[sent];
        let commands = &commands[..count];
        assert_eq!(commands[count - 1], Command::END.0);
        assert_eq!(
            &commands[count - 3..],
            &[
                Command::wait(true, true).0,
                Command::FLUSH.0,
                Command::END.0
            ]
        );
        assert_eq!(commands.iter().filter(|c| **c >> 28 == 10).count(), 1);
        assert_eq!(commands.iter().filter(|c| **c >> 28 == 8).count(), 2);
        assert!(commands.contains(&Command::clear(false, 0x2222 + t as u16).0));
        let floor = if (13..16).contains(&t) { 5..11 } else { 0..5 };
        let mut expected: Vec<usize> = floor.collect();
        if state_expected(t)[3] & 1 != 0 {
            expected.push(11);
        }
        expected.push(12);
        let mut local = vec![0u16; 1536];
        let mut pending = false;
        let mut render_idle = true;
        let mut slots = [None, None];
        let mut prefetch = 0;
        let mut draws = 0;
        let mut ranges = Vec::new();
        for (at, &c) in commands.iter().enumerate() {
            match c >> 28 {
                1 => {
                    let src = (((c >> 10) & 0x3ffff) * 32) as usize;
                    let dst = (((c >> 3) & 127) * 16) as usize;
                    let n = (32 << ((c >> 1) & 3)) / 2;
                    let data = if (objbase..objbase + 65536).contains(&src) {
                        &objs[(src - objbase) / 2..(src - objbase) / 2 + n as usize]
                    } else {
                        assert!((page..page + 1024).contains(&src));
                        &ui[(src - page) / 2..(src - page) / 2 + n as usize]
                    };
                    if dst == 1056 || dst == 1088 {
                        assert!(render_idle);
                        assert_eq!(commands[at - 1], Command::wait(true, true).0);
                    }
                    local[dst..dst + n as usize].copy_from_slice(data);
                    pending = true;
                }
                2 => {
                    let index = expected[prefetch];
                    let (sector, sectors) = entries[index];
                    let slot = (c & 1) as usize;
                    assert_eq!(
                        c,
                        Command::dma_meshlet(
                            u32::from(sector) * 128,
                            slot as u8,
                            u32::from(sectors) * 128
                        )
                        .unwrap()
                        .0
                    );
                    assert!(slots[slot].is_none());
                    slots[slot] = Some((index, false));
                    prefetch += 1;
                }
                3 => {
                    assert!(!pending);
                    let slot = (c & 1) as usize;
                    assert_eq!(slots[slot].take(), Some((expected[draws], true)));
                    assert_eq!(c, Command::draw_meshlet(slot as u8).unwrap().0);
                    let case = if expected[draws] == 12 {
                        pose(t)
                    } else {
                        scene::Case {
                            eye: [0., 5., 12.],
                            camera: [-0.5, 0., 0.],
                            focal: 200.,
                            position: [0.; 3],
                            angles: [0.; 3],
                            scale: [0.5; 3],
                        }
                    };
                    check_instance(&local[1280..1344], case);
                    draws += 1;
                    render_idle = false;
                }
                4 => {
                    if c & 1 != 0 {
                        pending = false;
                        for s in slots.iter_mut().flatten() {
                            s.1 = true;
                        }
                    }
                    if c & 2 != 0 {
                        render_idle = true;
                    }
                }
                5 | 6 => {
                    assert!(!pending && render_idle);
                    let offset = ((c & 127) * 16) as usize;
                    if c >> 28 == 5 {
                        let g = &local[offset..offset + 32];
                        assert_eq!(&g[..2], &[0, 100]);
                        assert_eq!(&g[8..12], &[64, 192, 51, 0]);
                        assert!(g[12..].iter().all(|w| *w == 0));
                        let rotation = scene::rotation([-0.5, 0., 0.]);
                        for axis in 0..3 {
                            assert!(
                                (f64::from(g[2 + axis] as i16) - rotation[2][axis] * 16384.).abs()
                                    < 10.
                            );
                        }
                        assert_eq!(&g[5..8], &[(-8192i16) as u16, (-4915i16) as u16, 8192]);
                    } else {
                        assert_eq!(
                            &local[offset..offset + 16],
                            &[0x97e0, 64, 0xa0a0, 0x00a0, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
                        );
                    } // texture 0x4bf000/128 + RGB565 mode
                }
                11 => {
                    assert!(!pending && render_idle);
                    let offset = ((c & 127) * 16) as usize;
                    let descriptor: Vec<u8> = local[offset..offset + 16]
                        .iter()
                        .flat_map(|w| w.to_le_bytes())
                        .collect();
                    assert_eq!(
                        descriptor,
                        gpu_v2::system::vliw_command::ui_params(page as u32, 0x4c0000).unwrap()
                    );
                    ranges.push((((c >> 7) & 1023) as u16, (((c >> 17) & 1023) + 1) as u16));
                }
                0 | 7 | 8 | 10 | 15 => {}
                op => panic!("unexpected{op}"),
            }
        }
        assert_eq!(draws, expected.len(), "tick{t}");
        assert_eq!(ranges, [(0, 4), (6, 7)]);
        let mut cells = vec![255u8; 750];
        let moves = state_expected(t)[5].to_string();
        cells[..moves.len()].copy_from_slice(moves.as_bytes());
        cells[6] = b'1';
        let packed: Vec<u16> = cells
            .as_chunks::<2>()
            .0
            .iter()
            .map(|x| u16::from_le_bytes([x[0], x[1]]))
            .collect();
        assert_eq!(&ui[..375], packed, "tick{t} UI");
        assert!(ui[375..384].iter().all(|x| *x == 65535));
        sent += 1;
    }
}
#[test]
fn actual_game_caller_cancels_disconnected_frames_and_keeps_native_snapshots() {
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
    let mut counters = Vec::new();
    let mut halted = false;
    for _ in 0..12_000_000 {
        let result = cpu.step().unwrap();
        let memory = cpu.physical_memory_mut();
        for (slot, old) in held.iter_mut().enumerate() {
            let page = &memory[0x7df800 / 2 + slot * 512..0x7df800 / 2 + (slot + 1) * 512];
            if memory[meta + slot * 4] >= 2 {
                if let Some(before) = old {
                    assert_eq!(page, before);
                } else {
                    *old = Some(page.to_vec());
                }
            } else {
                *old = None;
            }
        }
        match result {
            StepOutcome::Signaled(e) => {
                let t = usize::from(e.value);
                if e.signal_type == 4 {
                    stimulus(t, &mut state.borrow_mut());
                }
                if e.signal_type == 3 {
                    check_state(memory, &program, t);
                    counters.push(memory[work..work + 8].to_vec());
                }
            }
            StepOutcome::Halted { signal } => {
                assert_eq!(
                    signal,
                    0x600d,
                    "meshes={:?} entries={:?}",
                    &memory[global(&program, "GPU_SCENE_MESHES")
                        ..global(&program, "GPU_SCENE_MESHES") + 96],
                    &memory[global(&program, "GPU_SCENE_ENTRIES")
                        ..global(&program, "GPU_SCENE_ENTRIES") + 32]
                );
                halted = true;
                break;
            }
            _ => {}
        }
    }
    assert!(halted);
    assert_eq!(counters.len(), FRAMES);
    assert!(counters.iter().all(|c| c[..4] == [1, 1, 1, 1] && c[7] == 4));
    assert_eq!(counters[5], counters[6], "steady same scene recomputed");
    assert_eq!(counters[18], counters[21], "static end recomputed");
    assert_eq!(cpu.data_segments(), [0, 1, 2, 3]);
    check_frames(&state.borrow(), &resources);
}
#[test]
fn actual_game_core_records_same_workload_preparation_costs_and_checks_every_held_write() {
    let (program, resources) = application("core");
    let (markers, state, _) = run_core(&program, &resources, |kind, t, m, s| {
        if kind == 4 {
            stimulus(usize::from(t), s);
        }
        if kind == 3 {
            check_state(m, &program, usize::from(t));
        }
    });
    check_frames(&state.borrow(), &resources);
    let mut rows = Vec::new();
    let mut draw_costs = Vec::new();
    for t in 0..FRAMES {
        let events: Vec<_> = markers
            .iter()
            .filter(|m| usize::from(m.frame) == t)
            .collect();
        let event = |kind| *events.iter().find(|m| m.kind == kind).unwrap();
        let start = event(4);
        let acquired = event(1);
        let updated = event(5);
        let posed = event(6);
        let draw = event(7);
        let returned = event(3);
        let geometry = event(9).cycle - draw.cycle;
        draw_costs.push(geometry);
        if t >= 20 {
            assert_eq!(returned.ui - start.ui, 0, "converged UI tick{t}");
        }
        let completed = events.iter().any(|m| m.kind == 2);
        assert_eq!(completed, !canceled(t));
        rows.push(serde_json::json!({"tick":t,"canceled":!completed,"acquire_input_cycles":acquired.cycle-start.cycle,"input_rules_cycles":updated.cycle-acquired.cycle,"pose_setter_cycles":posed.cycle-updated.cycle,"ui_edit_cycles":draw.cycle-posed.cycle,"geometry_record_cycles":geometry,"ui_record_cycles":if completed{event(2).cycle-event(9).cycle}else{0},"cancel_cycles":if completed{0}else{returned.cycle-event(9).cycle},"seal_advance_cycles":if completed{returned.cycle-event(2).cycle}else{0},"retired_words":returned.retired-start.retired,"stores":returned.stores-start.stores,"ui_stores":returned.ui-start.ui}));
    }
    assert!(
        draw_costs[5] > draw_costs[6],
        "changed/steady matched floor+bridge+block preparation"
    );
    assert_eq!(
        draw_costs[19], draw_costs[21],
        "same workload/slot steady preparation differs"
    );
    let out = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-renderer-round3-20261026/cpu-costs.json");
    std::fs::write(out,serde_json::to_string_pretty(&serde_json::json!({"model":"production CPU/FPU core, separate instruction ROM and one-cycle word data/control; no cache/SDRAM/GPU/Display timing","window":"host event time only; acquire includes waits/input capture; rules includes frame queries; pose setter; UI edits; geometry recording includes cached preparation, comparisons and object copies; seal includes advance; model SIGNAL overhead included; canceled geometry not compared","frames":rows})).unwrap()).unwrap();
}
