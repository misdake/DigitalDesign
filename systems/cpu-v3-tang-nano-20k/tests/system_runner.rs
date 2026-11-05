//! Small actual-System input/timer/watch/inspection parity fixture.
use cpu_v3::{device_receive, device_send, halt, load, load_immediate16, prefixed, store};
use cpu_v3_tang_nano_20k::{
    boot::{pack_application_resources, BootResource},
    input_device::{ControllerSnapshot, InputMode},
    system_cycle_emu::{profiling::Recorder, runner, System},
};
use serde_json::json;
fn publish(s: &mut System, buttons: u16) {
    s.publish_input(Some(ControllerSnapshot {
        mode: InputMode::Digital,
        buttons_active_low: buttons,
        left: [128; 2],
        right: [128; 2],
    }));
}
#[test]
fn actual_device_edges_watch_and_cache_inspection_do_not_change_any_output() {
    use gpu_v2::system::vliw_command::{memcpy_params, Command, CommandBuffer, DeviceSubmission};
    let mut cb = CommandBuffer::new();
    for c in [
        Command::dma_local(0x110000, 264, 32).unwrap(),
        Command::wait(true, true),
        Command::FLUSH,
        Command::memcpy(264).unwrap(),
        Command::END,
    ] {
        cb.push(c);
    }
    let image = cb.image().unwrap();
    let submit = DeviceSubmission::new(0x100000, image.valid_bytes).unwrap();
    let mut code = load_immediate16(5, 0x2100).to_vec();
    let prefix_site = code.len() as u16;
    code.extend(prefixed(load(7, 5, 0), 16));
    code.extend(load_immediate16(0, 1));
    code.push(device_send(0, 0, 11));
    code.push(device_receive(1, 0, 11));
    code.extend(load_immediate16(0, submit.count_minus_one));
    code.push(device_send(0, 4, 0));
    code.extend(load_immediate16(0, submit.base_sector));
    code.push(device_send(0, 4, 1));
    code.extend(load_immediate16(0, (4 << 3) | 5));
    code.push(device_send(0, 0, 6));
    code.extend(load_immediate16(0, 255));
    code.push(device_send(0, 0, 7));
    code.push(device_receive(2, 5, 4));
    code.push(device_send(0, 5, 8));
    code.push(device_receive(3, 5, 4));
    code.push(device_receive(4, 0, 11));
    code.extend(load_immediate16(5, 0x2000));
    code.extend(load_immediate16(6, 0xabcd));
    code.push(store(6, 5, 0));
    code.extend([0u16; 2000]);
    code.push(halt());
    let bytes: Vec<_> = code.iter().flat_map(|w| w.to_le_bytes()).collect();
    let source: Vec<_> = (0..4096).map(|i| (i * 7) as u8).collect();
    let package = pack_application_resources(
        &bytes,
        2,
        &[
            BootResource::copy(0x100000, image.storage),
            BootResource::copy(
                0x110000,
                memcpy_params(0x120000, 0x180000, 4096).unwrap().to_vec(),
            ),
            BootResource::copy(0x120000, source.clone()),
        ],
    )
    .unwrap();
    let mut observed = System::from_preloaded_cold(&package, 0xa55a).unwrap();
    let mut plain = System::from_preloaded_cold(&package, 0xa55a).unwrap();
    publish(&mut observed, 65535);
    publish(&mut plain, 65535);
    let started = std::time::Instant::now();
    // Reuse qualified real cold preparation; no shortened clock/startup state.
    while !observed.application_ready() {
        assert!(observed.clock().logic_edges < 1_000_000 && started.elapsed().as_secs() < 180);
        let a = observed.step().unwrap();
        let b = plain.step().unwrap();
        assert_eq!(a.cpu, b.cpu);
        assert_eq!(a.native, b.native);
    }
    let prefix = observed.clock().logic_edges;
    let expected_cold_load = observed.backing()[0x2110];
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ordinary-runner-short");
    std::fs::create_dir_all(&root).unwrap();
    let folder = root.join(format!("actual-{}", std::process::id()));
    let mut recorder =
        Recorder::create_runner(&folder, &package, observed.clock().peek().ticks, 10000).unwrap();
    recorder.initialize_memory(&observed).unwrap();
    let mut events = Vec::new();
    let mut watched = 0;
    let mut pressed = false;
    let mut released = false;
    let mut halted = false;
    let mut arrivals = runner::ExecuteArrival::default();
    let mut prefix_arrivals = Vec::new();
    let mut consumer_arrivals = Vec::new();
    let mut old_rule_prefix_hits = 0;
    let mut memory_retirement = false;
    let app = std::time::Instant::now();
    for _ in 0..100000 {
        assert!(observed.clock().logic_edges - prefix < 20000 && started.elapsed().as_secs() < 180);
        let mut device = None;
        let mut executing = None;
        let mut arrival = None;
        let mut retired_before = 0;
        let a = observed
            .step_observing(|o| {
                recorder.observe(o);
                device = runner::cpu_device(o);
                executing = o.executing_pc;
                arrival = arrivals.observe(o);
                retired_before = o.cpu.map_or(0, |c| c.core.retired_words);
                if o.cpu.is_some_and(|c| c.system.watch_read_enable) {
                    watched += 1;
                }
            })
            .unwrap();
        let b = plain.step().unwrap();
        recorder.after_step(&a, &observed);
        if a.instant.logic_rise {
            let after = observed.core().retired_words() as u64;
            if executing == Some((7, prefix_site)) && after > retired_before {
                old_rule_prefix_hits += 1;
            }
            if arrival == Some((7, prefix_site)) {
                prefix_arrivals
                    .push(json!({"tick":a.instant.ticks,"before":retired_before,"after":after}));
            }
            if arrival == Some((7, prefix_site + 1)) {
                consumer_arrivals
                    .push(json!({"tick":a.instant.ticks,"before":retired_before,"after":after}));
            }
            if executing.is_none() && after > retired_before {
                memory_retirement = true;
            }
        }
        assert_eq!(a.instant, b.instant);
        assert_eq!(a.cpu, b.cpu);
        assert_eq!(a.display, b.display);
        assert_eq!(a.native, b.native);
        if watched == 16 && !pressed {
            publish(&mut observed, 0xffdf);
            publish(&mut plain, 0xffdf);
            pressed = true;
        }
        if let Some(e) = device {
            if e["device"] == 5 && e["channel"] == 8 && e["write"] == true {
                publish(&mut observed, 0xffff);
                publish(&mut plain, 0xffff);
                released = true;
            }
            events.push(e);
        }
        // Pure inspection even while cache/store is active cannot alter parity.
        assert_eq!(
            observed.inspect_cpu_word(0x2000),
            plain.inspect_cpu_word(0x2000)
        );
        assert!(observed.inspect_cpu_word(4194304).is_none());
        if a.cpu.is_some_and(|c| c.halted) && observed.statistics.gpu_completed == 1 {
            halted = true;
            break;
        }
    }
    assert!(halted && pressed && released && watched >= 16);
    assert_eq!(old_rule_prefix_hits, 0);
    assert_eq!(prefix_arrivals.len(), 1);
    assert_eq!(consumer_arrivals.len(), 1);
    assert_eq!(prefix_arrivals[0]["before"], prefix_arrivals[0]["after"]);
    assert_eq!(
        consumer_arrivals[0]["before"],
        consumer_arrivals[0]["after"]
    );
    assert!(memory_retirement);
    assert_eq!(observed.core().architectural_gprs()[7], expected_cold_load);
    assert_eq!(observed.backing(), plain.backing());
    assert_eq!(
        observed.core().architectural_gprs(),
        plain.core().architectural_gprs()
    );
    assert_eq!(observed.inspect_cpu_word(0x2000), Some(0xabcd));
    let copied: Vec<_> = observed.backing()[0xc0000..0xc0800]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    assert_eq!(copied, source);
    assert_eq!(
        events
            .iter()
            .filter(|e| e["device"] == 0 && e["channel"] == 11)
            .count(),
        3
    );
    let input: Vec<_> = events
        .iter()
        .filter(|e| e["device"] == 5 && e["write"] == false)
        .map(|e| e["value"].as_u64().unwrap())
        .collect();
    assert_eq!(input, vec![223, 255]);
    let clear = events
        .iter()
        .find(|e| e["device"] == 0 && e["channel"] == 11 && e["write"] == true)
        .unwrap()["tick"]
        .as_u64()
        .unwrap();
    for e in events
        .iter()
        .filter(|e| e["device"] == 0 && e["channel"] == 11 && e["write"] == false)
    {
        let delta = (e["tick"].as_u64().unwrap() - clear) / 148;
        assert_eq!(e["value"].as_u64(), Some((delta - 1) / 54));
    }
    let runtime_logic = observed.clock().logic_edges - prefix;
    let host = app.elapsed().as_secs_f64();
    recorder
        .finish(
            json!({"reached":true,"entry":"preloaded-post-boot","prefix_logic":prefix,
        "host_step_seconds":host,"runtime_logic":runtime_logic,"stop_reason":"short-fixture-halt"}),
            &observed,
        )
        .unwrap();
    std::fs::write(
        folder.join("short-device-events.json"),
        serde_json::to_vec_pretty(&events).unwrap(),
    )
    .unwrap();
    std::fs::write(folder.join("execute-site-proof.json"),serde_json::to_vec_pretty(&json!({
        "prefix_pc":[7,prefix_site],"prefix_arrivals":prefix_arrivals,"consumer_arrivals":consumer_arrivals,
        "old_retirement_rule_prefix_hits":old_rule_prefix_hits,"retirement_with_no_execute_pc":memory_retirement})).unwrap()).unwrap();
    eprintln!("short every-edge parity app_logic={runtime_logic} app_host={host:.6} watch_edges={watched} folder={}",folder.display());
}
