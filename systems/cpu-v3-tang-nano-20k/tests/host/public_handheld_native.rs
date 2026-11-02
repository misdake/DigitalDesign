//! Bounded controlled Stage2 package on the two existing CPU runtimes.
use super::*;
use cpu_v3_tang_nano_20k::{
    boot,
    profiling_capture::{self, CapturePoint},
    system_emu::CpuV3SystemEmu,
    system_sim::{CpuV3SystemSim, VBlankMode},
};
use resolver::capture_profile::abi::*;
fn controlled_source() -> String {
    let source = include_str!("../../examples/gpu_demo/rcc/public_main.rs");
    let first = source.find("fn prepare_ui(").unwrap();
    let end = first + source[first..].find("\nfn ").unwrap();
    let mut source = format!(
        "{}fn prepare_ui(grid:&mut UiGrid)->u16{{grid.fill(0);demo_glyph(grid,2,1,3);1}}{}",
        &source[..first],
        &source[end..]
    );
    source = source
        .replace("demo_run(0, 2000);", "demo_run(2, 2000);")
        .replace(
            "let rabbit_model = demo_identity_model();",
            "let mut rabbit_model = demo_identity_model();",
        );
    let anchor = "        // The persistent stream keeps geometry, Render, END and padding.";
    assert_eq!(source.matches(anchor).count(), 1);
    source = source.replace(
        anchor,
        &format!("        if frame == 1 {{stage2_camera(&mut camera);}}\n{anchor}"),
    );
    let anchor = "        // Only dynamic CPU storage is touched while the static GPU work runs.";
    assert_eq!(source.matches(anchor).count(), 1);
    source = source.replace(
        anchor,
        &format!("{anchor}\n        if frame == 1 {{stage2_rabbit(&mut rabbit_model);}}"),
    );
    assert_eq!(source.matches("        frame += 1;").count(), 1);
    source = source.replace(
        "        frame += 1;",
        "        frame += 1;\n        if frame == 1 {gpu.request_snapshot();}",
    );
    source.push_str(
        r#"
fn stage2_camera(camera:&mut Camera){camera.set_pose(
 demo_point(fix32::from_words(16384,0),fix32::from_words(14418,0),fix32::from_words(52429,2)),
 demo_point(fix32::from_words(3277,0),fix32::from_words(24576,3),fix32::zero()));}
fn stage2_rabbit(model:&mut Model){model.set_pose(demo_pose(demo_zero(),
 demo_point(fix32::zero(),fix32::from_words(8192,0),fix32::zero()),demo_scale()));}
"#,
    );
    source
}
fn package(program: &rcc_backend::CpuV3Program, out: &Path) -> Vec<u8> {
    let budget: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("budget-false.json")).unwrap()).unwrap();
    let catalog: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("assets.json")).unwrap()).unwrap();
    let resources: Vec<_> = catalog["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            boot::BootResource::copy(
                r["address"].as_u64().unwrap() as usize,
                std::fs::read(out.join(r["file"].as_str().unwrap())).unwrap(),
            )
        })
        .collect();
    boot::pack_application_resources(
        &program
            .words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>(),
        budget["static_words"].as_u64().unwrap() as usize,
        &resources,
    )
    .unwrap()
}
fn save_words(out: &Path, name: &str, words: &[u16]) {
    std::fs::write(
        out.join(name),
        words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
}
fn plane(memory: &[u16], base: usize) -> Vec<u16> {
    let words = memory[base..base + 131072].to_vec();
    assert!(
        words[102400..].iter().all(|&w| w == 0x5aa5),
        "complete allocation tail guard at {base:x}"
    );
    words
}
fn render_pair(image: &[u8], native: bool, out: &Path) -> (Vec<Vec<u16>>, Vec<Recorded>) {
    let mut fast = (!native).then(|| {
        CpuV3SystemSim::from_single_flash(image, 0x5aa5, VBlankMode::PauseOnFrameIndexWait)
            .unwrap()
            .with_functional_gpu()
    });
    let mut emu = native.then(|| {
        CpuV3SystemEmu::from_single_flash(image, 0x5aa5, VBlankMode::PauseOnFrameIndexWait)
            .unwrap()
            .with_functional_gpu()
    });
    let mut retired = Vec::new();
    let mut submissions = Vec::new();
    let mut count = None;
    let mut halted = false;
    for _ in 0..4_000_000 {
        let mut observe = |write: cpu_v3::DeviceWrite, memory: &[u16]| {
            if write.device == 4 {
                if write.channel == 0 {
                    count = Some(write.value);
                } else if write.channel == 1 {
                    let role = if submissions.is_empty() {
                        0
                    } else if submissions.len() % 2 == 1 {
                        1
                    } else {
                        2
                    };
                    submissions.push(recorded(memory, write.value, count.take().unwrap(), role));
                }
            }
            true
        };
        if let Some(s) = emu.as_mut() {
            assert!(s.gpu().last_error().is_none());
            if s.waiting_for_vblank() && s.advance_vblank() {
                let memory = s.physical_memory();
                retired.push(plane(&memory, s.display_state().active_base as usize));
                retired.push(plane(&memory, 0x240000));
            }
            let o = s.step_observing(&mut observe).unwrap().unwrap();
            assert!(!o.fault);
            if o.halted {
                assert_eq!(o.halt_signal, 0x600d);
                halted = true;
                break;
            }
        } else {
            let s = fast.as_mut().unwrap();
            assert!(s.gpu().last_error().is_none());
            if s.waiting_for_vblank() && s.advance_vblank() {
                let memory = s.cpu().physical_memory_slice();
                retired.push(plane(memory, s.display_state().active_base as usize));
                retired.push(plane(memory, 0x240000));
            }
            if let StepOutcome::Halted { signal } = s.step_observing(&mut observe).unwrap().unwrap()
            {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
        }
    }
    assert!(halted, "4M CPU advances bound");
    assert_eq!(retired.len(), 4);
    assert_eq!(submissions.len(), 5, "startup then static/dynamic twice");
    let mut planes = if let Some(s) = emu.as_ref() {
        let memory = s.physical_memory();
        [0x200000, 0x220000, 0x240000]
            .map(|base| plane(&memory, base))
            .to_vec()
    } else {
        let memory = fast.as_ref().unwrap().cpu().physical_memory_slice();
        [0x200000, 0x220000, 0x240000]
            .map(|base| plane(memory, base))
            .to_vec()
    };
    planes.extend(retired);
    for (index, words) in planes.iter().enumerate() {
        save_words(
            out,
            &format!(
                "{}-plane-{index}.bin",
                if native { "native" } else { "instruction" }
            ),
            words,
        );
    }
    (planes, submissions)
}
fn pair(header: &[u16], at: u16) -> u32 {
    u32::from(header[usize::from(at)]) | (u32::from(header[usize::from(at) + 1]) << 16)
}
#[test]
#[ignore = "bounded actual Stage2 package, two CPU backends, functional GPU and Prepared gate"]
fn controlled_camera_rabbit_and_first_eligible_native_capture() {
    let out = qualification_assets("native-eligible");
    let source = controlled_source();
    std::fs::write(out.join("controlled.rs"), &source).unwrap();
    // This is an explicitly changed test package with fixed UI. Normal APP/timers stay unchanged.
    let control = compile_mode(&source, &out, false, false);
    let control_image = package(&control, &out);
    std::fs::write(out.join("control.cb06"), &control_image).unwrap();
    let (fast_planes, fast_trace) = render_pair(&control_image, false, &out);
    let (native_planes, native_trace) = render_pair(&control_image, true, &out);
    assert!(
        fast_planes == native_planes,
        "all full planes/retirements including guards across backends"
    );
    assert_ne!(
        fast_trace[1].matrix, fast_trace[3].matrix,
        "frame2 camera reaches persistent static patch"
    );
    assert_ne!(
        fast_trace[3].matrix, fast_trace[4].matrix,
        "frame2 rabbit has independent Y transform"
    );
    for (a, b) in fast_trace.iter().zip(&native_trace) {
        assert_eq!(
            a.trace, b.trace,
            "complete consumed mesh/global/render/instance trace"
        );
        assert_eq!(a.ui_descriptors, b.ui_descriptors);
    }
    let capture = compile_mode(&source, &out, false, true);
    let image = package(&capture, &out);
    std::fs::write(out.join("capture.cb06"), &image).unwrap();
    std::fs::write(out.join("capture.debug"), capture.debug.render()).unwrap();
    let point = CapturePoint::from_program(&capture).unwrap();
    let fast =
        profiling_capture::capture_flash_with_functional_prefix(&image, point, 0x5aa5, 4_000_000)
            .unwrap();
    let native = profiling_capture::capture_native_flash_with_functional_prefix(
        &image, point, 0x5aa5, 4_000_000,
    )
    .unwrap();
    save_words(&out, "instruction-header.bin", &fast.header);
    save_words(&out, "instruction-ranges.bin", &fast.ranges);
    save_words(&out, "native-header.bin", &native.data.header);
    save_words(&out, "native-ranges.bin", &native.data.ranges);
    save_words(&out, "native-physical.bin", &native.physical_memory);
    save_words(
        &out,
        "native-physical-header.bin",
        &native.physical_memory[point.header_word..point.header_word + 64],
    );
    for (i, bytes) in native.data.payloads.iter().enumerate() {
        std::fs::write(out.join(format!("payload-{i}.bin")), bytes).unwrap();
    }
    assert_eq!(native.data.header, fast.header, "CPU-private metadata view");
    assert_eq!(native.data.ranges, fast.ranges);
    assert_eq!(
        native.data.payloads, fast.payloads,
        "every GPU-consumed range stays raw physical memory"
    );
    assert_eq!(
        pair(&fast.header, GPU_CAPTURE_H_SUBMISSION),
        5,
        "ordinary static submission retains snapshot request; first gate is dynamic F2"
    );
    assert_eq!(fast.header[usize::from(GPU_CAPTURE_H_SLOT)], 1);
    assert_eq!(native.display_state.active_base, 0x220000);
    assert!(!native.display_state.swap_pending);
    assert_eq!(
        pair(&fast.header, GPU_CAPTURE_H_FRONT_BASE),
        0,
        "explicit-v2 omits Front; inspect actual physical image separately"
    );
    assert_eq!(pair(&fast.header, GPU_CAPTURE_H_COLOR_BASE), 0x400000);
    assert_eq!(pair(&fast.header, GPU_CAPTURE_H_DEPTH_BASE), 0x480000);
    let base = pair(&fast.header, GPU_CAPTURE_H_COMMAND_BASE);
    let bytes = pair(&fast.header, GPU_CAPTURE_H_COMMAND_BYTES);
    let descriptor = gpu_v2::system::vliw_command::DeviceSubmission::new(base, bytes).unwrap();
    let target = recorded(
        &native.physical_memory,
        descriptor.base_sector,
        descriptor.count_minus_one,
        2,
    );
    assert_eq!(target.trace, fast_trace[4].trace);
    assert_eq!(target.ui_descriptors, fast_trace[4].ui_descriptors);
    // The active Front is F1; the current Back contains completed F2 static geometry, not the rabbit.
    assert_eq!(plane(&native.physical_memory, 0x220000), fast_planes[3]);
    let mut replay = native.physical_memory.clone();
    let before = replay.clone();
    let report = gpu_v2::system::functional::Executor::new(64_000_000)
        .execute(&mut replay, descriptor)
        .unwrap();
    assert_eq!(report.effects.target, Some([0x400000, 0x480000]));
    assert_eq!(plane(&replay, 0x200000), fast_planes[0]);
    assert_eq!(plane(&replay, 0x220000), fast_planes[1]);
    assert_eq!(plane(&replay, 0x240000), fast_planes[2]);
    assert!(
        before
            .iter()
            .zip(&replay)
            .enumerate()
            .all(|(i, (a, b))| a == b
                || (0x200000..0x219000).contains(&i)
                || (0x240000..0x259000).contains(&i)),
        "dynamic target writes only Back/depth; all other physical guards preserved"
    );
    assert_ne!(
        plane(&before, 0x200000),
        plane(&replay, 0x200000),
        "target rabbit really changes completed-static preimage"
    );
    for (name, base) in [
        ("replayed-color-a.bin", 0x200000),
        ("replayed-color-b.bin", 0x220000),
        ("replayed-depth.bin", 0x240000),
    ] {
        save_words(&out, name, &plane(&replay, base));
    }
    std::fs::write(out.join("qualification.json"),serde_json::to_vec_pretty(&serde_json::json!({"base":"87b3f7ce","private_cpu":"18975aa4-not-accepted","controlled_source":true,"fixed_ui":true,"normal_timers_unchanged":true,"gpu_display_competition":false,"debug":false,"logic_cycles":native.logic_cycles,"native_cycles":native.native_cycles,"instruction_steps":fast.cpu_steps,"native_vblank_events":native.vblank_events,"gate_pc":point.gate_pc,"header_word":point.header_word,"ranges_word":point.ranges_word,"actual_gate_submission":5,"actual_gate_slot":1,"gpu_target_consumed_before_snapshot":false,"all_payload_ranges_equal":true,"full_frames_and_guards_equal":true,"frame2_camera_and_independent_rabbit_y":true,"physical_header_differs_from_cpu_metadata":native.physical_memory[point.header_word..point.header_word+64]!=native.data.header})).unwrap()).unwrap();
    eprintln!("NATIVE_ELIGIBLE_ARTIFACTS {}", out.display());
}
