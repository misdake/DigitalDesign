//! CPU cycle emulation with synchronous functional GPU and a fixed shadow pose.
use super::*;
use cpu_v3_tang_nano_20k::{
    boot,
    input_device::{ControllerSnapshot, InputMode},
    system_emu::CpuV3SystemEmu,
    system_sim::{CpuV3SystemSim, VBlankMode},
};
use serde_json::json;

fn input(buttons: u16) -> Option<ControllerSnapshot> {
    Some(ControllerSnapshot {
        mode: InputMode::Digital,
        buttons_active_low: buttons,
        left: [128; 2],
        right: [128; 2],
    })
}
fn save(path: &Path, words: &[u16]) {
    std::fs::write(
        path,
        words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
}

#[test]
#[ignore = "explicit bounded CPU emu + functional GPU, fixed level29 shadow frame"]
fn level29_shadow_frame_cpu_cycles_and_instruction_oracle() {
    let arm = std::env::var("BLOXORZ_CPU_ARM").expect("explicit CPU arm");
    let (program, resources, root) = application("cpu-shadow-stage29", Some("run(3,false,128);"));
    let out = root.join(&arm);
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("application.listing"), &program.listing).unwrap();
    let budget: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("budget.json")).unwrap()).unwrap();
    let image = boot::pack_application_resources(
        &program
            .words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>(),
        budget["static_words"].as_u64().unwrap() as usize,
        &resources
            .iter()
            .map(|(p, b)| boot::BootResource::copy(*p, b.clone()))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    std::fs::write(out.join("cpu-v3-boot.bin"), &image).unwrap();

    let mut oracle =
        CpuV3SystemSim::from_single_flash(&image, 0, VBlankMode::PauseOnFrameIndexWait)
            .unwrap()
            .with_functional_gpu();
    oracle.publish_input(input(0xffff));
    let mut expected = vec![];
    let mut halted = false;
    for _ in 0..12_000_000 {
        if oracle.waiting_for_vblank() {
            oracle.advance_vblank();
        }
        match oracle.step().unwrap() {
            StepOutcome::Signaled(e) if e.signal_type == 4 && e.value == 1 => {
                oracle.publish_input(input(0xffdf))
            }
            StepOutcome::Signaled(e) if e.signal_type == 12 => {
                let memory = oracle.cpu().physical_memory_slice();
                let active = oracle.display_state().active_base as usize;
                expected.push((
                    memory[active..active + 102400].to_vec(),
                    memory[0x480000 / 2..0x480000 / 2 + 102400].to_vec(),
                    oracle.render_active_framebuffer(),
                ));
                if expected.len() == 2 {
                    oracle.publish_input(input(0xffff));
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
    assert_eq!(expected.len(), 3);
    assert!(oracle.gpu().last_error().is_none());

    let mut native =
        CpuV3SystemEmu::from_single_flash(&image, 0, VBlankMode::PauseOnFrameIndexWait)
            .unwrap()
            .with_functional_gpu();
    native.publish_input(input(0xffff));
    let wall = std::time::Instant::now();
    let mut last = None;
    let mut frame = 0;
    let mut start = 0;
    let mut submits = vec![];
    let mut frames = vec![];
    let mut state = None;
    let mut minimum_sp = 0xc000u16;
    let session = global(&program, "SESSION");
    let game = global(&program, "GAME");
    let shadow = global(&program, "SHADOW_INFO");
    halted = false;
    for _ in 0..24_000_000 {
        assert!(
            wall.elapsed().as_secs() < 240,
            "bounded CPU emu host timeout"
        );
        if native.waiting_for_vblank() {
            native.advance_vblank();
        }
        let cycle = native.cycles();
        let outcome=native.step_observing(|w,m| {
            if w.device==4&&w.channel==1 {
                submits.push(json!({"frame":frame,"cycle":cycle,"base_sector":w.value}));
                if frame==3 {
                    state=Some(json!({"phase":m[session],"age_ms":m[session+1],"moves":m[game+5],"x":m[game],"z":m[game+1],"orientation":m[game+2],"mask":m[game+3],"hull":m[shadow],"receivers":m[shadow+1],"triangles":m[shadow+2]}));
                }
            }
            true
        }).unwrap().unwrap();
        assert!(!outcome.fault);
        let (event, sp) = {
            let core = native.core();
            (core.signal_event(), core.architectural_gprs()[13])
        };
        if sp != 0 {
            minimum_sp = minimum_sp.min(sp);
        }
        if event != last {
            if let Some(e) = event {
                match e.signal_type {
                    4 => {
                        frame += 1;
                        start = native.cycles();
                        if frame == 2 {
                            native.publish_input(input(0xffdf));
                        }
                    }
                    12 => {
                        let i = frames.len();
                        let active = native.display_state().active_base as usize;
                        let m = native.physical_memory();
                        assert_eq!(&m[active..active + 102400], expected[i].0);
                        assert_eq!(&m[0x480000 / 2..0x480000 / 2 + 102400], expected[i].1);
                        if frame == 3 {
                            save(&out.join("frame3-color.bin"), &m[active..active + 102400]);
                            save(
                                &out.join("frame3-depth.bin"),
                                &m[0x480000 / 2..0x480000 / 2 + 102400],
                            );
                        }
                        drop(m);
                        assert_eq!(native.render_active_framebuffer(), expected[i].2);
                        let end = native.cycles();
                        frames.push(json!({"frame":frame,"start":start,"end":end,"cpu_cycles":end-start,"cpu_ms":(end-start) as f64/54000.0}));
                        if frame == 2 {
                            native.publish_input(input(0xffff));
                        }
                    }
                    _ => {}
                }
            }
            last = event;
        }
        if outcome.halted {
            assert_eq!(outcome.halt_signal, 0x600d);
            halted = true;
            break;
        }
    }
    assert!(halted);
    assert_eq!(frames.len(), 3);
    assert!(native.gpu().last_error().is_none());
    assert!(minimum_sp >= 0xa000);
    let state = state.unwrap();
    assert_eq!(state["phase"], 1);
    assert_eq!(state["age_ms"], 17);
    assert_eq!(state["moves"], 1);
    assert_eq!(state["mask"], 25);
    assert_eq!(state["triangles"], 11);
    write_display_ppm(&out.join("frame3-display.ppm"), &expected[2].2);
    std::fs::write(out.join("cpu-frame.json"),serde_json::to_vec_pretty(&json!({"arm":arm,"frames":frames,"submissions":submits,"state":state,"minimum_sp":minimum_sp,"host_seconds":wall.elapsed().as_secs_f64(),"native_cycles":native.cycles(),"memory_cycles":native.native_cycles(),"max_cycles":24000000,"max_host_seconds":240,"instruction_oracle_equal":true,"scope":"CPU native/cycle emu + synchronous functional GPU; fixed level29 PLAY, real Device5 right then release, test-only dt0/0/17ms; signal4 to signal12 includes CPU recording, submission and functional present overhead; excludes physical GPU execution, shared contention and scanout; not whole-system frame time or FPS"})).unwrap()).unwrap();
    eprintln!("CPU timing: {}", out.join("cpu-frame.json").display());
}
