use super::scenarios;
use super::*;

#[test]
fn cached_veil_captures_each_boundary_once_and_discards_locked_input() {
    use cpu_v3_tang_nano_20k::{
        input_device::{ControllerSnapshot, InputMode},
        system_sim::{CpuV3SystemSim, VBlankMode},
    };
    let (program, resources, out) = application("web-cached-veil", None);
    let session = global(&program, "SESSION");
    let transition = global(&program, "TRANSITION");
    let motion = global(&program, "MOTION");
    let game = global(&program, "GAME");
    let hud = global(&program, "HUD");
    let mut observed_draws = 0;
    let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
    sim.cpu_mut().set_boot_window(&program.words);
    preload(sim.cpu_mut().physical_memory_mut(), &resources);
    let catalog: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("assets.json")).unwrap()).unwrap();
    let region = goal_region(&catalog);
    let mut goal_pixels: Option<Vec<u32>> = None;
    let mut goal_checks = 0;
    let initial_background = sim.cpu().physical_memory_slice()[0x580000 / 2..0x5b2000 / 2].to_vec();
    let mut connected = false;
    let mut moved = false;
    let mut restarted = false;
    let mut boundary = false;
    let mut held_play = 0;
    let mut released = false;
    let mut new_press = false;
    let mut accepted = false;
    let mut count = 0u16;
    let mut static_replays = 0;
    let mut consumer_depth: Option<Vec<u16>> = None;
    let mut sources = 0;
    let mut source_dirty = false;
    let mut consumers = 0;
    let mut ordinary = [false; 2];
    let mut source_images = vec![];
    let mut half = false;
    let mut opaque = false;
    let mut saw_live_roll = false;
    let mut initial_closed = false;
    for _ in 0..40_000_000 {
        if sim.waiting_for_vblank() {
            let m = sim.cpu().physical_memory_slice();
            if !boundary && m[transition] == 11 && m[transition + 1] == 380 {
                sim.publish_input(Some(ControllerSnapshot {
                    mode: InputMode::Analog,
                    buttons_active_low: !(32u16 | 8 | 4096),
                    right: [128; 2],
                    left: [128; 2],
                }));
                boundary = true;
            }
            sim.advance_vblank();
        }
        let step = sim
            .step_observing(|write, m| {
                if write.device == 4 && write.channel == 0 {
                    count = write.value;
                }
                if write.device == 4 && write.channel == 1 {
                    let at = usize::from(write.value) * 64;
                    let code = m[at..at + (usize::from(count) + 1) * 2]
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|p| u32::from(p[0]) | (u32::from(p[1]) << 16))
                        .collect::<Vec<_>>();
                    assert_eq!(code.last(), Some(&0xf0000000));
                    let mut local = vec![0u16; 2048];
                    let mut render = 0usize;
                    let mut target = 0u32;
                    let mut depth = 0u32;
                    let mut fixed = false;
                    let mut copy = false;
                    let mut invalidate = false;
                    let mut clear = None;
                    let mut depth_clear = None;
                    for c in code {
                        if c >> 28 == 3 || c >> 28 == 11 {
                            observed_draws += 1;
                        }
                        match c >> 28 {
                            1 => {
                                let src = ((c >> 10) & 0x3ffff) as usize * 16;
                                let dst = ((c >> 3) & 127) as usize * 16;
                                let n = 16 << ((c >> 1) & 3);
                                local[dst..dst + n].copy_from_slice(&m[src..src + n]);
                            }
                            6 => render = (c & 127) as usize * 16,
                            10 => {
                                let q = (c & 127) as usize * 16;
                                let w = u64::from(local[q])
                                    | (u64::from(local[q + 1]) << 16)
                                    | (u64::from(local[q + 2]) << 32)
                                    | (u64::from(local[q + 3]) << 48);
                                target = ((w & 0x3fff) << 9) as u32;
                                depth = (((w >> 14) & 0x3fff) << 9) as u32;
                            }
                            3 if local[render + 1] & 128 != 0 => {
                                fixed = true;
                                assert_eq!(local[render], 0xc000);
                                assert_ne!(local[render + 4] & 16, 0);
                                assert_eq!(
                                    local[render + 4] & 8,
                                    0,
                                    "veil must not write shared scene depth"
                                );
                            }
                            8 if c & 65536 == 0 => clear = Some(c as u16),
                            8 => depth_clear = Some(c as u16),
                            9 => invalidate = true,
                            12 => copy = true,
                            _ => {}
                        }
                    }
                    assert_eq!(
                        depth,
                        if target == 0x500000 {
                            0x540000
                        } else {
                            0x480000
                        }
                    );
                    if target == 0x500000 {
                        assert_eq!(depth_clear, Some(65535));
                        assert!(copy || clear == Some(0x2924));
                    } else if fixed {
                        consumers += 1;
                        assert_eq!(
                            invalidate, source_dirty,
                            "invalidate exactly after source writes"
                        );
                        source_dirty = false;
                        assert!(!copy);
                        assert_eq!(clear, Some(0x1107));
                        assert!(
                            depth_clear.is_none(),
                            "veil must preserve shared scene depth"
                        );
                        assert!(target == 0x400000 || target == 0x440000);
                        consumer_depth = Some(m[0x480000 / 2..0x480000 / 2 + 102400].to_vec());
                    } else if target == 0x600000 {
                        assert!(clear.is_none());
                        if copy {
                            static_replays += 1;
                            assert!(depth_clear.is_none(), "restoration loads the copied depth");
                        } else {
                            sources += 1;
                            source_dirty = true;
                            assert!(depth_clear.is_none());
                        }
                    } else {
                        assert!(target == 0x400000 || target == 0x440000);
                        if copy {
                            static_replays += 1;
                            assert!(depth_clear.is_none() && clear.is_none());
                        } else {
                            assert!(!copy && clear.is_none());
                            ordinary[usize::from(target == 0x440000)] = true;
                        }
                    }
                }
                true
            })
            .unwrap()
            .unwrap();
        match step {
            StepOutcome::Signaled(e) if e.signal_type == 4 => {
                if e.value != 0 {
                    sim.advance_host_time(std::time::Duration::from_millis(40));
                }
                let m = sim.cpu().physical_memory_slice();
                let mut buttons = 0u16;
                if m[session] == 3 && m[session + 1] >= 300 {
                    connected = true;
                }
                if boundary {
                    if !released {
                        buttons = 32 | 8 | 4096;
                        if m[session] == 0 && m[transition] == 0 {
                            assert_eq!(m[game + 5], 0);
                            assert_eq!(m[motion + 6], 0);
                            held_play += 1;
                            if held_play >= 2 {
                                buttons = 0;
                                released = true;
                            }
                        }
                    } else if !new_press {
                        buttons = 8;
                        new_press = true;
                    }
                } else if m[session] == 0 && !moved {
                    buttons = 32;
                    moved = true;
                } else if m[session] == 1 && m[session + 1] >= 40 && !restarted {
                    buttons = 8;
                    restarted = true;
                }
                if connected {
                    sim.publish_input(Some(ControllerSnapshot {
                        mode: InputMode::Analog,
                        buttons_active_low: !buttons,
                        right: [128; 2],
                        left: [128; 2],
                    }));
                } else {
                    sim.publish_input(None);
                }
            }
            StepOutcome::Signaled(e) if e.signal_type == 11 => {
                let m = sim.cpu().physical_memory_slice();
                source_images.push(m[0x600000 / 2..0x632000 / 2].to_vec());
                if m[session] == 1 {
                    saw_live_roll = true;
                }
            }
            StepOutcome::Signaled(e) if e.signal_type == 12 => {
                let m = sim.cpu().physical_memory_slice();
                if let Some(expected) = consumer_depth.take() {
                    assert_eq!(
                        expected,
                        &m[0x480000 / 2..0x480000 / 2 + 102400],
                        "veil changed shared scene depth"
                    );
                }
                assert_eq!(
                    m[hud + 1],
                    observed_draws,
                    "HUD must sum static/dynamic/veil/UI actual executions"
                );
                observed_draws = 0;
                assert_eq!(m[hud], 0);
                assert_eq!(&actual_hud_cells(m, 0)[..6], b"LEVEL ");
                if m[transition] != 0 {
                    assert_eq!(
                        source_images.last().unwrap().as_slice(),
                        &m[0x600000 / 2..0x632000 / 2],
                        "cached RTT must stay immutable between captures"
                    );
                }
                let phase = m[transition];
                let time = m[transition + 1];
                if phase == 0 {
                    let pixels = sim.render_active_framebuffer();
                    let mut current = vec![];
                    for y in region.1..region.3 {
                        current.extend_from_slice(&pixels[y * 400 + region.0..y * 400 + region.2]);
                    }
                    if let Some(expected) = &goal_pixels {
                        assert!(
                            expected == &current,
                            "goal/neighbor pixels changed during entry or the first move"
                        );
                        goal_checks += 1;
                    } else {
                        goal_pixels = Some(current);
                    }
                }
                if m[session] == 2 && m[session + 1] == 0 && !initial_closed {
                    write_display_ppm(
                        &out.join("entry-closed-no-controller-display.ppm"),
                        &sim.render_active_framebuffer(),
                    );
                    initial_closed = true;
                }
                if phase == 9 && time == 160 && !half {
                    let active = sim.display_state().active_base as usize;
                    let veil = expanded565(0x1107);
                    for y in 0..240 {
                        for x in 0..400 {
                            if x < 64 && y < 16 {
                                continue;
                            }
                            let offset =
                                (((x >> 4) * 16 + (y >> 4)) * 256) + (y & 15) * 16 + (x & 15);
                            let source = expanded565(m[0x600000 / 2 + offset]);
                            let mix: [u32; 3] =
                                std::array::from_fn(|i| (source[i] + veil[i]).div_ceil(2));
                            let expected = (((mix[0] * 31 + 127) / 255) << 11)
                                | (((mix[1] * 63 + 127) / 255) << 5)
                                | ((mix[2] * 31 + 127) / 255);
                            assert_eq!(
                                u32::from(m[active + offset]),
                                expected,
                                "half veil pixel {x},{y}"
                            );
                        }
                    }
                    write_display_ppm(
                        &out.join("half-veil-display.ppm"),
                        &sim.render_active_framebuffer(),
                    );
                    half = true;
                }
                if phase == 9 && time == 320 && !opaque {
                    let pixels = sim.render_active_framebuffer();
                    assert!(
                        pixels
                            .iter()
                            .enumerate()
                            .all(|(i, p)| (i % 400 < 64 && i / 400 < 16) || *p == 0x102039)
                    );
                    assert!(
                        pixels[..16 * 400]
                            .iter()
                            .enumerate()
                            .any(|(i, p)| i % 400 < 64 && *p != 0x102039),
                        "level HUD must remain visible over opaque veil"
                    );
                    write_display_ppm(
                        &out.join("opaque-veil-display.ppm"),
                        &sim.render_active_framebuffer(),
                    );
                    opaque = true;
                }
                if new_press && phase == 9 {
                    accepted = true;
                    break;
                }
            }
            StepOutcome::Halted { signal } => panic!(
                "unexpected cached-veil halt {signal:#x}, moved={moved} restarted={restarted} boundary={boundary} held_play={held_play}"
            ),
            _ => {}
        }
    }
    assert!(
        accepted
            && moved
            && restarted
            && boundary
            && half
            && opaque
            && saw_live_roll
            && initial_closed
    );
    assert!(ordinary.iter().all(|v| *v));
    assert!(goal_checks > 10);
    assert!(static_replays > sources);
    assert_eq!(
        sources, 3,
        "outgoing, incoming, then the next explicit restart"
    );
    assert!(consumers > sources);
    assert!(
        source_images.windows(2).any(|w| w[0] != w[1]),
        "outgoing and incoming boundaries must have different content"
    );
    assert_eq!(
        &sim.cpu().physical_memory_slice()[0x580000 / 2..0x5b2000 / 2],
        initial_background
    );
    assert!(sim.gpu().last_error().is_none());
    std::fs::write(out.join("qualification.json"),serde_json::to_vec_pretty(&serde_json::json!({"functional_gpu_actual_display":true,"boundary_snapshots":sources,"consumers":consumers,"goal_neighbor_invariant_normal_frames":goal_checks,"initial_closed_entry_without_input":initial_closed,"full_half_veil_formula_matches":half,"opaque_color_matches":opaque,"boundary_held_dpad_start_triangle_suppressed":true,"fresh_start_accepted":accepted,"both_normal_back_targets":ordinary,"source_immutable":true,"scope":"instruction CPU + functional GPU/Display; explicit40ms host ticks; no board, scanout timing or native CPU performance claim"})).unwrap()).unwrap();
}

