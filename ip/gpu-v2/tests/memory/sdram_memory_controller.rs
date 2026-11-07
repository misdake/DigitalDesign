use digital_design_hardware_gowin::sdram_memory_controller::{
    ports::*,
    sim::oracle::{Config, Memory},
    RtlSources,
};
fn run(service: &mut impl Service, max: usize) -> Vec<Event> {
    let mut events = vec![];
    for _ in 0..max {
        events.extend(service.step().unwrap());
        if service.idle() {
            return events;
        }
    }
    panic!("SDRAM test watchdog");
}
#[test]
fn protected_payload_constant_and_host_sources_are_explicit() {
    assert_eq!(OracleWord::constant::<0x1234>().origin(), Origin::Constant);
    // SAFETY: this value is external stimulus. Naming it documents that input boundary.
    let value = unsafe { OracleWord::from_host(1234, "test stimulus").unwrap() };
    assert_eq!(value.bits(), 1234);
    assert_eq!(value.origin(), Origin::Host("test stimulus"));
    // SAFETY: exercising the independently checked invalid source path.
    assert!(unsafe { OracleWord::from_host(1, "") }.is_err());
}
#[test]
fn masked_writes_and_following_reads_match_independent_byte_scoreboard() {
    for bytes in BURST_BYTES {
        let mut service = Memory::new(
            OracleImage::filled::<0xa7>(0, 1024).unwrap(),
            Default::default(),
        )
        .unwrap();
        let data = vec![OracleWord::constant::<0x8877665544332211>(); bytes / 8];
        service
            .submit(
                Client::FramebufferWrite,
                Request::Write {
                    address: 0,
                    data,
                    enables: vec![0x55; bytes / 8],
                },
            )
            .unwrap();
        service
            .submit(Client::FramebufferRead, Request::Read { address: 0, bytes })
            .unwrap();
        let e = run(&mut service, 1000);
        let expected: Vec<_> = (0..bytes)
            .map(|i| {
                if i % 2 == 0 {
                    [0x11, 0x33, 0x55, 0x77][(i % 8) / 2]
                } else {
                    0xa7
                }
            })
            .collect();
        assert_eq!(&service.bytes()[..bytes], expected);
        assert!(service.bytes()[bytes..].iter().all(|&b| b == 0xa7));
        let returned: Vec<_> = e
            .into_iter()
            .filter_map(|e| {
                if let Event::ReadBeat { data, .. } = e {
                    Some(data.to_le_bytes())
                } else {
                    None
                }
            })
            .flatten()
            .collect();
        assert_eq!(returned, expected);
    }
}
#[test]
fn rejected_data_requests_preserve_ids_image_and_bounded_work() {
    let mut memory = Memory::new(
        OracleImage::filled::<0x73>(128, 128).unwrap(),
        Config {
            max_cycles: 4,
            max_requests: 1,
            ..Default::default()
        },
    )
    .unwrap();
    for (address, bytes) in [
        (0, 32),
        (128, 16),
        (136, 32),
        (256, 32),
        (u64::MAX - 31, 32),
    ] {
        assert!(memory
            .submit(Client::GpuReadOnly, Request::Read { address, bytes })
            .is_err());
    }
    assert!(memory
        .submit(
            Client::FramebufferWrite,
            Request::Write {
                address: 128,
                data: vec![OracleWord::constant::<0>(); 4],
                enables: vec![255; 3]
            }
        )
        .is_err());
    assert_eq!(
        memory
            .submit(
                Client::GpuReadOnly,
                Request::Read {
                    address: 128,
                    bytes: 32
                }
            )
            .unwrap(),
        0
    );
    assert!(memory
        .submit(
            Client::GpuReadOnly,
            Request::Read {
                address: 128,
                bytes: 32
            }
        )
        .is_err());
    for _ in 0..4 {
        memory.step().unwrap();
    }
    assert!(memory.step().is_err());
    assert_eq!(memory.bytes(), &[0x73; 128]);
}

#[test]
fn stream_rtl_bundle_has_only_the_current_endpoint_and_registered_cl3_return() {
    assert!(
        RtlSources::STREAM_GEARBOX.contains("CAS=3, RCD_CYCLES=3, RP_CYCLES=3, CAPTURE_LATENCY=3")
    );
    assert!(RtlSources::STREAM_GEARBOX.contains("REGISTER_READ_RETURN=1, DQ_INPUT_DELAY_TAPS=32"));
    assert!(RtlSources::STREAM_GEARBOX.contains(".CONTINUE(1),.NEXT_TAGGED(1)"));
    assert!(RtlSources::CONTROLLER.contains("module SdramController"));
    assert!(RtlSources::STREAM_SHARED_PORT.contains("Ordered two-descriptor endpoint"));
    assert!(!RtlSources::STREAM_SHARED_PORT.contains("parameter EARLY_GRANT"));
}

#[test]
fn public_payload_boundary_rejects_safe_host_injection_and_forged_fields() {
    use std::{
        fs,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let vendor = fs::read_dir(&deps)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("libdigital_design_hardware_gowin-")
                && entry.path().extension().is_some_and(|e| e == "rlib")
        })
        .max_by_key(|entry| entry.metadata().unwrap().modified().unwrap())
        .unwrap()
        .path();
    let out = deps
        .parent()
        .unwrap()
        .join(format!("sdram-api-check-{}", std::process::id()));
    fs::create_dir_all(&out).unwrap();
    for (name, body, diagnostic) in [
        ("constant", "let _ = OracleWord::constant::<37>();", None),
        (
            "host",
            "let _ = OracleWord::from_host(37, \"input\");",
            Some("E0133"),
        ),
        (
            "runtime",
            "let n = 37_u64; let _ = OracleWord::constant::<n>();",
            Some("E0435"),
        ),
        (
            "fields",
            "let _ = OracleWord { bits: 37, origin: Origin::Constant };",
            Some("E0451"),
        ),
    ] {
        let source = out.join(format!("{name}.rs"));
        fs::write(&source,format!("use digital_design_hardware_gowin::sdram_memory_controller::ports::*; fn main() {{ {body} }}")).unwrap();
        let stderr = fs::File::create(out.join(format!("{name}.stderr"))).unwrap();
        let mut child = Command::new("rustc")
            .arg("--edition=2021")
            .arg("--crate-type=lib")
            .arg(&source)
            .arg("--emit=metadata")
            .arg("-o")
            .arg(out.join(format!("{name}.rmeta")))
            .arg("--extern")
            .arg(format!(
                "digital_design_hardware_gowin={}",
                vendor.display()
            ))
            .arg("-L")
            .arg(format!("dependency={}", deps.display()))
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn()
            .unwrap();
        let start = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if start.elapsed() > Duration::from_secs(20) {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("API compiler watchdog");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let errors = fs::read_to_string(out.join(format!("{name}.stderr"))).unwrap();
        if let Some(code) = diagnostic {
            assert!(
                !status.success() && errors.contains(code),
                "{name}: {errors}"
            );
        } else {
            assert!(status.success(), "{name}: {errors}");
        }
    }
}
