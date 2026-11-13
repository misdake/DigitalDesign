//! Bounded full-system timing with the same fixed PLAY fixture on both arms.
use super::scenarios;
use super::*;
use cpu_v3_tang_nano_20k::{
    boot,
    system_cycle_emu::{runner, System},
};
use serde_json::json;
use std::time::{Duration, Instant};

fn words(path: &Path, data: &[u16]) {
    std::fs::write(
        path,
        data.iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
}

#[test]
fn prepared_bridge_capture_reads_fixed_push_after_reset_and_normal_retirement() {
    use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
    let (program, resources, out) = application_case(
        "static-cache-prepared-capture",
        Some("run(3,false,128);"),
        None,
        scenarios::static_cache(scenarios::cache_level(), true),
    );
    let records = global(&program, "GPU_CB_RECORDS");
    let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
    sim.cpu_mut().set_boot_window(&program.words);
    preload(sim.cpu_mut().physical_memory_mut(), &resources);
    let mut captures = vec![];
    let mut expected = vec![];
    let mut samples = 0;
    let mut halted = false;
    let mut minimum_sp = 0xc000;
    for _ in 0..12_000_000 {
        if sim.waiting_for_vblank() {
            sim.advance_vblank();
        }
        let sp = sim.cpu().registers()[13];
        if sp != 0 {
            minimum_sp = minimum_sp.min(sp);
        }
        let outcome = sim.step().unwrap();
        if let StepOutcome::Signaled(e) = outcome {
            let m = sim.cpu().physical_memory_slice();
            match e.signal_type {
                5 => {
                    assert!(!sim.gpu().busy());
                    let header = &m[usize::from(e.value)..usize::from(e.value) + 64];
                    assert_eq!(header[9], 0);
                    assert_eq!(&header[12..14], &[0x4000, 0x4c]);
                    let c = &m[records..records + 224];
                    assert_eq!(c[209] & 1, 1);
                    assert!(c[209] & !1 != 0, "button parts were recorded");
                    assert!(header[20] >= c[211] * 2);
                    if captures.len() == 1 {
                        words(
                            &out.join("warm-command.bin"),
                            &m[0x9880 * 64..0x9880 * 64 + usize::from(c[5])],
                        );
                        words(
                            &out.join("warm-push.bin"),
                            &m[0x9a00 * 64..0x9a00 * 64 + usize::from(c[6])],
                        );
                        words(&out.join("warm-template.bin"), &m[0x9800 * 64..0x9880 * 64]);
                        words(&out.join("warm-record.bin"), c);
                        words(
                            &out.join("warm-facts.bin"),
                            &m[global(&program, "GPU_CB_CAPTURE_FACTS")
                                ..global(&program, "GPU_CB_CAPTURE_FACTS")
                                    + usize::from(c[15]) * 4],
                        );
                        words(&out.join("warm-physical.bin"), m);
                    }
                    captures.push(json!({"header":header,"persistent_push_words":c[211],"group_words":c[210],"fact_count":c[15]}));
                }
                6 => {
                    let at = usize::from(e.value) * 64;
                    expected = m[at..at + 16]
                        .iter()
                        .chain(m[at + 64..at + 88].iter())
                        .copied()
                        .collect();
                    samples = 0;
                }
                7 => {
                    assert_eq!(e.value, expected[samples]);
                    samples += 1;
                }
                _ => {}
            }
        }
        if let StepOutcome::Halted { signal } = outcome {
            assert_eq!(
                signal,
                0x600d,
                "GPU API fault {:?}",
                &sim.cpu().physical_memory_slice()
                    [global(&program, "GPU_API_FAULT")..global(&program, "GPU_API_FAULT") + 6]
            );
            halted = true;
            break;
        }
    }
    assert!(halted, "bounded prepared capture timeout");
    assert_eq!(captures.len(), 3);
    assert_eq!(samples, 40);
    assert!(minimum_sp >= 0xa000);
    assert!(sim.gpu().last_error().is_none());
    std::fs::write(out.join("capture-qualification.json"),serde_json::to_vec_pretty(&json!({"captures":captures,"minimum_sp":minimum_sp,"max_steps":12000000,"scope":"real A publication and C indirect dependency capture; fixed push samples read through capture exports before normal START"})).unwrap()).unwrap();
}

#[test]
#[ignore = "explicit bounded full-system Bloxorz timing and physical outputs"]
fn warm_second_frame_system_timing() {
    runner::host_worker(|| {
        measure();
        Ok(())
    })
    .unwrap();
}

fn measure() {
    let arm = std::env::var("BLOXORZ_CACHE_ARM").expect("explicit arm");
    let (image, out, listing) = if let Some(saved) = std::env::var_os("BLOXORZ_CACHE_IMAGE") {
        let saved = PathBuf::from(saved);
        let out = PathBuf::from(std::env::var_os("BLOXORZ_SIM_OUTPUT_ROOT").unwrap())
            .join(format!("static-cache-{arm}"));
        std::fs::create_dir_all(&out).unwrap();
        let listing =
            std::fs::read_to_string(saved.parent().unwrap().join("application.listing")).unwrap();
        let image = std::fs::read(saved).unwrap();
        std::fs::write(out.join("cpu-v3-boot.bin"), &image).unwrap();
        std::fs::write(out.join("application.listing"), &listing).unwrap();
        (image, out, listing)
    } else {
        let (program, resources, out) = application_case(
            &format!("static-cache-{arm}"),
            Some("run(3,false,128);"),
            None,
            scenarios::static_cache(scenarios::cache_level(), false),
        );
        std::fs::write(out.join("application.listing"), &program.listing).unwrap();
        let code = program
            .words
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>();
        let budget: serde_json::Value =
            serde_json::from_slice(&std::fs::read(out.join("budget.json")).unwrap()).unwrap();
        let copies = resources
            .iter()
            .map(|(a, b)| boot::BootResource::copy(*a, b.clone()))
            .collect::<Vec<_>>();
        let image = boot::pack_application_resources(
            &code,
            budget["static_words"].as_u64().unwrap() as usize,
            &copies,
        )
        .unwrap();
        std::fs::write(out.join("cpu-v3-boot.bin"), &image).unwrap();
        (image, out, program.listing)
    };
    // The composition does not expose model-only SIGNAL notifications. Resolve
    // its existing marker in the exact image and observe first Execute arrival.
    let sites = listing
        .lines()
        .filter(|l| l.contains("signal ") && l.trim_end().ends_with(", 4"))
        .map(|l| u16::from_str_radix(l.trim().split(':').next().unwrap(), 16).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(sites.len(), 1, "one application frame marker");
    let mut arrival = runner::ExecuteArrival::default();
    let mut system = System::from_preloaded_cold(&image, 0xa55a).unwrap();
    let quantum_hz = system.clock().quantum_hz;
    let started = Instant::now();
    let max_logic = 12_000_000;
    let max_host = 1800;
    let mut events = vec![];
    let mut frame = 0;
    let mut busy = false;
    let mut submission = 0;
    let mut complete = false;
    // This fixed warm fixture has restore then dynamic. Attribute by accepted
    // START order, retaining actual addresses, rather than a hard-coded base.
    let mut frame_submissions = 0;
    let mut written = 0;
    while system.clock().logic_edges < max_logic
        && started.elapsed() < Duration::from_secs(max_host)
    {
        let step=system.step_observing(|o|{
            if arrival.observe(o)==Some((7,sites[0])) {
                frame+=1; frame_submissions=0;
                events.push(json!({"kind":"frame_start","tick":o.instant.ticks,"frame":frame,"execute_pc":[7,sites[0]]}));
            }
            if let Some(gpu)=o.gpu {
                if busy && !gpu.control.processor.busy {
                    events.push(json!({"kind":"gpu_done","tick":o.instant.ticks,"frame":frame,"submission":submission}));
                    complete=frame==2&&frame_submissions==2;
                }
                busy=gpu.control.processor.busy;
                if let Some(bus)=o.bus {
                    if bus.write&&bus.index==4&&bus.channel==1&&gpu.control.processor.submit_ready {
                        submission+=1;frame_submissions+=1;
                        events.push(json!({"kind":"cpu_submit","tick":o.instant.ticks,"frame":frame,"submission":submission,"base_sector":bus.data}));
                    }
                }
            }
        }).unwrap();
        if events.len() != written {
            std::fs::write(
                out.join("events-progress.json"),
                serde_json::to_vec_pretty(&events).unwrap(),
            )
            .unwrap();
            written = events.len();
        }
        if let Some(cpu) = step.cpu {
            assert!(
                !cpu.fault,
                "CPU fault {} at {}",
                cpu.fault_code, cpu.fault_pc
            );
            if cpu.halted {
                assert_eq!(cpu.halt_signal, 0x600d);
                break;
            }
        }
        if complete {
            // F2 Back A is fully drained at normal GPU completion. Stop before
            // the subsequent present; it is excluded from these five points.
            words(
                &out.join("frame2-color.bin"),
                &system.backing()[0x400000 / 2..0x400000 / 2 + 102400],
            );
            words(
                &out.join("frame2-depth.bin"),
                &system.backing()[0x480000 / 2..0x480000 / 2 + 102400],
            );
            break;
        }
        if system.clock().logic_edges.is_multiple_of(1_000_000) && step.instant.logic_rise {
            eprintln!(
                "{arm}: {} logic, frame {frame}, submits {submission}, {:.1}s",
                system.clock().logic_edges,
                started.elapsed().as_secs_f64()
            );
        }
    }
    let f2 = events
        .iter()
        .filter(|e| e["frame"] == 2)
        .cloned()
        .collect::<Vec<_>>();
    let report = json!({"arm":arm,"level":29,"entry":"synthetic cold preload; production cycle composition; test-only fixed level29 PLAY, dt=0, neutral input, HUD off","frame_start":"first unheld Execute at the linked existing signal4 marker, after shadow frame reset","quantum_hz":quantum_hz,"logic_hz":54000000,"logic_edges":system.clock().logic_edges,"host_seconds":started.elapsed().as_secs_f64(),"max_logic":max_logic,"max_host_seconds":max_host,"complete":complete,"events":events,"frame2":f2,"submissions":submission});
    std::fs::write(
        out.join("timing.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    assert!(
        complete,
        "full-system frame bound; report saved at {}",
        out.display()
    );
    assert_eq!(f2.iter().filter(|e| e["kind"] == "cpu_submit").count(), 2);
    assert_eq!(f2.iter().filter(|e| e["kind"] == "gpu_done").count(), 2);
    eprintln!("timing: {}", out.join("timing.json").display());
}
pub(super) fn retained_static_scene_regression() {
    use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
    let mut outputs = vec![];
    for fresh in [false, true] {
        let name = if fresh {
            "web-stage1-native-warm-rerecord"
        } else {
            "web-stage1-native-warm"
        };
        let (program, resources, out) = application_case(
            name,
            None,
            None,
            if fresh {
                scenarios::rerecord()
            } else {
                scenarios::stage(0, false, false, true)
            },
        );
        let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
        sim.cpu_mut().set_boot_window(&program.words);
        super::preload(sim.cpu_mut().physical_memory_mut(), &resources);
        let hud = super::global(&program, "HUD");
        let mut counts = vec![];
        let mut frames = vec![];
        let mut starts = 0;
        let mut count = 0;
        let mut draws = 0;
        let mut original_cache = None;
        let mut halted = false;
        for _ in 0..8_000_000 {
            if sim.waiting_for_vblank() {
                sim.advance_vblank();
            }
            let outcome = sim
                .step_observing(|w, m| {
                    if w.device == 4 && w.channel == 0 {
                        count = w.value;
                    }
                    if w.device == 4 && w.channel == 1 {
                        starts += 1;
                        let at = usize::from(w.value) * 64;
                        draws += m[at..at + (usize::from(count) + 1) * 2]
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .filter(|p| [3, 11].contains(&(p[1] >> 12)))
                            .count();
                    }
                    true
                })
                .unwrap()
                .unwrap();
            if let StepOutcome::Signaled(e) = outcome {
                if e.signal_type == 12 {
                    let memory = sim.cpu().physical_memory_slice();
                    assert_eq!(usize::from(memory[hud + 1]), draws);
                    let color = sim.display_state().active_base as usize;
                    frames.push((
                        memory[color..color + 102400].to_vec(),
                        memory[0x480000 / 2..0x480000 / 2 + 102400].to_vec(),
                        sim.render_active_framebuffer(),
                    ));
                    let cache = (
                        memory[0x500000 / 2..0x500000 / 2 + 131072].to_vec(),
                        memory[0x540000 / 2..0x540000 / 2 + 131072].to_vec(),
                    );
                    if let Some(old) = &original_cache {
                        assert_eq!(
                            old, &cache,
                            "dynamic frame must leave static planes and their tails unchanged"
                        );
                    } else {
                        original_cache = Some(cache);
                    }
                    counts.push(starts);
                    starts = 0;
                    draws = 0;
                }
            }
            if let StepOutcome::Halted { signal } = outcome {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
        }
        assert!(halted, "bounded cached/fresh application");
        assert!(sim.gpu().last_error().is_none());
        assert_eq!(frames.len(), 3);
        assert_eq!(counts, if fresh { vec![3, 3, 3] } else { vec![3, 2, 2] });
        std::fs::write(out.join("static-scene.json"),serde_json::to_vec_pretty(&json!({"submissions_by_frame":counts,"static_planes_immutable":true,"max_steps":8000000,"mode":"instruction CPU plus functional GPU; no cycle timing claim"})).unwrap()).unwrap();
        outputs.push(frames);
    }
    assert_eq!(
        outputs[0], outputs[1],
        "warm restore must match full static redraw color/depth/Display"
    );
}
