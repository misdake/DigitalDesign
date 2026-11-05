use cpu_v3::{device_send, halt, load_immediate16};
use cpu_v3_tang_nano_20k::{
    boot::{pack_application_resources, BootResource},
    system_cycle_emu::{profiling::Recorder, scanout::Scanout, System},
};
#[test]
fn passive_capture_preserves_every_edge_cpu_scanout_and_memory() {
    use gpu_v2::system::vliw_command::{Command, CommandBuffer, DeviceSubmission};
    let mut cb = CommandBuffer::new();
    for command in [
        Command::NOP,
        Command::wait(true, true),
        Command::FLUSH,
        Command::END,
    ] {
        cb.push(command);
    }
    let image = cb.image().unwrap();
    let submit = DeviceSubmission::new(0x100000, image.valid_bytes).unwrap();
    let mut code = load_immediate16(0, submit.count_minus_one).to_vec();
    code.push(device_send(0, 4, 0));
    code.extend(load_immediate16(0, submit.base_sector));
    code.push(device_send(0, 4, 1));
    code.extend(load_immediate16(0, 0x1837));
    code.push(device_send(0, 7, 1));
    code.push(halt());
    let bytes: Vec<_> = code.iter().flat_map(|w| w.to_le_bytes()).collect();
    let package =
        pack_application_resources(&bytes, 2, &[BootResource::copy(0x100000, image.storage)])
            .unwrap();
    let mut observed = System::from_preloaded_cold(&package, 0xa55a).unwrap();
    let mut plain = System::from_preloaded_cold(&package, 0xa55a).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/c4-profile");
    std::fs::create_dir_all(&root).unwrap();
    let folder = root.join(format!(
        "edge-ab-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    let mut r = Recorder::create(&folder, &package, 0, 10000, 2).unwrap();
    r.initialize_memory(&observed).unwrap();
    let limited_folder = folder.with_extension("limited");
    let mut limited = Recorder::create(&limited_folder, &package, 0, 3, 0).unwrap();
    let mut scan = Scanout::default();
    let mut halted = false;
    let started = std::time::Instant::now();
    while observed.clock().logic_edges < 1_000_000 {
        let a = observed
            .step_observing(|o| {
                r.observe(o);
                limited.observe(o);
            })
            .unwrap();
        let b = plain.step().unwrap();
        r.after_step(&a, &observed);
        limited.after_step(&a, &observed);
        assert_eq!(a.instant, b.instant);
        assert_eq!(a.cpu, b.cpu);
        assert_eq!(a.display, b.display);
        assert_eq!(a.native, b.native);
        if a.instant.pixel_rise {
            if let Some(frame) = scan.sample(a.display.video).unwrap() {
                r.frame(&frame, a.instant.ticks).unwrap();
            }
        }
        if a.cpu.is_some_and(|c| c.halted) && observed.statistics.gpu_completed == 1 {
            halted = true;
            break;
        }
        if a.instant.logic_rise && observed.clock().logic_edges.is_multiple_of(4096) {
            assert!(started.elapsed().as_secs() < 300);
        }
    }
    assert!(halted);
    assert_eq!(observed.backing(), plain.backing());
    assert_eq!(
        observed.core().architectural_gprs(),
        plain.core().architectural_gprs()
    );
    assert_eq!(
        format!("{:?}", observed.statistics),
        format!("{:?}", plain.statistics)
    );
    assert_eq!(observed.observation(1), Some(0x1837));
    assert_eq!(observed.statistics.gpu_submits, 1);
    assert_eq!(observed.statistics.gpu_completed, 1);
    limited
        .finish(
            serde_json::json!({"reached":true,"entry":"cold-preloaded"}),
            &observed,
        )
        .unwrap();
    let cap: serde_json::Value =
        serde_json::from_slice(&std::fs::read(limited_folder.join("capture.json")).unwrap())
            .unwrap();
    assert_eq!(cap["events"], 3);
    assert_eq!(cap["truncated"], true);
    assert_eq!(cap["end_tick"], observed.clock().peek().ticks);
    r.finish(serde_json::json!({"entry":"cold-preloaded","reached":true,"stop_reason":"cpu-halt","host_step_seconds":started.elapsed().as_secs_f64(),"host_export_seconds":0.0}),&observed).unwrap();
    let events: Vec<serde_json::Value> =
        std::fs::read_to_string(folder.join("system-events.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
    for kind in ["cb_submit", "cb_drain", "gpu_done"] {
        assert_eq!(events.iter().filter(|row| row[1] == kind).count(), 1);
    }
    for kind in ["cb_latch", "cb_execute", "cb_issue", "cb_release"] {
        assert_eq!(events.iter().filter(|row| row[1] == kind).count(), 4);
    }
    eprintln!(
        "A/B every scheduler instant through {} logic; artifacts {}",
        observed.clock().logic_edges,
        folder.display()
    );
}
