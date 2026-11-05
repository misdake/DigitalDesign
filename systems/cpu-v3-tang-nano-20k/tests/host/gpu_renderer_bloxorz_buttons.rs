//! Incremental button snapshots across animation, release and stage rebuilds.
use super::*;
use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};

#[test]
fn incremental_button_parts_match_fresh_color_depth_and_display() {
    let mut outputs = vec![];
    for mode in ["cached", "fresh"] {
        let (program, resources, out) =
            application(&format!("button-parts-{mode}"), Some("run(16,false,128);"));
        let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
        sim.cpu_mut().set_boot_window(&program.words);
        preload(sim.cpu_mut().physical_memory_mut(), &resources);
        let mut frames = vec![];
        let mut rebuilt = vec![];
        let mut by_frame = vec![];
        let mut halted = false;
        for _ in 0..35_000_000 {
            if sim.waiting_for_vblank() {
                sim.advance_vblank();
            }
            match sim.step().unwrap() {
                StepOutcome::Signaled(e) if e.signal_type == 14 => rebuilt.push(e.value),
                StepOutcome::Signaled(e) if e.signal_type == 12 => {
                    let n = frames.len();
                    let memory = sim.cpu().physical_memory_slice();
                    let base = sim.display_state().active_base as usize;
                    let pixels = sim.render_active_framebuffer();
                    write_display_ppm(&out.join(format!("frame-{n:02}-display.ppm")), &pixels);
                    frames.push((
                        memory[base..base + 102400].to_vec(),
                        memory[0x480000 / 2..0x480000 / 2 + 102400].to_vec(),
                        pixels,
                    ));
                    by_frame.push(std::mem::take(&mut rebuilt));
                }
                StepOutcome::Halted { signal } => {
                    assert_eq!(signal, 0x600d);
                    halted = true;
                    break;
                }
                _ => {}
            }
        }
        assert!(halted, "bounded button lifecycle");
        assert!(sim.gpu().last_error().is_none());
        assert_eq!(frames.len(), 16);
        if mode == "cached" {
            assert_eq!(by_frame[0].len(), 6);
            assert_eq!(by_frame[9].len(), 13);
            assert_eq!(by_frame[14].len(), 13);
            for n in [1, 4, 6, 8, 10, 13, 15] {
                assert!(by_frame[n].is_empty(), "unchanged frame {n}");
            }
            assert!(!by_frame[2].is_empty() && by_frame[2].len() < 6);
            for n in [3, 5, 7] {
                assert_eq!(by_frame[n], by_frame[2], "only changed channel {n}");
            }
            assert!(!by_frame[11].is_empty() && by_frame[11].len() < 13);
            assert_eq!(by_frame[11], by_frame[12]);
        } else {
            for (n, parts) in by_frame.iter().enumerate() {
                assert_eq!(parts.len(), if n < 9 { 6 } else { 13 });
            }
        }
        std::fs::write(
            out.join("button-lifecycle.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "rebuilt_parts_by_frame":by_frame,"frames":16,"max_steps":35000000,
                "fixture":"controlled session state and dt; not physical input or cycle timing"
            }))
            .unwrap(),
        )
        .unwrap();
        outputs.push(frames);
    }
    assert_eq!(
        outputs[0], outputs[1],
        "cached/fresh physical color/depth and Display"
    );
}
