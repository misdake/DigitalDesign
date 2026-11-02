//! Bounded production-command transport checks, not a whole-GPU executable.
#[path = "support/vliw_cycle/mod.rs"]
mod support;

use gpu_v2::system::vliw_cycle::{command_dma::Work, Input, Response, Transport};

#[test]
fn two_jobs_return_owners_ce_pause_and_payload_terminal_are_independent() {
    let run = support::synthetic();
    assert!(run.stats.max_dma_returns == 2 && run.stats.max_ro_owners == 2);
    assert!(run.stats.ce0_writes > 10 && run.stats.held_requests > 10);
    assert!(run.stats.delayed_write_terminals > 0 && run.stats.overlapped_write_payload > 0);
    assert_eq!(run.stats.work_done, 5);
    assert_eq!(run.stats.cache_done, 3);
    assert_eq!(run.stats.texture_done, 4);
    assert_eq!(run.stats.fb_done, 8);
    assert!(run.stats.uniform_done > 0 && run.stats.mesh_done == 2);
    assert!(run.reset_while_active);
}

#[test]
fn production_contract_errors_stop_without_inventing_success() {
    let mut t = Transport::default();
    t.clock(Input {
        reset: true,
        ..Default::default()
    })
    .unwrap();
    let unowned = Input {
        physical: [
            gpu_v2::system::vliw_cycle::Physical {
                response: Response {
                    valid: true,
                    last: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            Default::default(),
            Default::default(),
        ],
        ..Default::default()
    };
    assert!(t.clock(unowned).unwrap_err().rule.contains("unowned"));
    assert!(!t.outputs(Input::default()).router.busy);
    let bad = Input {
        ce: true,
        work: Work {
            valid: true,
            source_halfword: 3,
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(t.clock(bad).unwrap_err().rule.contains("alignment"));
    assert_eq!(t.dma.occupancy(), (0, 0));
    assert!(support::stop_trace().len() > 10);
}

#[test]
fn production_transport_runs_against_one_native_arbiter_endpoint_gear_mc() {
    let run = support::native();
    assert_eq!(run.stats.fb_done, 8);
    assert!(run.stats.ce0_writes > 0 && run.native_cycles > 0);
    assert_eq!(run.stats.work_done, 5);
    assert_eq!(run.stats.cache_done, 3);
    assert_eq!(run.stats.texture_done, 4);
    assert!(run.backing_write_logic_edges > 0);
}

#[test]
fn simultaneous_accept_return_and_last_payload_ack_use_old_owners() {
    assert_eq!(support::simultaneous_trace().len(), 38);
}

#[test]
#[ignore = "short Icarus production RTL differential and actual DUT negative controls"]
fn production_rtl_matches_every_sample_and_rejects_real_mutations() {
    let root = support::evidence_root();
    let synthetic = support::synthetic();
    support::rtl(&root, "synthetic", &synthetic.trace, None, true, false);
    let native = support::native();
    support::rtl(&root, "native", &native.trace, None, true, false);
    support::rtl(
        &root,
        "stop-drain",
        &support::stop_trace(),
        None,
        true,
        false,
    );
    for mutation in [
        "ce_drop",
        "wrong_owner",
        "ignore_ready",
        "early_write_terminal",
    ] {
        support::rtl(
            &root,
            mutation,
            &synthetic.trace,
            Some(mutation),
            false,
            false,
        );
    }
    for (label, trace) in support::invalid_peers(&synthetic.trace) {
        support::rtl(&root, label, &trace, None, false, true);
    }
    support::rtl(
        &root,
        "simultaneous",
        &support::simultaneous_trace(),
        None,
        true,
        false,
    );
    support::receipt(&root, &synthetic, &native);
    println!("P1A QUALIFIED {}", root.display());
}

#[test]
#[ignore = "bounded host fixture throughput; includes native initialization, checks and in-memory trace"]
fn bounded_native_fixture_host_throughput() {
    use std::{fmt::Write, time::Instant};
    let root = support::evidence_root();
    let mut samples = String::new();
    for n in 0..4 {
        let begin = Instant::now();
        let run = support::native();
        let seconds = begin.elapsed().as_secs_f64();
        if n != 0 {
            samples.push(',');
        }
        write!(samples,"{{\"sample\":{n},\"logic_edges\":{},\"native_cycles\":{},\"seconds\":{seconds},\"logic_edges_per_second\":{}}}",run.trace.len(),run.native_cycles,run.trace.len() as f64/seconds).unwrap();
    }
    let text=format!("{{\"scope\":\"GPU transport/native fixture, not whole GPU or frame speed\",\"boundary\":\"Includes native initialization, actor assertions, in-memory pin trace and arbiter settling; excludes build, RTL processes and export\",\"concurrent_host_load\":\"uncontrolled\",\"rss\":\"unmeasured\",\"samples\":[{samples}]}}\n");
    std::fs::write(root.join("host-throughput.json"), &text).unwrap();
    println!("P1A HOST {} {text}", root.display());
}
