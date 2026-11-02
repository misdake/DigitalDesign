//! Command-control only: backend render/flush/source availability are peers.
#[path = "support/vliw_command/mod.rs"]
mod support;
#[allow(dead_code)]
#[path = "support/vliw_cycle/mod.rs"]
mod transport_support;
use gpu_v2::system::vliw_cycle::{
    command::{Device4, DeviceBus, Error},
    Physical, Response,
};

#[test]
fn native_cb_control_uses_dma_ram_old_q_and_resubmits_without_ack() {
    for long in [false, true] {
        let r = support::native(long, None);
        assert_eq!(r.stats.submits, 2);
        assert_eq!(r.stats.completed, 2);
        assert_eq!(r.stats.ce0_device_writes, 4);
        assert_eq!(r.stats.ce0_retirements, 2);
        assert_eq!(r.stats.commands, if long { 69 } else { 8 });
        assert!(
            r.stats.ce0_writes > 0
                && r.stats.ram_stalls > 0
                && r.stats.flush_stalls > 0
                && r.stats.end_stalls > 0
        );
        assert!(r.native_cycles > 0);
    }
}
#[test]
fn abort_drains_physical_owners_and_reset_allows_new_submission() {
    let r = support::native(false, Some("abort_reset"));
    assert_eq!(r.stats.aborts, 1);
    assert_eq!(r.stats.completed, 2);
    assert_eq!(r.stats.writes, 32);
}
#[test]
fn production_count_submit_encoding_and_end_contracts_reject_invalid_inputs() {
    for &case in support::CONTRACT_CASES {
        assert!(matches!(
            support::native(false, Some(case)).error,
            Some(Error::Contract(_))
        ));
    }
    assert!(matches!(
        support::native(false, Some("unsupported")).error,
        Some(Error::UnsupportedOpcode(9))
    ));
}
#[test]
fn device4_raw_transport_status_is_reset_only_and_reads_are_passive() {
    let mut d = Device4::default();
    let bus = DeviceBus {
        index: 4,
        channel: 2,
        read: true,
        ..Default::default()
    };
    d.clock(true, [Physical::default(); 3]);
    assert_eq!(d.read(bus, true), 1);
    let mut pins = [Physical::default(); 3];
    pins[1].response = Response {
        valid: true,
        error: true,
        ..Default::default()
    };
    d.clock(false, pins);
    for _ in 0..32 {
        assert_eq!(d.read(bus, true), 3);
        assert_eq!(d.read(bus, false), 2);
    }
    d.clock(false, [Physical::default(); 3]);
    assert_eq!(d.read(bus, false), 2);
    assert_eq!(d.read(DeviceBus { index: 3, ..bus }, true), 0);
    assert_eq!(d.read(DeviceBus { channel: 1, ..bus }, true), 0);
    d.clock(true, [Physical::default(); 3]);
    assert_eq!(d.read(bus, false), 0);
}
#[test]
#[ignore = "short production command/transport/RAM RTL differential and actual DUT mutations"]
fn original_production_command_cone_matches_and_detects_real_mutations() {
    let root = support::root();
    let short = support::native(false, None);
    let long = support::native(true, None);
    support::rtl(&root, "two-cb", &short, None, false);
    support::rtl(&root, "cache-wrap", &long, None, false);
    let recovery = support::native(false, Some("abort_reset"));
    support::rtl(&root, "abort-reset", &recovery, None, false);
    for mutation in ["ce_submit", "early_fetch", "ignore_flush", "early_idle"] {
        support::rtl(&root, mutation, &short, Some(mutation), false);
    }
    for &case in support::CONTRACT_CASES {
        let bad = support::native(false, Some(case));
        support::rtl(&root, case, &bad, None, true);
    }
    for mutation in [None, Some("read_clear"), Some("ignore_valid")] {
        support::status_rtl(&root, mutation);
    }
    std::fs::write(root.join("coverage.json"),format!("{{\"scope\":\"command-control; controlled render/flush/source availability, idle CPU/Display, no board request slots\",\"two_cb\":{},\"cache_wrap\":{},\"abort_reset\":{},\"positive_rtl_cases\":4,\"compiled_mutations_detected\":6,\"CP_contract_cases\":12,\"abort_statistics\":\"final reset epoch; trace and cycle counts include prefix\"}}\n",support::metrics(&short),support::metrics(&long),support::metrics(&recovery))).unwrap();
    println!("P1B QUALIFIED {}", root.display());
}

#[test]
#[ignore = "finite host fixture cost, not GPU throughput or physical cycle equivalence"]
fn bounded_command_fixture_host_cost() {
    use std::{fmt::Write, time::Instant};
    let root = support::root();
    let mut samples = String::new();
    for n in 0..4 {
        let begin = Instant::now();
        let run = support::native(false, None);
        let seconds = begin.elapsed().as_secs_f64();
        if n != 0 {
            samples.push(',');
        }
        write!(
            samples,
            "{{\"sample\":{n},\"seconds\":{seconds},\"logic_edges_per_second\":{},\"work\":{}}}",
            run.trace.len() as f64 / seconds,
            support::metrics(&run)
        )
        .unwrap();
    }
    let text=format!("{{\"scope\":\"bounded command-control fixture; not GPU/frame throughput\",\"boundary\":\"includes native initialization, peer scheduling, assertions, settling and in-memory traces; excludes build/RTL/export\",\"host_contention\":\"uncontrolled\",\"RSS\":\"unmeasured\",\"samples\":[{samples}]}}\n");
    std::fs::write(root.join("host-cost.json"), &text).unwrap();
    println!("P1B HOST {} {text}", root.display());
}
