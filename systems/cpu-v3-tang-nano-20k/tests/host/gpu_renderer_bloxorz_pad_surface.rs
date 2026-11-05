//! Bounded ordinary-renderer pad and marker repair evidence, with frozen controls.
use super::{application_from_sources, global, preload, write_display_ppm};
use cpu_v3::StepOutcome;
use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
use std::path::PathBuf;

#[test]
#[ignore = "explicit frozen-control or repair candidate pad rendering; no board"]
fn pad_surface_fixed_controls_and_candidate() {
    let arm = std::env::var("BLOXORZ_PAD_ARM").expect("explicit arm");
    assert!(matches!(arm.as_str(), "8235" | "fe171" | "7x8" | "8x8"));
    let root =
        PathBuf::from(std::env::var_os("BLOXORZ_SIM_OUTPUT_ROOT").expect("explicit fresh root"));
    let control_root =
        std::env::var_os("BLOXORZ_PAD_CONTROL_ROOT").map_or_else(|| root.clone(), PathBuf::from);
    let frozen = if arm == "8235" {
        Some(control_root.join("control-8235/systems/cpu-v3-tang-nano-20k"))
    } else if arm == "fe171" {
        Some(control_root.join("control-fe171/systems/cpu-v3-tang-nano-20k"))
    } else {
        None
    };
    let name = if frozen.is_some() {
        format!("fragile-accepted-pad-{arm}")
    } else {
        format!("fragile-pad-candidate-{arm}")
    };
    let (program, resources, out) =
        application_from_sources(&name, Some("run(13,false,128);"), frozen.as_deref());
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
    let session = global(&program, "SESSION");
    let mut frames = vec![];
    let mut count = 0;
    let mut inputs = vec![];
    let mut halted = false;
    let mut minimum_sp = 0xc000;
    let mut steps = 0;
    for _ in 0..64_000_000 {
        if sim.waiting_for_vblank() {
            sim.advance_vblank();
        }
        steps += 1;
        let frame = frames.len();
        let outcome=sim.step_observing(|write,memory|{
            if write.device==4 && write.channel==0 {count=write.value;}
            if write.device==4 && write.channel==1 && [0,2,5,7,9,10,11,12].contains(&frame) {
                let file=format!("gpu-before-{}.bin",inputs.len());
                std::fs::write(out.join(&file),memory.iter().flat_map(|v|v.to_le_bytes()).collect::<Vec<_>>()).unwrap();
                inputs.push(serde_json::json!({"frame":frame,"memory":file,"base_bytes":u32::from(write.value)*128,"count_minus_one":count,"maximum_work":10000000}));
            }true
        }).unwrap().unwrap();
        let sp = sim.cpu().registers()[13];
        if sp != 0 {
            minimum_sp = minimum_sp.min(sp);
        }
        match outcome {
            StepOutcome::Signaled(e) if e.signal_type == 12 => {
                let rgb = sim.render_active_framebuffer();
                write_display_ppm(&out.join(format!("frame-{frame}.ppm")), &rgb);
                let m = sim.cpu().physical_memory_slice();
                let expected_mode = [0, 2, 2, 3, 0, 0, 4, 5, 7, 0, 0, 0, 0][frame];
                let expected_time = [0, 0, 100, 190, 0, 0, 70, 180, 20, 0, 0, 0, 0][frame];
                assert_eq!(
                    &m[session..session + 2],
                    &[expected_mode, expected_time],
                    "actual session frame {frame}"
                );
                for (label, at) in [
                    ("color", sim.display_state().active_base as usize),
                    ("depth", 0x480000 / 2),
                ] {
                    std::fs::write(
                        out.join(format!("frame-{frame}-{label}.bin")),
                        m[at..at + 102400]
                            .iter()
                            .flat_map(|v| v.to_le_bytes())
                            .collect::<Vec<_>>(),
                    )
                    .unwrap();
                }
                frames.push(serde_json::json!({"frame":frame,"level":if frame<5 {1}else{if frame<9 {2}else{if frame==9 {15}else{if frame==10 {17}else{if frame==11 {4}else{8}}}}},"session":m[session..session+2].to_vec()}));
            }
            StepOutcome::Halted { signal } => {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
            _ => {}
        }
    }
    assert!(halted, "bounded64M pad {arm}");
    assert_eq!(frames.len(), 13);
    assert!(minimum_sp >= 40959);
    assert!(sim.gpu().last_error().is_none());
    std::fs::write(out.join("qualification.json"),serde_json::to_vec_pretty(&serde_json::json!({"arm":arm,"mode":"instruction CPU + functional GPU, ordinary renderer, controlled real session actions; no hardware timing","max_steps":64000000,"steps":steps,"minimum_SP_word":minimum_sp,"gpu_inputs":inputs,"frames":frames})).unwrap()).unwrap();
}
