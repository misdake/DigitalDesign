//! Physical input, passive CPU function intervals and the common GPU journal.
use super::scenarios;
use super::*;
use cpu_v3_tang_nano_20k::{
    boot,
    input_device::{ControllerSnapshot, InputMode},
    system_cycle_emu::{profiling::Recorder, runner, scanout::Scanout, System},
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

fn publish(system: &mut System, buttons: u16) {
    system.publish_input(Some(ControllerSnapshot {
        mode: InputMode::Digital,
        buttons_active_low: buttons,
        left: [128; 2],
        right: [128; 2],
    }));
}
fn save_words(path: &Path, data: &[u16]) {
    std::fs::write(
        path,
        data.iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
}
fn snapshot(system: &System, addresses: &BTreeMap<&str, usize>) -> Value {
    let mut out = serde_json::Map::new();
    for (name, at) in addresses {
        out.insert(
            (*name).into(),
            json!(system.inspect_cpu_word(*at as u32).unwrap()),
        );
    }
    Value::Object(out)
}

#[test]
#[ignore = "explicit bounded physical-right-input frame and common GPU profiling"]
fn moving_right_frame_system_profile() {
    runner::host_worker(|| {
        measure();
        Ok(())
    })
    .unwrap();
}

fn measure() {
    let (program, resources, out) = application_case(
        "moving-cache-stage29",
        Some("run(5);"),
        None,
        scenarios::moving(),
    );
    std::fs::write(out.join("application.listing"), &program.listing).unwrap();
    std::fs::write(out.join("application.debug"), program.debug.render()).unwrap();
    let budget: Value =
        serde_json::from_slice(&std::fs::read(out.join("budget.json")).unwrap()).unwrap();
    let image = boot::pack_application_resources(
        &program
            .words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>(),
        budget["static_words"].as_u64().unwrap() as usize,
        &resources
            .iter()
            .map(|(a, b)| boot::BootResource::copy(*a, b.clone()))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    std::fs::write(out.join("cpu-v3-boot.bin"), &image).unwrap();
    let marker = program
        .listing
        .lines()
        .filter(|l| l.contains("signal ") && l.trim_end().ends_with(", 4"))
        .map(|l| u16::from_str_radix(l.trim().split(':').next().unwrap(), 16).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(marker.len(), 1);
    let names = [
        "prepare_block",
        "draw_bridge_group",
        "draw_switches",
        "draw_pad_part",
        "draw_shadow",
        "shadow_project",
        "shadow_hull",
        "shadow_clip",
        "shadow_batch_cell",
        "shadow_batch_finish",
        "draw_block",
        "gpu_model_matrix_prepare",
        "gpu_mesh_draw",
    ];
    let mut entries = BTreeMap::new();
    for l in program.listing.lines() {
        if let Some((name, at)) = l.split_once(" @ 0x") {
            if names.contains(&name) {
                entries.insert(u16::from_str_radix(at, 16).unwrap(), name.to_string());
            }
        }
    }
    assert!(entries.values().any(|n| n == "draw_shadow"));
    assert!(entries.values().any(|n| n == "shadow_project"));
    let session = global(&program, "GAME_SESSION.animation");
    let game = global(&program, "GAME_SESSION.rules");
    let motion = global(&program, "GAME_SESSION.motion");
    let shadow = global(&program, "SHADOW_WORKSPACE.info");
    let records = global(&program, "GPU_CB_RECORDS");
    let addresses = BTreeMap::from([
        ("phase", session),
        ("age_ms", session + 1),
        ("x", game),
        ("z", game + 1),
        ("orientation", game + 2),
        ("mask", game + 3),
        ("moves", game + 5),
        ("moving", motion + 5),
        ("hull_vertices", shadow),
        ("receivers", shadow + 1),
        ("shadow_triangles", shadow + 2),
    ]);
    let mut system = System::from_preloaded_cold(&image, 0xa55a).unwrap();
    publish(&mut system, 0xffff);
    let capture_enabled = std::env::var("BLOXORZ_MOVING_CAPTURE").as_deref() != Ok("0");
    let mut recorder = capture_enabled.then(|| {
        let mut recorder =
            Recorder::create_runner(&out.join("capture"), &image, 0, 10_000_000).unwrap();
        if let Ok(frame) = std::env::var("BLOXORZ_PROFILE_FRAME") {
            recorder.select_frame(frame.parse().unwrap()).unwrap();
        }
        recorder.initialize_memory(&system).unwrap();
        recorder
    });
    let mut raster = Scanout::default();
    let mut arrival = runner::ExecuteArrival::default();
    let mut frame = 0;
    let mut frame_submits = 0;
    let mut submission = 0;
    let mut busy = false;
    let mut events = vec![];
    let mut intervals = vec![];
    let mut stack: Vec<(String, u64, u16, u16, u16)> = vec![];
    let mut right_read = false;
    let mut acknowledged = false;
    let mut released = false;
    let mut complete = false;
    let mut dynamic_done = false;
    let started = Instant::now();
    let max_logic = 18_000_000;
    let max_host = 2400;
    while system.clock().logic_edges < max_logic
        && started.elapsed() < Duration::from_secs(max_host)
    {
        let mut entry = None;
        let mut frame_begin = false;
        let mut device = None;
        let mut retired = 0;
        let step=system.step_observing(|o| {
            if let Some(recorder) = &mut recorder { recorder.observe(o); }
            if let Some((segment,pc))=arrival.observe(o) {
                if segment==7 {
                    while stack.last().is_some_and(|f|f.2==pc) {
                        let (name,begin,ret,sp,owner)=stack.pop().unwrap();
                        intervals.push(json!({"function":name,"frame":owner,"begin_tick":begin,"end_tick":o.instant.ticks,"return_pc":ret,"entry_sp":sp}));
                    }
                    if pc==marker[0] {
                        frame+=1;frame_submits=0;frame_begin=true;
                        events.push(json!({"kind":"frame_start","frame":frame,"tick":o.instant.ticks}));
                    }
                    if let Some(name)=entries.get(&pc) { entry=Some((name.clone(),o.instant.ticks)); }
                }
            }
            device=runner::cpu_device(o);
            retired=o.cpu.map_or(0,|c|c.core.retired_words);
            if let Some(gpu)=o.gpu {
                if busy&&!gpu.control.processor.busy {
                    events.push(json!({"kind":"gpu_done","frame":frame,"submission":submission,"tick":o.instant.ticks}));
                    dynamic_done=frame==3&&frame_submits==2;
                }
                busy=gpu.control.processor.busy;
                if let Some(bus)=o.bus {
                    if bus.write&&bus.index==4&&bus.channel==1&&gpu.control.processor.submit_ready {
                        submission+=1;frame_submits+=1;
                        events.push(json!({"kind":"cpu_submit","frame":frame,"submission":submission,"tick":o.instant.ticks,"base_sector":bus.data}));
                    }
                }
            }
        }).unwrap();
        if let Some(recorder) = &mut recorder {
            recorder.after_step(&step, &system);
            assert!(!recorder.truncated(), "common GPU journal exceeded bound");
        }
        if let Some((name, tick)) = entry {
            let regs = system.core().architectural_gprs();
            stack.push((name.clone(), tick, regs[14], regs[13], frame));
            if name == "prepare_block" {
                events.push(json!({"kind":"scene_state","frame":frame,"tick":tick,"state":snapshot(&system,&addresses)}));
            }
        }
        if frame_begin && frame == 2 {
            publish(&mut system, 0xffdf);
            events.push(json!({"kind":"input","frame":frame,"tick":step.instant.ticks,"buttons_active_low":65503,"reason":"right before real frame2 Device5 capture"}));
        }
        if let Some(mut d) = device {
            d["committed"] = json!(system.core().retired_words() as u64 > retired);
            if d["device"] == 5 && d["committed"] == true {
                if !released
                    && frame >= 2
                    && d["write"] == true
                    && d["channel"].as_u64().is_some_and(|c| (8..=15).contains(&c))
                {
                    acknowledged = true;
                }
                if !released
                    && frame >= 2
                    && d["channel"] == 4
                    && d["write"] == false
                    && d["value"] == 0xdf
                {
                    right_read = true;
                }
                if !released
                    && right_read
                    && acknowledged
                    && d["write"] == false
                    && d["channel"] == 8
                    && d["value"] == 1
                {
                    publish(&mut system, 0xffff);
                    released = true;
                    events.push(json!({"kind":"input_release","frame":frame,"tick":step.instant.ticks,"buttons_active_low":65535,"state":snapshot(&system,&addresses)}));
                }
                events.push(d.clone());
            }
            if d["device"] == 3
                && d["write"] == true
                && d["channel"] == 3
                && frame == 3
                && dynamic_done
            {
                events.push(
                    json!({"kind":"present_request","frame":frame,"tick":step.instant.ticks}),
                );
                complete = true;
            }
            if d["device"] == 4
                && d["write"] == true
                && d["channel"] == 1
                && frame == 3
                && frame_submits == 2
            {
                let m = system.backing();
                let c = (0..224)
                    .map(|i| system.inspect_cpu_word((records + i) as u32).unwrap())
                    .collect::<Vec<_>>();
                save_words(&out.join("moving-before-dynamic.bin"), m);
                save_words(
                    &out.join("moving-command.bin"),
                    &m[usize::from(c[1]) * 64..usize::from(c[1]) * 64 + usize::from(c[5])],
                );
                events.push(json!({"kind":"dynamic_state","frame":frame,"tick":step.instant.ticks,"state":snapshot(&system,&addresses),"record":c}));
            }
        }
        if let Some(recorder) = &mut recorder {
            if step.instant.pixel_rise {
                if let Some(f) = raster.sample(step.display.video).unwrap() {
                    recorder.frame(&f, step.instant.ticks).unwrap();
                }
            }
        }
        if let Some(cpu) = step.cpu {
            assert!(!cpu.fault && !cpu.halted, "unexpected CPU stop");
        }
        if complete {
            save_words(
                &out.join("moving-color.bin"),
                &system.backing()[0x440000 / 2..0x440000 / 2 + 102400],
            );
            save_words(
                &out.join("moving-depth.bin"),
                &system.backing()[0x480000 / 2..0x480000 / 2 + 102400],
            );
            break;
        }
        if step.instant.logic_rise && system.clock().logic_edges.is_multiple_of(1_000_000) {
            std::fs::write(out.join("progress.json"),serde_json::to_vec_pretty(&json!({"events":events,"logic":system.clock().logic_edges,"host_seconds":started.elapsed().as_secs_f64()})).unwrap()).unwrap();
            eprintln!(
                "moving: {} logic, frame {frame}, {:.1}s",
                system.clock().logic_edges,
                started.elapsed().as_secs_f64()
            );
        }
    }
    let report = json!({"complete":complete,"quantum_hz":system.clock().quantum_hz,"frame":3,"events":events,"cpu_intervals":intervals,
        "open_cpu_intervals":stack,"right_read":right_read,"acknowledged":acknowledged,"released":released,"max_logic":max_logic,"max_host_seconds":max_host,"host_seconds":started.elapsed().as_secs_f64(),
        "gpu_journal":capture_enabled,"timing_scope":"first unheld function Execute to matching link-register return PC; inclusive CPU wall intervals include stalls; physical Device5 right press, natural animation clock; synthetic cold preload"});
    std::fs::write(
        out.join("timing.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    if let Some(recorder) = recorder {
        recorder.finish(json!({"entry":"cold-preloaded","reached":complete,"stop_reason":"third-frame dynamic drained and present requested",
            "runner_selected_presentations":[3],"runner_max_native_command_images":1024}),&system).unwrap();
    }
    assert!(
        complete && right_read && released,
        "bounded physical input/frame qualification"
    );
    let state = report["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "dynamic_state")
        .unwrap();
    assert_eq!(state["state"]["phase"], 1);
    assert!(state["state"]["age_ms"].as_u64().unwrap() > 0);
    assert_eq!(state["state"]["x"], 8);
    assert_eq!(state["state"]["mask"], 25);
    assert!(report["cpu_intervals"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["frame"] == 3 && e["function"] == "shadow_project"));
    assert!(report["cpu_intervals"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["frame"] == 3 && e["function"] == "draw_shadow"));
}
