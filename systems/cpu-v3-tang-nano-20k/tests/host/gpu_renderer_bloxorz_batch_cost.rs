//! Controlled CPU/MC cost, with functional GPU and immediate functional vblank.
use super::{application, global, write_display_ppm};
use cpu_v3::StepOutcome;
use cpu_v3_tang_nano_20k::{
    boot,
    system_emu::CpuV3SystemEmu,
    system_sim::{CpuV3SystemSim, VBlankMode},
};

fn commands(memory: &[u16], base: u16, count: u16) -> serde_json::Value {
    let at = usize::from(base) * 64;
    let mut banks = [0usize; 2];
    let mut draws = 0;
    let mut loads = 0;
    let mut triangles = 0;
    let mut renders = 0;
    let mut ui = 0;
    for i in 0..=usize::from(count) {
        let code = u32::from(memory[at + i * 2]) | (u32::from(memory[at + i * 2 + 1]) << 16);
        match code >> 28 {
            2 => {
                loads += 1;
                let p = ((code >> 12) & 65535) as usize * 64;
                banks[(code & 1) as usize] = usize::from((memory[p] >> 8) & 127);
            }
            3 => {
                draws += 1;
                triangles += banks[(code & 1) as usize];
            }
            6 => renders += 1,
            11 => ui += 1,
            _ => {}
        }
    }
    serde_json::json!({"native_draws":draws,"meshlet_jobs":loads,"triangles":triangles,"set_render":renders,"native_ui":ui,"command_words":usize::from(count)+1})
}