#[test]
fn cached_veil_diagnostic_updates_transmittance_overlay_on_every_consumer() {
    use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
    let (program, resources, out) = application_case(
        "web-stage1-warm-cached-diagnostic",
        None,
        None,
        scenarios::cached_veil(),
    );
    let session = global(&program, "SESSION");
    let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
    sim.cpu_mut().set_boot_window(&program.words);
    preload(sim.cpu_mut().physical_memory_mut(), &resources);
    let mut overlays = vec![];
    let mut snapshots = 0;
    let mut starts = 0;
    let mut halted = false;
    let mut frozen = None;
    for _ in 0..4_000_000 {
        if sim.waiting_for_vblank() {
            sim.advance_vblank();
        }
        match sim
            .step_observing(|w, _| {
                if w.device == 4 && w.channel == 1 {
                    starts += 1;
                }
                true
            })
            .unwrap()
            .unwrap()
        {
            StepOutcome::Signaled(e) if e.signal_type == 4 && e.value != 0 => {
                sim.advance_host_time(std::time::Duration::from_millis(40))
            }
            StepOutcome::Signaled(e) if e.signal_type == 11 => snapshots += 1,
            StepOutcome::Signaled(e) if e.signal_type == 12 => {
                let m = sim.cpu().physical_memory_slice();
                assert_eq!(m[session + 4], 1);
                let image = m[0x600000 / 2..0x632000 / 2].to_vec();
                if let Some(old) = &frozen {
                    assert_eq!(old, &image);
                } else {
                    frozen = Some(image);
                }
                overlays.push(actual_hud_number(&actual_hud_cells(m, 0)[..4]));
            }
            StepOutcome::Halted { signal } => {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
            _ => {}
        }
    }
    assert!(halted, "bounded4M diagnostic timeout");
    assert_eq!(snapshots, 1);
    assert_eq!(starts, 8, "one cache build precedes the boundary snapshot");
    assert_eq!(overlays, [1255, 1224, 1192, 1160, 1128]);
    assert!(!sim.display_state().swap_pending);
    assert!(sim.gpu().last_error().is_none());
    write_display_ppm(
        &out.join("half-diagnostic-display.ppm"),
        &sim.render_active_framebuffer(),
    );
    std::fs::write(out.join("diagnostic-qualification.json"),serde_json::to_vec_pretty(&serde_json::json!({"overlays":overlays,"snapshots":snapshots,"submissions":starts,"maximum_steps":4000000,"snapshot_immutable":true,"selected_128_halt_after_retired_present":true})).unwrap()).unwrap();
}
