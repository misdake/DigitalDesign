use super::scenarios;
use super::*;

#[test]
fn render_us_uses_last_gpu_wait_excludes_present_and_recovers_after_saturation() {
    use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
    use std::time::Duration;
    for fade in [false, true] {
        let name = if fade {
            "web-stage1-warm-render-time-fade"
        } else {
            "web-stage1-warm-render-time-normal"
        };
        let (program, resources, out) =
            application_case(name, None, None, scenarios::render_time(fade));
        let timing = global(&program, "DRAW_STATS.render_microseconds");
        let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
        sim.cpu_mut().set_boot_window(&program.words);
        preload(sim.cpu_mut().physical_memory_mut(), &resources);
        let mut starts = 0usize;
        let mut frames = vec![];
        let mut halted = false;
        for _ in 0..8_000_000 {
            if sim.waiting_for_vblank() {
                sim.advance_host_time(Duration::from_millis(50));
                sim.advance_vblank();
            }
            let mut started = false;
            let outcome = sim
                .step_observing(|w, _| {
                    if w.device == 4 && w.channel == 1 {
                        started = true;
                    }
                    true
                })
                .unwrap()
                .unwrap();
            if started {
                let elapsed = if frames.len() == 2 && starts == 0 {
                    66_000
                } else {
                    [3_754u64, 5_241, 7_009, 11_023][starts]
                };
                sim.advance_host_time(Duration::from_micros(elapsed));
                starts += 1;
            }
            if let StepOutcome::Signaled(e) = outcome {
                if e.signal_type == 12 {
                    let expected_starts = if fade {
                        if frames.is_empty() {
                            4
                        } else {
                            1
                        }
                    } else if frames.is_empty() {
                        3
                    } else {
                        2
                    };
                    assert_eq!(starts, expected_starts);
                    let (expected, formatted) = if fade {
                        (3_754, &[255, b'3', b'.', b'7', b'5'])
                    } else {
                        (8_995, &[255, b'9', b'.', b'0', b'0'])
                    };
                    let m = sim.cpu().physical_memory_slice();
                    let row = actual_hud_cells(m, 2);
                    if frames.is_empty() {
                        assert_eq!(m[timing + 1], 0);
                    } else if frames.len() == 2 {
                        assert_eq!(&m[timing..timing + 2], &[65535, 0]);
                    } else {
                        assert_eq!(&m[timing..timing + 2], &[expected, 1]);
                    }
                    if frames.len() == 2 {
                        assert_eq!(&row[3..8], formatted);
                    } else {
                        assert_eq!(&row[3..8], b"-----");
                    }
                    frames.push(serde_json::json!({"frame":frames.len()+1,"gpu_submissions":starts,"sample_us_and_valid":m[timing..timing+2],"hud_ms_cells":row[3..8],"present_wait_ms_excluded":50}));
                    starts = 0;
                }
            }
            if let StepOutcome::Halted { signal } = outcome {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
        }
        assert!(halted);
        assert_eq!(frames.len(), 4);
        assert!(sim.gpu().last_error().is_none());
        std::fs::write(out.join("timing-qualification.json"),serde_json::to_vec_pretty(&serde_json::json!({"fade":fade,"max_steps":8000000,"clock":"real simulator timer advanced at actual GPU starts and vblank boundaries","frames":frames,"initial_interval_unknown":true,"saturation_recovery_tested":true,"hardware_frame_time_claim":false})).unwrap()).unwrap();
    }
}
