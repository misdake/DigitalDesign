use cpu_v3::{device_send, halt, load_immediate16};
use cpu_v3_tang_nano_20k::{
    boot::pack_single_application,
    system_cycle_emu::{profiling::Recorder, scanout::Scanout, System},
};
#[test]
fn passive_capture_preserves_every_edge_cpu_scanout_and_memory() {
    let mut code = load_immediate16(0, 0x1837).to_vec();
    code.push(device_send(0, 7, 1));
    code.push(halt());
    let bytes: Vec<_> = code.iter().flat_map(|w| w.to_le_bytes()).collect();
    let package = pack_single_application(&bytes, 2).unwrap();
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
        if a.cpu.is_some_and(|c| c.halted) {
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
    eprintln!(
        "A/B every scheduler instant through {} logic; artifacts {}",
        observed.clock().logic_edges,
        folder.display()
    );
}