#[test]
#[ignore = "bounded two-arm CPU/MC cost; no GPU cycles or scanout contention"]
fn material_batches_measure_cpu_cost_and_publish_heap_to_functional_gpu() {
    let mut prepared = vec![];
    for mode in ["legacy", "batches"] {
        let (program, resources, out) =
            application(&format!("web-stage1-warm-cpu-cost-{mode}"), None);
        prepared.push((mode, program, resources, out));
    }
    assert_eq!(prepared[0].1.words.len(), prepared[1].1.words.len());
    assert_eq!(
        prepared[0].2, prepared[1].2,
        "counterfactual resources differ"
    );
    let differences = prepared[0]
        .1
        .words
        .iter()
        .zip(&prepared[1].1.words)
        .enumerate()
        .filter_map(|(i, (a, b))| (a != b).then_some((i, *a, *b)))
        .collect::<Vec<_>>();
    let root = std::path::PathBuf::from(std::env::var_os("BLOXORZ_SIM_OUTPUT_ROOT").unwrap());
    std::fs::write(root.join("counterfactual.json"),serde_json::to_vec_pretty(&serde_json::json!({"machine_word_differences":differences,"resources_equal":true,"checked_before_native_run":true})).unwrap()).unwrap();
    assert_eq!(
        differences.len(),
        1,
        "counterfactual must differ only in its mode initialization word"
    );
    let mut results = vec![];
    let mut outputs = vec![];
    for (mode, program, resources, out) in prepared {
        let budget: serde_json::Value =
            serde_json::from_slice(&std::fs::read(out.join("budget.json")).unwrap()).unwrap();
        let code = program
            .words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>();
        let copies = resources
            .iter()
            .map(|(at, bytes)| boot::BootResource::copy(*at, bytes.clone()))
            .collect::<Vec<_>>();
        let package = boot::pack_application_resources(
            &code,
            budget["static_words"].as_u64().unwrap() as usize,
            &copies,
        )
        .unwrap();
        std::fs::write(out.join("cpu-v3-boot.bin"), &package).unwrap();
        let heap_global = global(&program, "GROUND_BASE");
        let mut reference =
            CpuV3SystemSim::from_single_flash(&package, 0, VBlankMode::PauseOnFrameIndexWait)
                .unwrap()
                .with_functional_gpu();
        let mut expected_heap = vec![];
        let mut expected_frames = vec![];
        let mut length = 0u16;
        let mut instruction_halt = false;
        for _ in 0..8_000_000 {
            if reference.waiting_for_vblank() {
                reference.advance_vblank();
            }
            let output = reference
                .step_observing(|w, m| {
                    if w.device == 4 && w.channel == 0 {
                        length = w.value;
                    }
                    if w.device == 4 && w.channel == 1 {
                        let base = usize::from(m[heap_global]);
                        expected_heap.push(if base == 0 {
                            vec![]
                        } else {
                            m[base..base + usize::from(m[base + 6]) * 64].to_vec()
                        });
                    }
                    true
                })
                .unwrap()
                .unwrap();
            if let StepOutcome::Signaled(e) = output {
                if e.signal_type == 12 {
                    let active = reference.display_state().active_base as usize;
                    let m = reference.cpu().physical_memory_slice();
                    expected_frames.push((
                        reference.render_active_framebuffer(),
                        m[active..active + 102400].to_vec(),
                        m[0x480000 / 2..0x480000 / 2 + 102400].to_vec(),
                    ));
                }
            }
            if let StepOutcome::Halted { signal } = output {
                assert_eq!(signal, 0x600d);
                instruction_halt = true;
                break;
            }
        }
        assert!(instruction_halt);
        assert_eq!(expected_frames.len(), 3);
        assert_eq!(expected_heap.len(), 6);
        let mut native =
            CpuV3SystemEmu::from_single_flash(&package, 0, VBlankMode::PauseOnFrameIndexWait)
                .unwrap()
                .with_functional_gpu();
        let wall = std::time::Instant::now();
        let mut last_event = None;
        let mut starts = vec![];
        let mut metrics = vec![];
        let mut frame_starts = vec![];
        let mut frames = vec![];
        let mut generation = vec![];
        let mut min_sp = 0xc000u16;
        let mut halt = false;
        let mut command_count = 0u16;
        for _ in 0..24_000_000 {
            assert!(
                wall.elapsed().as_secs() < 240,
                "bounded host240s CPU-cost timeout"
            );
            if native.waiting_for_vblank() {
                native.advance_vblank();
            }
            let cycle = native.cycles();
            let output = native
                .step_observing(|w, m| {
                    if w.device == 4 && w.channel == 0 {
                        command_count = w.value;
                    }
                    if w.device == 4 && w.channel == 1 {
                        let heap = usize::from(m[heap_global]);
                        let bytes = if heap == 0 {
                            vec![]
                        } else {
                            m[heap..heap + usize::from(m[heap + 6]) * 64].to_vec()
                        };
                        assert_eq!(
                            bytes,
                            expected_heap[starts.len()],
                            "cache publication omitted a heap word at submit{}",
                            starts.len()
                        );
                        if starts.len() < 2 {
                            std::fs::write(
                                out.join(format!("native-gpu-before-{}.bin", starts.len())),
                                m.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<_>>(),
                            )
                            .unwrap();
                        }
                        starts.push(cycle);
                        metrics.push(commands(m, w.value, command_count));
                    }
                    true
                })
                .unwrap()
                .unwrap();
            assert!(!output.fault, "native core fault at{}", native.cycles());
            let core = native.core();
            let sp = core.architectural_gprs()[13];
            if sp != 0 {
                min_sp = min_sp.min(sp);
            }
            let event = core.signal_event();
            drop(core);
            if event != last_event {
                if let Some(e) = event {
                    match e.signal_type {
                        4 => frame_starts.push(native.cycles()),
                        9 | 10 => generation.push((e.signal_type, native.cycles())),
                        12 => {
                            let frame = frames.len();
                            let active = native.display_state().active_base as usize;
                            assert_eq!(
                                native.render_active_framebuffer(),
                                expected_frames[frame].0
                            );
                            let memory = native.physical_memory();
                            assert_eq!(&memory[active..active + 102400], expected_frames[frame].1);
                            assert_eq!(
                                &memory[0x480000 / 2..0x480000 / 2 + 102400],
                                expected_frames[frame].2
                            );
                            frames.push(serde_json::json!({"frame":frame+1,"start_cycle":frame_starts[frame],"completed_cycle":native.cycles(),"cpu_logic_cycles":native.cycles()-frame_starts[frame],"all_physical_color_depth_display_equal_to_instruction":true}));
                        }
                        _ => {}
                    }
                }
                last_event = event;
            }
            if output.halted {
                assert_eq!(output.halt_signal, 0x600d);
                halt = true;
                break;
            }
        }
        assert!(halt, "bounded24M-cycle CPU-cost timeout");
        assert_eq!(frames.len(), 3);
        assert_eq!(starts.len(), 6);
        assert!(native.gpu().last_error().is_none());
        assert!(min_sp > 0x9fff, "stack overlaps configured heap");
        if mode == "batches" {
            assert_eq!(generation.len(), 2);
            assert_eq!((generation[0].0, generation[1].0), (9, 10));
        } else {
            assert!(generation.is_empty());
        }
        write_display_ppm(
            &out.join("frame-display.ppm"),
            &native.render_active_framebuffer(),
        );
        let result = serde_json::json!({"mode":mode,"scope":"stage8 stationary PLAY,3frames, same resources/camera/real timer/no input/formal hook; one mode word at diagnostic entry selects old face path or heap batches","gpu_execution":"functional synchronous callback; no GPU cycles charged","gpu_display_contention":false,"display_wait":"immediate functional vblank, not scanout/FPS","loading":"CB06 installed functionally before clock; runtime cold CPU/cache/native MC includes heap and level setup","cold_start_to_f1_logic_cycles":frames[0]["completed_cycle"],"mesh_generation_markers":generation,"frames":frames,"submissions":metrics,"gpu_start_cycles":starts,"total_logic_cycles":native.cycles(),"native_memory_cycles":native.native_cycles(),"host_wall_seconds_including_observers_and_gpu":wall.elapsed().as_secs_f64(),"minimum_stack_word_address":min_sp,"peak_observed_stack_bytes":(0xc000u32-u32::from(min_sp))*2,"heap_limit_word":0x9fff,"heap_reserved_payload_words":if mode=="batches" {8255}else{0},"heap_geometry_published_exactly_before_all6_gpu_starts":true,"static_words":budget["static_words"],"max_logic_cycles":24000000,"max_host_seconds":240});
        std::fs::write(
            out.join("cpu-cost.json"),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
        outputs.push(expected_frames.pop().unwrap());
        results.push(result);
    }
    let color_changed = outputs[0]
        .1
        .iter()
        .zip(&outputs[1].1)
        .filter(|(a, b)| a != b)
        .count();
    let depth_changed = outputs[0]
        .2
        .iter()
        .zip(&outputs[1].2)
        .filter(|(a, b)| a != b)
        .count();
    let root = std::path::PathBuf::from(std::env::var_os("BLOXORZ_SIM_OUTPUT_ROOT").unwrap());
    let summary = compare_costs(&results, &differences, color_changed, Some(depth_changed));
    std::fs::write(
        root.join("comparison.json"),
        serde_json::to_vec_pretty(&summary).unwrap(),
    )
    .unwrap();
}

fn compare_costs(
    results: &[serde_json::Value],
    differences: &[(usize, u16, u16)],
    color_changed: usize,
    depth_changed: Option<usize>,
) -> serde_json::Value {
    let total = |arm: usize, key: &str| {
        results[arm]["submissions"].as_array().unwrap()[..2]
            .iter()
            .map(|v| v[key].as_u64().unwrap())
            .sum::<u64>()
    };
    assert!(total(1, "meshlet_jobs") < total(0, "meshlet_jobs"));
    assert!(total(1, "native_draws") < total(0, "native_draws"));
    assert_eq!(total(0, "triangles"), total(1, "triangles"));
    for arm in results {
        assert_eq!(
            arm["heap_geometry_published_exactly_before_all6_gpu_starts"],
            true
        );
        for frame in arm["frames"].as_array().unwrap() {
            assert_eq!(
                frame["all_physical_color_depth_display_equal_to_instruction"],
                true
            );
        }
    }
    let warm = |arm: usize| {
        results[arm]["frames"][2]["start_cycle"].as_u64().unwrap()
            - results[arm]["frames"][1]["start_cycle"].as_u64().unwrap()
    };
    let cold = |arm: usize| {
        results[arm]["cold_start_to_f1_logic_cycles"]
            .as_u64()
            .unwrap()
    };
    serde_json::json!({"arms":results,"machine_word_differences":differences,"counterfactual_source_delta":"one nonzero mode initialization value2/1; other source/resources identical","cold_cpu_cycles_saved":cold(0) as i64-cold(1) as i64,"steady_f2_start_to_start_cycles":[warm(0),warm(1)],"steady_cpu_cycles_change":warm(1) as i64-warm(0) as i64,"steady_cpu_change_percent":(warm(1) as f64/warm(0) as f64-1.)*100.,"native_draws_per_frame":[total(0,"native_draws"),total(1,"native_draws")],"meshlet_jobs_per_frame":[total(0,"meshlet_jobs"),total(1,"meshlet_jobs")],"triangles_per_frame":[total(0,"triangles"),total(1,"triangles")],"set_render_per_frame":[total(0,"set_render"),total(1,"set_render")],"command_words_per_frame":[total(0,"command_words"),total(1,"command_words")],"color_pixels_changed_between_paths":color_changed,"depth_halfwords_changed_between_paths":depth_changed,"positions_normals_uv_preserved":"independent all33 RCC builder qualification","steady_cpu_reduction_required":false,"gpu_speed_claim":false})
}

#[test]
#[ignore = "audit an explicit prior completed CPU-cost capture without rerunning emulation"]
fn saved_native_costs_match_final_code_and_installed_memory() {
    let saved = std::path::PathBuf::from(
        std::env::var_os("BLOXORZ_SAVED_CPU_COST_ROOT").expect("explicit saved CPU-cost directory"),
    );
    let mut reports = vec![];
    let mut programs = vec![];
    let mut pixels = vec![];
    let mut bindings = vec![];
    for mode in ["legacy", "batches"] {
        let (program, resources, out) =
            application(&format!("web-stage1-warm-cpu-cost-{mode}"), None);
        let budget: serde_json::Value =
            serde_json::from_slice(&std::fs::read(out.join("budget.json")).unwrap()).unwrap();
        let code = program
            .words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>();
        let copies = resources
            .iter()
            .map(|(p, b)| boot::BootResource::copy(*p, b.clone()))
            .collect::<Vec<_>>();
        let corrected = boot::pack_application_resources(
            &code,
            budget["static_words"].as_u64().unwrap() as usize,
            &copies,
        )
        .unwrap();
        let old = saved.join(format!("web-stage1-warm-cpu-cost-{mode}"));
        let captured = std::fs::read(old.join("cpu-v3-boot.bin")).unwrap();
        let mut original_memory = vec![0u16; 1 << 22];
        let mut final_memory = vec![0u16; 1 << 22];
        boot::install_single_application(&captured, &mut original_memory).unwrap();
        boot::install_single_application(&corrected, &mut final_memory).unwrap();
        assert_eq!(
            original_memory, final_memory,
            "saved execution has different runtime bytes from final program/resources"
        );
        std::fs::write(out.join("cpu-v3-boot.bin"), &corrected).unwrap();
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(old.join("cpu-cost.json")).unwrap()).unwrap();
        bindings.push(serde_json::json!({"mode":mode,"original_reported_frontend_static_words":report["static_words"],"final_frontend_static_words":budget["static_words"],"all8MiB_installed_runtime_words_equal":true,"new_emulation_cycles":0,"initial_memory_seed":0}));
        reports.push(report);
        programs.push(program.words);
        let ppm = std::fs::read(old.join("frame-display.ppm")).unwrap();
        let header = b"P6\n400 240\n255\n";
        assert!(ppm.starts_with(header));
        pixels.push(ppm[header.len()..].to_vec());
    }
    let differences = programs[0]
        .iter()
        .zip(&programs[1])
        .enumerate()
        .filter_map(|(i, (a, b))| (a != b).then_some((i, *a, *b)))
        .collect::<Vec<_>>();
    assert_eq!(programs[0].len(), programs[1].len());
    assert_eq!(differences.len(), 1);
    let color = pixels[0]
        .as_chunks::<3>()
        .0
        .iter()
        .zip(pixels[1].as_chunks::<3>().0.iter())
        .filter(|(a, b)| a != b)
        .count();
    let summary = compare_costs(&reports, &differences, color, None);
    let root = std::path::PathBuf::from(std::env::var_os("BLOXORZ_SIM_OUTPUT_ROOT").unwrap());
    std::fs::write(
        root.join("comparison.json"),
        serde_json::to_vec_pretty(&summary).unwrap(),
    )
    .unwrap();
    std::fs::write(root.join("saved-runtime-audit.json"),serde_json::to_vec_pretty(&serde_json::json!({"bindings":bindings,"native_emulation_rerun":false,"original_fail_retained":"unrequested steady-CPU-improvement assertion failed after both actual arms and all functional/stack checks completed","contract":"measure signed CPU changes and assert reduced submitted jobs/same triangles","static_extent":"same frontend static extent as the formal boot producer; no compiler or boot metadata change"})).unwrap()).unwrap();
}
