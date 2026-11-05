//! Opt-in bounded texture qualification using ordinary CPU scene submissions.
use super::{application_from_sources, preload, write_display_ppm};
use cpu_v3::StepOutcome;
use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
use std::path::PathBuf;

#[test]
#[ignore = "bounded atlas source/LOD experiment; select one explicit before/candidate arm"]
fn atlas_fixed_scenes_and_real_submission_inputs() {
    let arm = std::env::var("BLOXORZ_ATLAS_ARM").expect("explicit arm");
    assert!(matches!(arm.as_str(), "before" | "candidate"));
    let root = PathBuf::from(std::env::var_os("BLOXORZ_SIM_OUTPUT_ROOT").expect("explicit output"));
    let frozen = root.join("source-8235/systems/cpu-v3-tang-nano-20k");
    let before = arm == "before";
    let identity_path = root.join("control-identity.json");
    let control_identity: serde_json::Value = if identity_path.is_file() {
        serde_json::from_slice(&std::fs::read(identity_path).unwrap()).unwrap()
    } else {
        serde_json::json!({"application_source":"8235b96fd799e0234d97dd1b653a930cc44d56b5","driver_source":"8235b96fd799e0234d97dd1b653a930cc44d56b5"})
    };
    if before {
        assert!(frozen
            .join("examples/gpu_renderer_bloxorz/rcc/main.rs")
            .is_file());
    }
    let sizes = std::env::var("BLOXORZ_ATLAS_SIZES").unwrap_or_else(|_| "6x7".into());
    assert!(matches!(sizes.as_str(), "6x7" | "7x7" | "6x8"));
    let name = if before {
        "fragile-accepted-atlas-scenes".into()
    } else {
        format!("fragile-atlas-candidate-{sizes}-scenes")
    };
    let (program, resources, out) = application_from_sources(
        &name,
        Some("run(7,false,128);"),
        before.then_some(frozen.as_path()),
    );
    std::fs::write(
        out.join("program.bin"),
        program
            .words
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
    sim.cpu_mut().set_boot_window(&program.words);
    preload(sim.cpu_mut().physical_memory_mut(), &resources);
    let mut frame = 0;
    let mut count = 0;
    let mut inputs = vec![];
    let mut frames = vec![];
    let mut halted = false;
    let mut steps = 0;
    let mut minimum_sp = 0xc000;
    for _ in 0..48_000_000 {
        if sim.waiting_for_vblank() {
            sim.advance_vblank();
        }
        steps += 1;
        let outcome = sim.step_observing(|write, memory| {
            if write.device == 4 && write.channel == 0 { count = write.value; }
            if write.device == 4 && write.channel == 1 && frame > 0 {
                let file = format!("gpu-before-{}.bin", inputs.len());
                std::fs::write(out.join(&file), memory.iter().flat_map(|v|v.to_le_bytes()).collect::<Vec<_>>()).unwrap();
                inputs.push(serde_json::json!({"frame":frame,"memory":file,"base_bytes":u32::from(write.value)*128,"count_minus_one":count,"maximum_work":10000000}));
            }
            true
        }).unwrap().unwrap();
        let sp = sim.cpu().registers()[13];
        if sp != 0 {
            minimum_sp = minimum_sp.min(sp);
        }
        match outcome {
            StepOutcome::Signaled(e) if e.signal_type == 12 => {
                let rgb = sim.render_active_framebuffer();
                assert_eq!(rgb.len(), 400 * 240);
                write_display_ppm(&out.join(format!("frame-{frame}.ppm")), &rgb);
                let memory = sim.cpu().physical_memory_slice();
                for (label, at) in [
                    ("color", sim.display_state().active_base as usize),
                    ("depth", 0x480000 / 2),
                ] {
                    std::fs::write(
                        out.join(format!("frame-{frame}-{label}.bin")),
                        memory[at..at + 102400]
                            .iter()
                            .flat_map(|v| v.to_le_bytes())
                            .collect::<Vec<_>>(),
                    )
                    .unwrap();
                }
                frames.push(serde_json::json!({"frame":frame,"level":([4,4,30,15,17,8,4][frame]),"pose":if frame==6{"actual roll 110ms"}else{"stationary PLAY"}}));
                frame += 1;
            }
            StepOutcome::Halted { signal } => {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
            _ => {}
        }
    }
    assert!(halted, "bounded48M atlas {arm}");
    assert_eq!(frame, 7);
    assert!(sim.gpu().last_error().is_none());
    assert!(
        minimum_sp >= 40959,
        "observed stack must stay above the configured heap"
    );
    let baseline = root.join("fragile-accepted-atlas-scenes");
    if !before {
        for i in 0..7 {
            assert_eq!(
                std::fs::read(out.join(format!("frame-{i}-depth.bin"))).unwrap(),
                std::fs::read(baseline.join(format!("frame-{i}-depth.bin"))).unwrap(),
                "texture-only edit changed full depth frame{i}"
            );
        }
    }
    std::fs::write(out.join("qualification.json"),serde_json::to_vec_pretty(&serde_json::json!({"mode":"CPU instruction + functional GPU; no hardware timing","arm":arm,"before_source":"8235b96fd799e0234d97dd1b653a930cc44d56b5","control_identity":control_identity,"frames":frames,"gpu_inputs":inputs,"instruction_steps":steps,"max_steps":48000000,"minimum_SP_word":minimum_sp,"heap_end_exclusive_word":40959,"full_depth_equals_before":!before,"gpu_error":null})).unwrap()).unwrap();
    assert!(out.join("qualification.json").is_file());
}

#[test]
#[ignore = "replay two existing warm submissions to compare base-only filter encodings"]
fn base_only_bilinear_matches_zero_lod_trilinear() {
    use gpu_v2::system::{functional::Executor, vliw_command::DeviceSubmission};
    use std::collections::BTreeMap;
    let root = PathBuf::from(std::env::var_os("BLOXORZ_SIM_OUTPUT_ROOT").expect("explicit inputs"));
    let folder = root.join("fragile-atlas-candidate-6x7-scenes");
    let q: serde_json::Value =
        serde_json::from_slice(&std::fs::read(folder.join("qualification.json")).unwrap()).unwrap();
    let mut changed = 0;
    for input in q["gpu_inputs"].as_array().unwrap().iter().take(2) {
        let bytes = std::fs::read(folder.join(input["memory"].as_str().unwrap())).unwrap();
        let mut before: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|w| u16::from_le_bytes(*w))
            .collect();
        let mut alternative = before.clone();
        let at = input["base_bytes"].as_u64().unwrap() as usize / 2;
        let count = input["count_minus_one"].as_u64().unwrap() as usize + 1;
        let mut slots = BTreeMap::new();
        let mut patches = BTreeMap::new();
        for pair in before[at..at + count * 2].as_chunks::<2>().0 {
            let command = u32::from(pair[0]) | (u32::from(pair[1]) << 16);
            if command >> 28 == 1 {
                slots.insert(
                    (command >> 3) & 127,
                    ((command >> 10) & 0x3ffff) as usize * 16,
                );
            }
            if command >> 28 == 6 {
                let source = slots[&(command & 127)];
                let high = before[source + 1];
                if (high >> 5) & 3 == 1 && high & 16 == 0 {
                    patches.insert(source + 1, high);
                    alternative[source + 1] = (high & !96) | 64;
                }
            }
        }
        assert!(!patches.is_empty());
        changed += patches.len();
        let descriptor = DeviceSubmission::new((at * 2) as u32, (count * 4) as u32).unwrap();
        Executor::new(10_000_000)
            .execute(&mut before, descriptor)
            .unwrap();
        Executor::new(10_000_000)
            .execute(&mut alternative, descriptor)
            .unwrap();
        for (at, value) in patches {
            alternative[at] = value;
        }
        assert_eq!(
            before, alternative,
            "filter1 vs2 must produce identical physical writes at base-only LOD0"
        );
    }
    std::fs::write(root.join("bilinear-equivalence.json"),serde_json::to_vec_pretty(&serde_json::json!({"actual_submissions":2,"patched_base_only_descriptors":changed,"filter_codes":[1,2],"all_physical_outputs_exact_after_restoring_only_explicit_descriptor_input_delta":true,"maximum_work_per_submission":10000000})).unwrap()).unwrap();
}

#[test]
#[ignore = "reuse existing fixed-depth scene programs for two bounded native CPU frames"]
fn fixed_depth_texture_cold_and_warm_native_cpu_cost() {
    use cpu_v3_tang_nano_20k::{boot, system_emu::CpuV3SystemEmu};
    let root = PathBuf::from(
        std::env::var_os("BLOXORZ_SIM_OUTPUT_ROOT").expect("explicit existing programs"),
    );
    let mut rows = vec![];
    for folder in [
        "fragile-accepted-atlas-scenes",
        "fragile-atlas-candidate-6x7-scenes",
    ] {
        let path = root.join(folder);
        let code = std::fs::read(path.join("program.bin")).unwrap();
        let budget: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path.join("budget.json")).unwrap()).unwrap();
        let catalog: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path.join("assets.json")).unwrap()).unwrap();
        let resources: Vec<_> = catalog["resources"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                boot::BootResource::copy(
                    r["address"].as_u64().unwrap() as usize,
                    std::fs::read(path.join(r["file"].as_str().unwrap())).unwrap(),
                )
            })
            .collect();
        let package = boot::pack_application_resources(
            &code,
            budget["static_words"].as_u64().unwrap() as usize,
            &resources,
        )
        .unwrap();
        std::fs::write(path.join("native-test-package.bin"), &package).unwrap();
        let mut native =
            CpuV3SystemEmu::from_single_flash(&package, 0, VBlankMode::PauseOnFrameIndexWait)
                .unwrap()
                .with_functional_gpu();
        let mut last = None;
        let mut begin = 0;
        let mut first = None;
        let mut cycles = vec![];
        let mut minimum_sp = 0xc000;
        let wall = std::time::Instant::now();
        for _ in 0..16_000_000 {
            assert!(
                wall.elapsed().as_secs() < 180,
                "native two-frame wall bound"
            );
            if native.waiting_for_vblank() {
                native.advance_vblank();
            }
            let step = native.step().unwrap();
            assert!(!step.fault);
            let core = native.core();
            let sp = core.architectural_gprs()[13];
            if sp != 0 {
                minimum_sp = minimum_sp.min(sp);
            }
            let event = core.signal_event();
            drop(core);
            if event != last {
                if let Some(e) = event {
                    if e.signal_type == 4 {
                        begin = native.cycles();
                        first.get_or_insert(begin);
                    }
                    if e.signal_type == 12 {
                        let frame = cycles.len();
                        let memory = native.physical_memory();
                        for (label, at) in [
                            ("color", native.display_state().active_base as usize),
                            ("depth", 0x480000 / 2),
                        ] {
                            let reference =
                                std::fs::read(path.join(format!("frame-{frame}-{label}.bin")))
                                    .unwrap();
                            let expected: Vec<u16> = reference
                                .as_chunks::<2>()
                                .0
                                .iter()
                                .map(|v| u16::from_le_bytes(*v))
                                .collect();
                            assert_eq!(
                                &memory[at..at + 102400],
                                expected,
                                "native full {label} {folder}/{frame}"
                            );
                        }
                        cycles.push(native.cycles() - begin);
                        if cycles.len() == 2 {
                            break;
                        }
                    }
                }
                last = event;
            }
        }
        assert_eq!(cycles.len(), 2);
        assert!(minimum_sp >= 40959);
        rows.push(serde_json::json!({"program":folder,"cold_setup_cycles":first,"frame_cycles":cycles,"minimum_SP_word":minimum_sp,"native_full_color_depth_equal_instruction":true,"program_execution_stopped_after_two_presented_frames":true}));
    }
    std::fs::write(root.join("native-two-frame-cost.json"),serde_json::to_vec_pretty(&serde_json::json!({"mode":"native CPU/cycle emu + synchronous functional GPU; no physical GPU/Display contention timing or FPS","same_fixed_depth_API_control":true,"scope":"existing seven-frame test programs stopped after frames0/1; synthetic postboot preload, finite lifetime checked separately","max_native_steps_per_arm":16000000,"host_seconds_per_arm":180,"arms":rows})).unwrap()).unwrap();
}
