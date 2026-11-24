use super::scenarios;
use super::*;

/// Four real aperture phases and the two dynamic-neighbor fallback levels.
#[test]
#[ignore = "bounded production goal aperture and dynamic-neighbor qualification"]
fn goal_carrier_surface_ownership_diagnostic() {
    use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
    assert!(std::env::var_os("BLOXORZ_CARRIER_PROBE").is_none());
    let extra_only = std::env::var("BLOXORZ_CARRIER_CASE_SET").is_ok_and(|v| {
        assert_eq!(v, "closing-and-fallback");
        true
    });
    for (variant, expected) in [
        ("carrier-play", [0u16, 0]),
        ("carrier-open", [4, 100]),
        ("carrier-drop", [5, 210]),
        ("carrier-close", [7, 100]),
        ("carrier-soft-fallback", [0, 0]),
        ("carrier-heavy-fallback", [0, 0]),
    ] {
        if extra_only
            && (variant == "carrier-play" || variant == "carrier-open" || variant == "carrier-drop")
        {
            continue;
        }
        let (program, resources, out) = application_case(
            &format!("opt-shadow-{variant}"),
            None,
            None,
            scenarios::shadow_diagnostic(variant),
        );
        let catalog: serde_json::Value =
            serde_json::from_slice(&std::fs::read(out.join("assets.json")).unwrap()).unwrap();
        assert_eq!(
            catalog["goal_carrier"]["fallback_levels"],
            serde_json::json!([15, 17])
        );
        let carrier_records: Vec<u64> = catalog["meshes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["name"].as_str().unwrap().starts_with("goal-carrier-"))
            .map(|m| m["record_sector"].as_u64().unwrap())
            .collect();

        std::fs::write(
            out.join("program.bin"),
            program
                .words
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
        sim.cpu_mut().set_boot_window(&program.words);
        preload(sim.cpu_mut().physical_memory_mut(), &resources);
        let mut inputs = vec![];
        let mut count = 0u16;
        let mut halted = false;
        let mut steps = 0;
        for _ in 0..4_000_000 {
            if sim.waiting_for_vblank() {
                sim.advance_vblank();
            }
            steps += 1;
            let outcome=sim.step_observing(|write,memory|{
                if write.device==4&&write.channel==0{count=write.value;}
                if write.device==4&&write.channel==1 {
                    let file=format!("gpu-before-{}.bin",inputs.len());std::fs::write(out.join(&file),memory.iter().flat_map(|w|w.to_le_bytes()).collect::<Vec<_>>()).unwrap();
                    let base=usize::from(write.value)*64;let mut banks=[0usize;2];let mut carrier_apertures=vec![];let mut draws=0;let mut triangles=0;
                    for pair in memory[base..base+(usize::from(count)+1)*2].as_chunks::<2>().0 {
                        let code=u32::from(pair[0])|(u32::from(pair[1])<<16);
                        if code>>28==2 {let at=((code>>12)&65535) as usize*64;let header=u32::from(memory[at])|(u32::from(memory[at+1])<<16);banks[(code&1) as usize]=((header>>8)&127) as usize;if let Some(aperture)=carrier_records.iter().position(|r|*r==(at/64) as u64){carrier_apertures.push(aperture);}}
                        if code>>28==3 {draws+=1;triangles+=banks[(code&1) as usize];}
                    }
                    inputs.push(serde_json::json!({"memory":file,"base_bytes":u32::from(write.value)*128,"count_minus_one":count,"maximum_work":10000000,"submitted_draws":draws,"submitted_triangles":triangles,"carrier_apertures":carrier_apertures}));
                }true
            }).unwrap().unwrap();
            if let StepOutcome::Signaled(e) = outcome {
                if e.signal_type == 13 {
                    let at = usize::from(e.value);
                    assert!(
                        world_determinant(&sim.cpu().physical_memory_slice()[at..at + 24]) > 0.
                    );
                }
            }
            if let StepOutcome::Halted { signal } = outcome {
                assert_eq!(signal, 0x600d, "{:?}", sim.gpu().last_error());
                halted = true;
                break;
            }
        }
        assert!(halted, "bounded carrier pose timeout");
        assert!(sim.gpu().last_error().is_none());
        let selected: Vec<u64> = inputs
            .iter()
            .flat_map(|input| {
                input["carrier_apertures"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap())
            })
            .collect();
        if variant.ends_with("fallback") {
            assert!(selected.is_empty());
        } else {
            assert_eq!(selected.len(), 1);
            match variant {
                "carrier-play" => assert_eq!(selected, [43]),
                "carrier-open" => assert_eq!(selected, [45]),
                "carrier-drop" => assert_eq!(selected, [47]),
                _ => assert!(selected[0] == 23 || selected[0] == 24),
            }
        }

        let m = sim.cpu().physical_memory_slice();
        let at = global(&program, "GAME_SESSION.animation");
        assert_eq!(&m[at..at + 2], &expected);
        let game = global(&program, "GAME_SESSION.rules");
        let active = sim.display_state().active_base as usize;
        for (filename, base) in [
            ("color-physical.bin", active),
            ("depth-physical.bin", 0x480000 / 2),
        ] {
            std::fs::write(
                out.join(filename),
                m[base..base + 102400]
                    .iter()
                    .flat_map(|w| w.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        }
        write_display_ppm(
            &out.join("frame-display.ppm"),
            &sim.render_active_framebuffer(),
        );
        std::fs::write(out.join("qualification.json"),serde_json::to_vec_pretty(&serde_json::json!({"variant":variant,"surface":"production-goal-carrier","level":if variant=="carrier-soft-fallback"{15}else if variant=="carrier-heavy-fallback"{17}else{2},"camera":"own stage eye/focal and conservative far","game":m[game..game+10],"session":expected,"steps":steps,"max_steps":4000000,"gpu_inputs":inputs,"active_color_byte_base":active*2,"display":"actual400x240 Display output","limits":"representative production states; all48 aperture meshes validated separately, not all33 whole-frame qualification; real hole geometry, no depth bias/Always override"})).unwrap()).unwrap();
    }
}
