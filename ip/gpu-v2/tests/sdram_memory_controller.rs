use digital_design_hardware_gowin::sdram_memory_controller::{
    ports::*,
    sim::{
        average::{Limits, Memory, Profile},
        calibration::{self, Sample},
        controller, oracle,
        traffic::*,
    },
    RtlSources,
};
fn burst(address: u64, bytes: usize, access: Access) -> Burst {
    Burst {
        address,
        bytes,
        access,
    }
}
fn profile() -> Profile {
    Profile {
        read_first: [8; 4],
        write_complete: [12, 16, 24, 80],
        read_sector_first: [8, 24, 40, 56],
        recovery: 1,
    }
}
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
fn image(bytes: Vec<u8>) -> OracleImage {
    // SAFETY: supplied bytes are independent test stimuli, not computed audited intermediates.
    unsafe { OracleImage::from_host(0, bytes, "integration test image").unwrap() }
}
#[test]
fn geometry_and_four_sector_native_command_goldens() {
    for (address, bank, row) in [
        (0, 0, 0),
        (128, 1, 0),
        (384, 3, 0),
        (512, 0, 0),
        (4096, 0, 1),
    ] {
        assert_eq!(calibration::bank_row(address), (bank, row));
    }
    let cfg = controller::Config {
        request_transport: 0,
        read_transport: 0,
        write_ack_transport: 0,
        ..Default::default()
    };
    let mut core = controller::Controller::new(cfg).unwrap();
    let group = burst(0, 512, Access::Read).sectors().unwrap();
    let r = core.serve(&group, 0, true, |_| true).unwrap();
    assert_eq!(
        r.sectors.iter().map(|s| s.launch_core).collect::<Vec<_>>(),
        [3, 35, 67, 99]
    );
    assert_eq!(r.done_core, 134); // first READ + 131, from CL2 diagram consumption edges.
    assert_eq!(
        r.sectors.iter().map(|s| s.bank).collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
    assert_eq!(
        r.sectors
            .iter()
            .skip(1)
            .map(|s| s.hidden_prepare_core)
            .collect::<Vec<_>>(),
        [2, 2, 2]
    );
    let mut core = controller::Controller::new(cfg).unwrap();
    let r = core
        .serve(
            &burst(0, 512, Access::Write).sectors().unwrap(),
            0,
            true,
            |_| true,
        )
        .unwrap();
    assert_eq!(
        r.sectors
            .iter()
            .map(|s| s.launch_core - r.sectors[0].launch_core)
            .collect::<Vec<_>>(),
        [0, 32, 64, 96]
    );
    assert_eq!(r.done_core - r.sectors[0].launch_core, 129);
}
#[test]
fn cancellation_and_same_bank_row_conflict_do_not_revoke_issued_sector() {
    let mut core = controller::Controller::new(Default::default()).unwrap();
    let group = burst(0, 512, Access::Read).sectors().unwrap();
    let r = core.serve(&group, 0, true, |edge| edge < 39).unwrap();
    assert_eq!(r.sectors.len(), 1);
    assert!(r.priority_break);
    assert_eq!(r.done_core, 42); // first sector READ7 + 35
    let mut core = controller::Controller::new(Default::default()).unwrap();
    let r = core
        .serve(
            &[burst(0, 128, Access::Read), burst(4096, 128, Access::Read)],
            0,
            true,
            |_| true,
        )
        .unwrap();
    assert_eq!(r.sectors.len(), 1);
    assert!(r.bank_break);
    let next = core
        .serve(&[burst(4096, 128, Access::Read)], 100, false, |_| true)
        .unwrap();
    assert_eq!(next.sectors[0].row_state, controller::RowState::Conflict);
    assert_eq!(next.sectors[0].launch_core - next.sectors[0].accept_core, 5);
}
#[test]
fn refresh_blocks_admission_and_bounds_chain_age() {
    let mut core = controller::Controller::new(controller::Config {
        refresh_interval: 40,
        continuation_age: 30,
        ..Default::default()
    })
    .unwrap();
    let r = core
        .serve(
            &burst(0, 512, Access::Read).sectors().unwrap(),
            0,
            true,
            |_| true,
        )
        .unwrap();
    assert!(r.refresh_break);
    assert_eq!(r.sectors.len(), 1);
    let r = core
        .serve(&[burst(128, 32, Access::Read)], 20, false, |_| true)
        .unwrap();
    assert!(r.refresh_wait_core > 0);
    assert_eq!(r.sectors[0].row_state, controller::RowState::Closed);
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
fn stable_average_reads_all_sizes_and_preserves_group_gaps() {
    let input: Vec<_> = (0..8192).map(|i| ((i * 17 + 31) & 255) as u8).collect();
    let mut service = Memory::new(image(input.clone()), profile(), Default::default()).unwrap();
    for bytes in BURST_BYTES {
        service
            .submit(Client::GpuReadOnly, Request::Read { address: 0, bytes })
            .unwrap();
    }
    let events = run(&mut service, 1000);
    let mut start = [0; 4];
    let mut beats = [0; 4];
    let mut complete = [0; 4];
    for e in events {
        match e {
            Event::Started { id, cycle, .. } => start[id as usize] = cycle,
            Event::ReadBeat {
                id,
                cycle,
                index,
                data,
                last,
            } => {
                assert_eq!(data.to_le_bytes(), input[index * 8..index * 8 + 8]);
                assert_eq!(data.origin(), Origin::Memory);
                assert_eq!(
                    cycle - start[id as usize],
                    profile()
                        .read_due(burst(0, BURST_BYTES[id as usize], Access::Read), index)
                        .unwrap()
                );
                assert_eq!(last, index + 1 == BURST_BYTES[id as usize] / 8);
                beats[id as usize] += 1;
            }
            Event::Complete { id, .. } => complete[id as usize] += 1,
        }
    }
    assert_eq!(beats, [4, 8, 16, 64]);
    assert_eq!(complete, [1; 4]);
    let mut bad = profile();
    bad.read_sector_first = [8, 25, 41, 60];
    let mut service = Memory::new(image(input), bad, Default::default()).unwrap();
    service
        .submit(
            Client::FramebufferRead,
            Request::Read {
                address: 0,
                bytes: 512,
            },
        )
        .unwrap();
    let e = run(&mut service, 100);
    let cycles: Vec<_> = e
        .iter()
        .filter_map(|e| {
            if let Event::ReadBeat { cycle, .. } = e {
                Some(*cycle)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(cycles[16] - cycles[15], 2);
    assert_eq!(cycles[48] - cycles[47], 4);
}
#[test]
fn masked_writes_and_following_reads_match_independent_byte_scoreboard() {
    for bytes in BURST_BYTES {
        let mut service = Memory::new(
            OracleImage::filled::<0xa7>(0, 1024).unwrap(),
            profile(),
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
fn rejected_requests_do_not_consume_ids_or_modify_data_and_limits_stop() {
    let mut service = Memory::new(
        OracleImage::filled::<0x73>(128, 128).unwrap(),
        profile(),
        Limits {
            max_cycles: 10,
            max_requests: 1,
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
        assert!(service
            .submit(Client::GpuReadOnly, Request::Read { address, bytes })
            .is_err());
    }
    assert!(service
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
        service
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
    assert!(service.step().is_err());
    assert_eq!(service.cycle(), 0);
    assert_eq!(service.bytes(), &[0x73; 128]);
}
#[test]
fn configured_load_is_reproducible_and_interrupted_groups_return_exactly_once() {
    let initial: Vec<_> = (0..2048).map(|i| ((i * 39 + 9) & 255) as u8).collect();
    let mut load = Load::display_and_cpu(1);
    load.streams[0].period = 30;
    load.streams[0].phase = 22;
    let config = oracle::Config {
        load,
        chain: ChainPolicy::ChainedCandidate,
        ..Default::default()
    };
    let replay = || {
        let mut service = oracle::Memory::new(image(initial.clone()), config.clone()).unwrap();
        service
            .submit(
                Client::FramebufferRead,
                Request::Read {
                    address: 0,
                    bytes: 512,
                },
            )
            .unwrap();
        let events = run(&mut service, 1000);
        assert!(service.background_requests() > 0);
        assert!(service.runs.iter().any(|r| r.priority_break));
        let data: Vec<_> = events
            .iter()
            .filter_map(|e| {
                if let Event::ReadBeat { data, .. } = e {
                    Some(data.to_le_bytes())
                } else {
                    None
                }
            })
            .flatten()
            .collect();
        assert_eq!(data, initial[..512]);
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, Event::Started { .. }))
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, Event::Complete { .. }))
                .count(),
            1
        );
        events
    };
    assert_eq!(replay(), replay());
    assert!(oracle::Memory::new(
        OracleImage::filled::<0>(4 * 1024 * 1024, 512).unwrap(),
        config
    )
    .is_err());
}
#[test]
fn calibration_weights_background_and_chain_effects_and_rounds_means_once() {
    let trace = calibration::representative_trace(64).unwrap();
    let solo = calibration::analyze(&trace, Default::default()).unwrap();
    let fixed = Profile::from_calibration(&solo).unwrap();
    for i in 0..8 {
        assert_eq!(solo.classes[i].samples, 64);
        let mean = if i < 4 {
            solo.classes[i].mean_first()
        } else {
            solo.classes[i].mean_complete()
        };
        let rounded = if i < 4 {
            fixed.read_first[i]
        } else {
            fixed.write_complete[i - 4]
        } as f64;
        assert!(rounded >= mean && rounded - mean < 1.0);
    }
    let again = calibration::analyze(&trace, Default::default()).unwrap();
    assert_eq!(fixed, Profile::from_calibration(&again).unwrap());
    let loaded = calibration::analyze(
        &trace,
        calibration::Config {
            load: Load::display_and_cpu(1),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(loaded.background_requests > 0);
    assert!(loaded.classes.iter().any(|s| s.sum_arbiter > 0));
    let single = [Sample {
        arrival: 0,
        client: Client::FramebufferRead,
        burst: burst(0, 512, Access::Read),
    }];
    let plain = calibration::analyze(&single, Default::default()).unwrap();
    let chained = calibration::analyze(
        &single,
        calibration::Config {
            chain: ChainPolicy::ChainedCandidate,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(plain.classes[3].sum_complete, 91); // four completions at 22,45,68,91
    assert_eq!(chained.classes[3].sum_complete, 70);
    assert_eq!(chained.classes[3].chained_sectors, 3);
    assert!(Profile::from_calibration(&chained).is_err()); // no implicit fallback for empty buckets
}
#[test]
fn background_overload_and_invalid_calibration_fail_with_bounded_work() {
    let trace = calibration::representative_trace(4).unwrap();
    let mut load = Load::display(1);
    load.streams[0].period = 1;
    assert!(calibration::analyze(
        &trace,
        calibration::Config {
            load,
            max_samples: 32,
            max_cycles: 1000,
            ..Default::default()
        }
    )
    .is_err());
    assert!(calibration::analyze(
        &trace,
        calibration::Config {
            max_cycles: 10,
            ..Default::default()
        }
    )
    .is_err());
    assert!(calibration::analyze(
        &[
            Sample {
                arrival: 1,
                client: Client::GpuReadOnly,
                burst: burst(0, 32, Access::Read)
            },
            Sample {
                arrival: 0,
                client: Client::GpuReadOnly,
                burst: burst(0, 32, Access::Read)
            }
        ],
        Default::default()
    )
    .is_err());
}
#[test]
fn migrated_rtl_bundle_preserves_existing_bridge_chain_default() {
    assert!(RtlSources::GEARBOX.contains(".next_valid(1'b0)"));
    assert!(RtlSources::CONTROLLER.contains("module SdramController"));
    assert!(RtlSources::SHARED_PORT.contains("module SharedSdramPort"));
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
