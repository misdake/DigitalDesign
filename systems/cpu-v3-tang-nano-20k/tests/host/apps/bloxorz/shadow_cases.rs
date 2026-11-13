use super::scenarios;
use super::*;

#[test]
fn native_triangle_instances_match_independent_web_shadow_clipping_samples() {
    use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../examples/gpu_renderer_bloxorz/assets/web-shadow-reference.json"
    ))
    .unwrap();
    let mut results = vec![];
    for case in fixture["cases"].as_array().unwrap() {
        let name = format!("web-shadow-{}", case["name"].as_str().unwrap());
        let (program, resources, out) = application_case(
            &name,
            None,
            None,
            scenarios::web_shadow(case["name"].as_str().unwrap()),
        );
        let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
        sim.cpu_mut().set_boot_window(&program.words);
        preload(sim.cpu_mut().physical_memory_mut(), &resources);
        let mut halted = false;
        let mut steps = 0;
        let mut push = 0;
        let mut command_words = 0;
        for _ in 0..2_000_000 {
            if sim.waiting_for_vblank() {
                sim.advance_vblank();
            }
            steps += 1;
            match sim.step().unwrap() {
                StepOutcome::Halted { signal } => {
                    assert_eq!(signal, 0x600d);
                    halted = true;
                    break;
                }
                StepOutcome::Signaled(event) if event.signal_type == 14 => push = event.value,
                StepOutcome::Signaled(event) if event.signal_type == 15 => {
                    command_words = event.value
                }
                _ => {}
            }
        }
        assert!(halted, "bounded shadow sample timeout");
        assert!(sim.gpu().last_error().is_none());
        assert!(push < 32768);
        let memory = sim.cpu().physical_memory_slice();
        let info = global(&program, "SHADOW_INFO");
        let stats = memory[info..info + 3].to_vec();
        assert_eq!(
            usize::from(stats[1]),
            case["cells"].as_array().unwrap().len(),
            "{name} receivers"
        );

        let at = global(&program, "SHADOW_HULL");
        let actual = (0..usize::from(stats[0]))
            .map(|i| {
                [
                    scene::fixed(memory, at + i * 4),
                    scene::fixed(memory, at + i * 4 + 2),
                ]
            })
            .collect::<Vec<_>>();
        let expected = case["hull"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| [p[0].as_f64().unwrap(), p[1].as_f64().unwrap()])
            .collect::<Vec<_>>();
        let error = actual
            .iter()
            .map(|p| hull_distance(*p, &expected))
            .chain(expected.iter().map(|p| hull_distance(*p, &actual)))
            .fold(0., f64::max);
        assert!(error <= 0.002, "{name}: Q16 hull boundary error {error}");
        let catalog: serde_json::Value =
            serde_json::from_slice(&std::fs::read(out.join("assets.json")).unwrap()).unwrap();
        std::fs::write(
            out.join("submission-commands.bin"),
            memory[0x9880 * 64..0x9880 * 64 + usize::from(command_words)]
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        std::fs::write(
            out.join("submission-push.bin"),
            memory[0x9a00 * 64..0x9a00 * 64 + usize::from(push)]
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let triangles = submitted_shadow_triangles(memory, &catalog, command_words);
        assert_eq!(
            triangles.len(),
            usize::from(stats[2]),
            "published shadow Instances"
        );
        let mut checked_cells = vec![];
        for cell in case["cells"].as_array().unwrap() {
            let x = cell["x"].as_f64().unwrap();
            let z = cell["z"].as_f64().unwrap();
            let expected = cell["polygon"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| [p[0].as_f64().unwrap(), p[1].as_f64().unwrap()])
                .collect::<Vec<_>>();
            let local = triangles
                .iter()
                .filter(|t| {
                    let cx = t.iter().map(|p| p[0]).sum::<f64>() / 3.;
                    let cz = t.iter().map(|p| p[1]).sum::<f64>() / 3.;
                    cx.floor() == x && cz.floor() == z
                })
                .collect::<Vec<_>>();
            let area = local
                .iter()
                .map(|t| polygon_area(t.as_slice()).abs())
                .sum::<f64>();
            let expected_area = polygon_area(&expected).abs();
            assert!(
                (area - expected_area).abs() < 0.002,
                "{name} cell ({x},{z}): submitted area {area} != {expected_area}"
            );
            for t in &local {
                for p in t.iter() {
                    assert!(
                        hull_distance(*p, &expected) < 0.003,
                        "{name}: emitted fan vertex outside reference boundary"
                    );
                }
            }
            for p in &expected {
                let distance = local
                    .iter()
                    .map(|t| hull_distance(*p, t.as_slice()))
                    .fold(f64::INFINITY, f64::min);
                assert!(
                    distance < 0.003,
                    "{name}: missing reference cell boundary {p:?}, distance {distance}"
                );
            }
            checked_cells.push(serde_json::json!({"x":x,"z":z,"reference_area":expected_area,"published_area":area,"triangles":local}));
        }
        assert!(
            triangles.iter().all(|t| {
                let x = (t.iter().map(|p| p[0]).sum::<f64>() / 3.).floor();
                let z = (t.iter().map(|p| p[1]).sum::<f64>() / 3.).floor();
                case["cells"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|cell| cell["x"].as_f64() == Some(x) && cell["z"].as_f64() == Some(z))
            }),
            "{name}: triangle outside all receiving cells"
        );
        std::fs::write(
            out.join("coverage.json"),
            serde_json::to_vec_pretty(&checked_cells).unwrap(),
        )
        .unwrap();
        let effective = case["cells"]
            .as_array()
            .unwrap()
            .iter()
            .map(|cell| {
                let p = cell["polygon"].as_array().unwrap();
                (2..p.len())
                    .filter(|&i| {
                        let ax = p[i - 1][0].as_f64().unwrap() - p[0][0].as_f64().unwrap();
                        let az = p[i - 1][1].as_f64().unwrap() - p[0][1].as_f64().unwrap();
                        let bx = p[i][0].as_f64().unwrap() - p[0][0].as_f64().unwrap();
                        let bz = p[i][1].as_f64().unwrap() - p[0][1].as_f64().unwrap();
                        (ax * bz - az * bx).abs() >= 2. / 65536.
                    })
                    .count()
            })
            .sum::<usize>();
        // Coverage and published area above are the contract; Q16 may choose a different fan.
        assert!(
            triangles.len() <= 352,
            "bounded per-frame triangle capacity"
        );
        let pixels = sim.render_active_framebuffer();
        let mut ppm = b"P6\n400 240\n255\n".to_vec();
        for pixel in pixels {
            ppm.extend([(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8]);
        }
        std::fs::write(out.join("frame-display.ppm"), ppm).unwrap();
        results.push(serde_json::json!({"case":case["name"],"hull_points":stats[0],"receiver_cells":stats[1],"triangles":stats[2],"command_words_u16":command_words,"push_words_u16":push,"reference_effective_triangles_diagnostic":effective,"instruction_steps_including_whole_scene":steps,"max_hull_boundary_error_world_units":error,"pose":"independent Web matrix imported; shadow expression test, not game-animation qualification"}));
    }
    let out =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/bloxorz-web/shadow-sample-v1");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(
        out.join("qualification.json"),
        serde_json::to_vec_pretty(&results).unwrap(),
    )
    .unwrap();
    eprintln!("{results:?}");
}

#[test]
fn shadow_geometry_cache_matches_fresh_instances_across_real_inputs() {
    use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
    struct Frame {
        commands: Vec<u16>,
        push: Vec<u16>,
        colors: Vec<u16>,
        depth: Vec<u16>,
        pixels: Vec<u32>,
        info: Vec<u16>,
    }
    let mut runs = vec![];
    for (name, forced) in [("web-cache-cached", false), ("web-cache-forced", true)] {
        let (program, resources, _) =
            application_case(name, None, None, scenarios::shadow_cache(forced));
        let info = global(&program, "SHADOW_INFO");
        let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
        sim.cpu_mut().set_boot_window(&program.words);
        preload(sim.cpu_mut().physical_memory_mut(), &resources);
        let mut frames = vec![];
        let mut commands = vec![];
        let mut push = vec![];
        let mut push_words = 0usize;
        let mut halted = false;
        for _ in 0..10_000_000 {
            if sim.waiting_for_vblank() {
                sim.advance_vblank();
            }
            match sim.step().unwrap() {
                StepOutcome::Signaled(e) if e.signal_type == 14 => {
                    push_words = usize::from(e.value)
                }
                StepOutcome::Signaled(e) if e.signal_type == 15 => {
                    let m = sim.cpu().physical_memory_slice();
                    commands = m[0x9880 * 64..0x9880 * 64 + usize::from(e.value)].to_vec();
                    push = m[0x9a00 * 64..0x9a00 * 64 + push_words].to_vec();
                }
                StepOutcome::Signaled(e) if e.signal_type == 12 => {
                    let m = sim.cpu().physical_memory_slice();
                    frames.push(Frame {
                        commands: commands.clone(),
                        push: push.clone(),
                        colors: m[0x400000 / 2..0x480000 / 2].to_vec(),
                        depth: m[0x480000 / 2..0x4c0000 / 2].to_vec(),
                        pixels: sim.render_active_framebuffer(),
                        info: m[info..info + 3].to_vec(),
                    });
                }
                StepOutcome::Halted { signal } => {
                    let f = global(&program, "GPU_API_FAULT");
                    assert_eq!(
                        signal,
                        0x600d,
                        "{name}: frame{} fault{:?}",
                        frames.len(),
                        &sim.cpu().physical_memory_slice()[f..f + 6]
                    );
                    halted = true;
                    break;
                }
                _ => {}
            }
        }
        assert!(
            halted,
            "{name}: bounded cache invalidation scene timeout after{}frames",
            frames.len()
        );
        assert!(sim.gpu().last_error().is_none());
        assert_eq!(frames.len(), 9);
        runs.push(frames);
    }
    for (i, (a, b)) in runs[0].iter().zip(&runs[1]).enumerate() {
        assert!(
            a.commands == b.commands,
            "frame{i}: command stream differs from fresh geometry"
        );
        assert!(
            a.push == b.push,
            "frame{i}: native Instances differ from fresh geometry"
        );
        assert!(
            a.colors == b.colors && a.depth == b.depth && a.pixels == b.pixels,
            "frame{i}: full physical surfaces/Display differ"
        );
        assert_eq!(a.info, b.info, "frame{i}: shadow footprint differs");
    }
    assert_ne!(
        runs[0][2].info[2], runs[0][3].info[2],
        "bridge-mask case must change actual receivers"
    );
    assert_ne!(
        runs[0][4].info[2], runs[0][5].info[2],
        "goal hatch changes receivers at an unchanged world pose"
    );
    assert_eq!(
        runs[0][7].info[2], 0,
        "hidden player cannot replay a shadow"
    );
    assert!(
        runs[0][8].info[2] > 0,
        "visible player after hidden state must rebuild/replay correctly"
    );
    let summary=runs[0].iter().enumerate().map(|(i,f)|serde_json::json!({"frame":i,"shadow":f.info,"command_u16":f.commands.len(),"push_u16":f.push.len(),"equal_to_forced_fresh":true})).collect::<Vec<_>>();
    let out = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/bloxorz-web/shadow-cache-inputs.json");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    std::fs::write(out, serde_json::to_vec_pretty(&summary).unwrap()).unwrap();
}
