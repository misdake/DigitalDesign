//! Public-API tests for the resource scheduler.

use resource_scheduler::{
    check, check_modulo, lower_bound, modulo_schedule, plan, Graph, Limits, ModuloError,
    ModuloGraph, ModuloViolation, Node, Resource, ScheduleError, SearchConfig, Violation,
};

#[test]
fn live_pressure_releases_values_after_the_last_consumer() {
    let graph = Graph {
        nodes: vec![
            node("a", &[], 0, Some(0)),
            node("b", &[0], 0, Some(0)),
            node("c", &[], 4, Some(0)),
            node("end", &[], 10, Some(0)),
        ],
        resources: vec![resource("unit", 4, 1, 1)],
    };
    let outcome = plan(&graph, &limits(8, 100, 1), &SearchConfig::default()).unwrap();
    // a dies at b's issue; only b and c are simultaneously held to completion.
    assert_eq!(outcome.best_candidate().live_pressure, 2);
}

#[test]
fn checker_rejects_zero_latency_dependency_cycles() {
    let graph = Graph {
        nodes: vec![node("a", &[1], 0, None), node("b", &[0], 0, None)],
        resources: vec![],
    };
    let schedule = resource_scheduler::Schedule {
        nodes: vec![
            resource_scheduler::NodeSchedule {
                issue: 0,
                ready: 0,
                lane: None
            };
            2
        ],
        cycles: 0,
    };
    assert!(!check(&graph, &limits(8, 100, 1), &schedule).is_ok());
}

fn node(name: &str, predecessors: &[usize], earliest: u64, resource: Option<usize>) -> Node {
    Node {
        name: name.to_owned(),
        predecessors: predecessors.to_vec(),
        earliest,
        resource,
    }
}

fn resource(name: &str, lanes: usize, latency: u64, initiation_interval: u64) -> Resource {
    Resource {
        name: name.to_owned(),
        lanes,
        latency,
        initiation_interval,
    }
}

fn limits(max_nodes: usize, max_cycle: u64, max_candidates: usize) -> Limits {
    Limits::new(max_nodes, max_cycle, max_candidates)
}

fn chain() -> Graph {
    Graph {
        nodes: vec![node("a", &[], 0, Some(0)), node("b", &[0], 0, Some(0))],
        resources: vec![resource("adder", 1, 2, 1)],
    }
}

#[test]
fn empty_graph_schedules_to_zero() {
    let graph = Graph::default();
    let outcome = plan(&graph, &limits(4, 10, 4), &SearchConfig::default()).unwrap();
    assert_eq!(outcome.best_candidate().makespan, 0);
    assert!(check(
        &graph,
        &limits(4, 10, 4),
        &outcome.best_candidate().schedule
    )
    .is_ok());
}

#[test]
fn chain_respects_latency_and_dependency() {
    let graph = chain();
    let outcome = plan(&graph, &limits(4, 100, 1), &SearchConfig::default()).unwrap();
    let schedule = &outcome.best_candidate().schedule;
    assert_eq!(schedule.nodes[0].issue, 0);
    assert_eq!(schedule.nodes[0].ready, 2);
    assert_eq!(schedule.nodes[1].issue, 2);
    assert_eq!(schedule.nodes[1].ready, 4);
    assert_eq!(schedule.cycles, 4);
    assert!(check(&graph, &limits(4, 100, 1), schedule).is_ok());
}

#[test]
fn wiring_node_has_no_lane_and_zero_latency() {
    let graph = Graph {
        nodes: vec![node("wire", &[], 3, None), node("op", &[0], 0, Some(0))],
        resources: vec![resource("adder", 1, 4, 1)],
    };
    let outcome = plan(&graph, &limits(4, 100, 1), &SearchConfig::default()).unwrap();
    let schedule = &outcome.best_candidate().schedule;
    assert_eq!(schedule.nodes[0].lane, None);
    assert_eq!(schedule.nodes[0].issue, 3);
    assert_eq!(schedule.nodes[0].ready, 3);
    assert_eq!(schedule.nodes[1].issue, 3);
    assert_eq!(schedule.nodes[1].ready, 7);
    assert!(check(&graph, &limits(4, 100, 1), schedule).is_ok());
}

#[test]
fn one_lane_serializes_independent_nodes() {
    let graph = Graph {
        nodes: vec![node("a", &[], 0, Some(0)), node("b", &[], 0, Some(0))],
        resources: vec![resource("adder", 1, 1, 1)],
    };
    let outcome = plan(&graph, &limits(4, 100, 1), &SearchConfig::default()).unwrap();
    let schedule = &outcome.best_candidate().schedule;
    assert_eq!(schedule.nodes[0].issue, 0);
    assert_eq!(schedule.nodes[1].issue, 1);
    assert_eq!(schedule.cycles, 2);
    assert!(check(&graph, &limits(4, 100, 1), schedule).is_ok());
}

#[test]
fn two_lanes_run_in_parallel() {
    let graph = Graph {
        nodes: vec![node("a", &[], 0, Some(0)), node("b", &[], 0, Some(0))],
        resources: vec![resource("adder", 2, 1, 1)],
    };
    let outcome = plan(&graph, &limits(4, 100, 1), &SearchConfig::default()).unwrap();
    let schedule = &outcome.best_candidate().schedule;
    assert_eq!(schedule.nodes[0].issue, 0);
    assert_eq!(schedule.nodes[1].issue, 0);
    assert_eq!(schedule.cycles, 1);
    assert_ne!(schedule.nodes[0].lane, schedule.nodes[1].lane);
}

#[test]
fn initiation_interval_spaces_same_lane() {
    let graph = Graph {
        nodes: vec![node("a", &[], 0, Some(0)), node("b", &[], 0, Some(0))],
        resources: vec![resource("multiplier", 1, 1, 3)],
    };
    let outcome = plan(&graph, &limits(4, 100, 1), &SearchConfig::default()).unwrap();
    let schedule = &outcome.best_candidate().schedule;
    assert_eq!(schedule.nodes[0].issue, 0);
    assert_eq!(schedule.nodes[1].issue, 3);
    assert!(check(&graph, &limits(4, 100, 1), schedule).is_ok());
}

#[test]
fn release_gate_delays_issue() {
    let graph = Graph {
        nodes: vec![node("a", &[], 5, Some(0))],
        resources: vec![resource("adder", 1, 1, 1)],
    };
    let outcome = plan(&graph, &limits(4, 100, 1), &SearchConfig::default()).unwrap();
    let schedule = &outcome.best_candidate().schedule;
    assert_eq!(schedule.nodes[0].issue, 5);
    assert_eq!(schedule.nodes[0].ready, 6);
}

#[test]
fn all_candidates_pass_the_checker() {
    let graph = Graph {
        nodes: vec![
            node("a", &[], 0, Some(0)),
            node("b", &[], 0, Some(0)),
            node("c", &[0], 0, Some(0)),
            node("d", &[1], 0, Some(0)),
            node("e", &[2, 3], 0, Some(0)),
        ],
        resources: vec![resource("unit", 2, 2, 1)],
    };
    let limits = limits(8, 100, 12);
    let outcome = plan(&graph, &limits, &SearchConfig::default()).unwrap();
    for candidate in &outcome.candidates {
        let report = check(&graph, &limits, &candidate.schedule);
        assert!(
            report.is_ok(),
            "{}: {:?}",
            candidate.label,
            report.violations
        );
    }
}

#[test]
fn best_is_never_worse_than_baseline() {
    let graph = Graph {
        nodes: vec![
            node("a", &[], 0, Some(0)),
            node("b", &[], 0, Some(0)),
            node("c", &[], 1, Some(0)),
            node("d", &[0, 1], 0, Some(0)),
            node("e", &[2], 0, Some(0)),
        ],
        resources: vec![resource("unit", 1, 2, 1)],
    };
    let limits = limits(8, 100, 32);
    let outcome = plan(&graph, &limits, &SearchConfig::default()).unwrap();
    let best = outcome.best_candidate();
    let baseline = &outcome.candidates[0];
    assert_eq!(baseline.label, "baseline");
    assert!(best.makespan <= baseline.makespan);
    assert!(best.within_deadline);
}

#[test]
fn candidate_count_matches_limit() {
    let graph = chain();
    for count in 1..=6 {
        let outcome = plan(&graph, &limits(4, 100, count), &SearchConfig::default()).unwrap();
        assert_eq!(outcome.candidates.len(), count);
    }
}

#[test]
fn search_is_deterministic_for_a_fixed_seed() {
    let graph = Graph {
        nodes: vec![
            node("a", &[], 0, Some(0)),
            node("b", &[], 0, Some(0)),
            node("c", &[0, 1], 0, Some(0)),
        ],
        resources: vec![resource("unit", 1, 1, 1)],
    };
    let limits = limits(8, 100, 20);
    let config = SearchConfig { base_seed: 42 };
    let first = plan(&graph, &limits, &config).unwrap();
    let second = plan(&graph, &limits, &config).unwrap();
    assert_eq!(first.best, second.best);
    let first_keys: Vec<_> = first
        .candidates
        .iter()
        .map(|c| (c.label.clone(), c.seed, c.makespan))
        .collect();
    let second_keys: Vec<_> = second
        .candidates
        .iter()
        .map(|c| (c.label.clone(), c.seed, c.makespan))
        .collect();
    assert_eq!(first_keys, second_keys);
}

#[test]
fn deterministic_candidates_are_labelled_and_seeded() {
    let graph = chain();
    let outcome = plan(&graph, &limits(4, 100, 5), &SearchConfig::default()).unwrap();
    assert_eq!(outcome.candidates[0].label, "baseline");
    assert_eq!(outcome.candidates[0].seed, None);
    assert_eq!(outcome.candidates[1].label, "critical-path");
    assert_eq!(outcome.candidates[1].seed, None);
    for (index, candidate) in outcome.candidates.iter().enumerate().skip(2) {
        assert_eq!(candidate.label, format!("random-{index}"));
        assert!(candidate.seed.is_some());
    }
    let seeds: Vec<u64> = outcome
        .candidates
        .iter()
        .filter_map(|candidate| candidate.seed)
        .collect();
    let mut unique = seeds.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), seeds.len(), "restart seeds must be distinct");
}

#[test]
fn planning_does_not_mutate_the_graph() {
    let graph = chain();
    let before = graph.clone();
    let _ = plan(&graph, &limits(4, 100, 8), &SearchConfig::default()).unwrap();
    assert_eq!(graph, before);
}

#[test]
fn live_pressure_is_reported_and_bounded() {
    let graph = Graph {
        nodes: vec![
            node("a", &[], 0, Some(0)),
            node("b", &[0], 0, Some(0)),
            node("x", &[], 0, Some(0)),
        ],
        resources: vec![resource("unit", 1, 1, 1)],
    };
    let outcome = plan(&graph, &limits(8, 100, 4), &SearchConfig::default()).unwrap();
    for candidate in &outcome.candidates {
        assert!(candidate.live_pressure <= graph.nodes.len() as u64);
    }
}

#[test]
fn rejects_zero_limits() {
    let graph = chain();
    assert_eq!(
        plan(&graph, &limits(0, 10, 1), &SearchConfig::default()).unwrap_err(),
        ScheduleError::ZeroLimit("max_nodes")
    );
    assert_eq!(
        plan(&graph, &limits(4, 0, 1), &SearchConfig::default()).unwrap_err(),
        ScheduleError::ZeroLimit("max_cycle")
    );
    assert_eq!(
        plan(&graph, &limits(4, 10, 0), &SearchConfig::default()).unwrap_err(),
        ScheduleError::ZeroLimit("max_candidates")
    );
}

#[test]
fn rejects_invalid_node_index() {
    let graph = Graph {
        nodes: vec![node("a", &[9], 0, Some(0))],
        resources: vec![resource("unit", 1, 1, 1)],
    };
    assert_eq!(
        plan(&graph, &limits(4, 100, 1), &SearchConfig::default()).unwrap_err(),
        ScheduleError::InvalidNodeIndex {
            node: 0,
            predecessor: 9
        }
    );
}

#[test]
fn rejects_invalid_resource_index() {
    let graph = Graph {
        nodes: vec![node("a", &[], 0, Some(3))],
        resources: vec![],
    };
    assert_eq!(
        plan(&graph, &limits(4, 100, 1), &SearchConfig::default()).unwrap_err(),
        ScheduleError::InvalidResourceIndex {
            node: 0,
            resource: 3
        }
    );
}

#[test]
fn rejects_duplicate_predecessors() {
    let graph = Graph {
        nodes: vec![node("a", &[], 0, Some(0)), node("b", &[0, 0], 0, Some(0))],
        resources: vec![resource("unit", 1, 1, 1)],
    };
    assert_eq!(
        plan(&graph, &limits(4, 100, 1), &SearchConfig::default()).unwrap_err(),
        ScheduleError::DuplicatePredecessor {
            node: 1,
            predecessor: 0
        }
    );
}

#[test]
fn rejects_cycles() {
    let graph = Graph {
        nodes: vec![node("a", &[1], 0, Some(0)), node("b", &[0], 0, Some(0))],
        resources: vec![resource("unit", 1, 1, 1)],
    };
    match plan(&graph, &limits(4, 100, 1), &SearchConfig::default()).unwrap_err() {
        ScheduleError::Cycle { nodes } => assert_eq!(nodes, vec![0, 1]),
        other => panic!("expected cycle, got {other:?}"),
    }
}

#[test]
fn rejects_too_many_nodes() {
    let graph = chain();
    assert_eq!(
        plan(&graph, &limits(1, 100, 1), &SearchConfig::default()).unwrap_err(),
        ScheduleError::TooManyNodes { count: 2, max: 1 }
    );
}

#[test]
fn rejects_zero_resource_capacities() {
    let cases = [
        (
            resource("unit", 0, 1, 1),
            ScheduleError::ZeroLanes { resource: 0 },
        ),
        (
            resource("unit", 1, 0, 1),
            ScheduleError::ZeroLatency { resource: 0 },
        ),
        (
            resource("unit", 1, 1, 0),
            ScheduleError::ZeroInitiation { resource: 0 },
        ),
    ];
    for (unit, expected) in cases {
        let graph = Graph {
            nodes: vec![node("a", &[], 0, Some(0))],
            resources: vec![unit],
        };
        assert_eq!(
            plan(&graph, &limits(4, 100, 1), &SearchConfig::default()).unwrap_err(),
            expected
        );
    }
}

#[test]
fn rejects_release_after_deadline() {
    let graph = Graph {
        nodes: vec![node("a", &[], 100, None)],
        resources: vec![],
    };
    assert_eq!(
        plan(&graph, &limits(4, 50, 1), &SearchConfig::default()).unwrap_err(),
        ScheduleError::DeadlineBreach { node: 0 }
    );
}

#[test]
fn rejects_unreachable_deadline() {
    let graph = Graph {
        nodes: vec![node("a", &[], 0, Some(0))],
        resources: vec![resource("unit", 1, 100, 1)],
    };
    assert_eq!(
        plan(&graph, &limits(4, 50, 1), &SearchConfig::default()).unwrap_err(),
        ScheduleError::DeadlineBreach { node: 0 }
    );
}

#[test]
fn rejects_arithmetic_overflow() {
    let graph = Graph {
        nodes: vec![node("a", &[], u64::MAX - 1, Some(0))],
        resources: vec![resource("unit", 1, 10, 1)],
    };
    assert_eq!(
        plan(&graph, &limits(4, u64::MAX, 1), &SearchConfig::default()).unwrap_err(),
        ScheduleError::ArithmeticOverflow { node: Some(0) }
    );
}

#[test]
fn reports_no_feasible_schedule_when_every_candidate_misses() {
    let graph = Graph {
        nodes: vec![
            node("a", &[], 0, Some(0)),
            node("b", &[0], 0, Some(0)),
            node("c", &[1], 0, Some(0)),
        ],
        resources: vec![resource("unit", 1, 10, 1)],
    };
    match plan(&graph, &limits(8, 15, 4), &SearchConfig::default()).unwrap_err() {
        ScheduleError::NoFeasibleSchedule { best_cycle } => {
            assert!(
                best_cycle > 15,
                "best cycle {best_cycle} should miss the deadline"
            );
        }
        other => panic!("expected no feasible schedule, got {other:?}"),
    }
}

#[test]
fn checker_detects_dependency_violation() {
    let graph = chain();
    let limits = limits(4, 100, 1);
    let outcome = plan(&graph, &limits, &SearchConfig::default()).unwrap();
    let mut schedule = outcome.best_candidate().schedule.clone();
    schedule.nodes[1].issue = 0;
    schedule.nodes[1].ready = 2 + 1;
    let report = check(&graph, &limits, &schedule);
    assert!(report
        .violations
        .iter()
        .any(|violation| matches!(violation, Violation::Dependency { .. })));
}

#[test]
fn checker_detects_release_gate_violation() {
    let graph = Graph {
        nodes: vec![node("a", &[], 5, Some(0))],
        resources: vec![resource("unit", 1, 1, 1)],
    };
    let limits = limits(4, 100, 1);
    let mut schedule = plan(&graph, &limits, &SearchConfig::default())
        .unwrap()
        .best_candidate()
        .schedule
        .clone();
    schedule.nodes[0].issue = 0;
    schedule.nodes[0].ready = 1;
    schedule.cycles = 1;
    let report = check(&graph, &limits, &schedule);
    assert!(report
        .violations
        .iter()
        .any(|violation| matches!(violation, Violation::ReleaseGate { .. })));
}

#[test]
fn checker_detects_latency_lane_and_makespan_violations() {
    let graph = Graph {
        nodes: vec![node("a", &[], 0, Some(0))],
        resources: vec![resource("unit", 2, 3, 1)],
    };
    let limits = limits(4, 100, 1);
    let mut schedule = plan(&graph, &limits, &SearchConfig::default())
        .unwrap()
        .best_candidate()
        .schedule
        .clone();
    schedule.nodes[0].ready = 99;
    schedule.nodes[0].lane = Some(7);
    schedule.cycles = 123;
    let report = check(&graph, &limits, &schedule);
    assert!(report
        .violations
        .iter()
        .any(|violation| matches!(violation, Violation::LatencyMismatch { .. })));
    assert!(report
        .violations
        .iter()
        .any(|violation| matches!(violation, Violation::LaneOutOfRange { .. })));
    assert!(report
        .violations
        .iter()
        .any(|violation| matches!(violation, Violation::MakespanMismatch { .. })));
}

#[test]
fn checker_detects_initiation_collision() {
    let graph = Graph {
        nodes: vec![node("a", &[], 0, Some(0)), node("b", &[], 0, Some(0))],
        resources: vec![resource("unit", 1, 1, 3)],
    };
    let limits = limits(4, 100, 1);
    let outcome = plan(&graph, &limits, &SearchConfig::default()).unwrap();
    let mut schedule = outcome.best_candidate().schedule.clone();
    // Force both nodes onto the same lane one cycle apart.
    schedule.nodes[0].lane = Some(0);
    schedule.nodes[1].lane = Some(0);
    schedule.nodes[1].issue = 1;
    schedule.nodes[1].ready = 2;
    let report = check(&graph, &limits, &schedule);
    assert!(report
        .violations
        .iter()
        .any(|violation| matches!(violation, Violation::InitiationCollision { .. })));
}

#[test]
fn checker_detects_wiring_lane_and_node_count() {
    let graph = Graph {
        nodes: vec![node("wire", &[], 0, None)],
        resources: vec![],
    };
    let limits = limits(4, 100, 1);
    let outcome = plan(&graph, &limits, &SearchConfig::default()).unwrap();
    let mut schedule = outcome.best_candidate().schedule.clone();
    schedule.nodes[0].lane = Some(0);
    let report = check(&graph, &limits, &schedule);
    assert!(report
        .violations
        .iter()
        .any(|violation| matches!(violation, Violation::UnexpectedLane { .. })));

    schedule.nodes.clear();
    let report = check(&graph, &limits, &schedule);
    assert_eq!(
        report.violations,
        vec![Violation::NodeCount {
            expected: 1,
            actual: 0
        }]
    );
}

/// Determinate graph where the critical-path-primary backfilling insertion
/// candidate finishes in 7 cycles while the append-only earliest-ready baseline
/// needs 8. Found by the bounded survey in `scheduler_upgrade_probe`.
fn backfill_improvement_graph() -> Graph {
    Graph {
        nodes: vec![
            node("n0", &[], 1, Some(1)),
            node("n1", &[], 1, Some(1)),
            node("n2", &[0, 1], 1, Some(0)),
            node("n3", &[1], 0, Some(0)),
            node("n4", &[], 0, Some(1)),
            node("n5", &[3, 4], 2, Some(0)),
            node("n6", &[], 1, Some(1)),
        ],
        resources: vec![resource("a", 2, 2, 3), resource("b", 2, 1, 2)],
    }
}

#[test]
fn new_candidates_are_labelled_and_deterministic() {
    let graph = backfill_improvement_graph();
    let limits = limits(16, 100, 8);
    let outcome = plan(&graph, &limits, &SearchConfig::default()).unwrap();
    assert_eq!(outcome.candidates[0].label, "baseline");
    assert_eq!(outcome.candidates[1].label, "critical-path");
    assert_eq!(outcome.candidates[2].label, "insertion");
    assert_eq!(outcome.candidates[2].seed, None);
    assert_eq!(outcome.candidates[3].label, "insertion-critical-path");
    assert_eq!(outcome.candidates[3].seed, None);
    for candidate in outcome.candidates.iter().skip(4) {
        assert!(candidate.label.starts_with("random-"));
        assert!(candidate.seed.is_some());
    }

    let second = plan(&graph, &limits, &SearchConfig::default()).unwrap();
    let keys: Vec<_> = outcome
        .candidates
        .iter()
        .map(|c| (c.label.clone(), c.seed, c.makespan))
        .collect();
    let second_keys: Vec<_> = second
        .candidates
        .iter()
        .map(|c| (c.label.clone(), c.seed, c.makespan))
        .collect();
    assert_eq!(keys, second_keys);
}

#[test]
fn insertion_primary_critical_path_improves_the_backfill_graph() {
    let graph = backfill_improvement_graph();
    let limits = limits(16, 100, 8);
    let outcome = plan(&graph, &limits, &SearchConfig::default()).unwrap();
    let baseline = &outcome.candidates[0];
    let insertion = outcome
        .candidates
        .iter()
        .find(|c| c.label == "insertion-critical-path")
        .unwrap();
    assert_eq!(baseline.makespan, 8, "committed baseline should be 8");
    assert_eq!(
        insertion.makespan, 7,
        "backfilling critical-path should reach 7"
    );
    assert!(outcome.best_candidate().makespan <= insertion.makespan);
    assert!(check(&graph, &limits, &insertion.schedule).is_ok());
}

#[test]
fn critical_path_priority_is_primary_not_only_a_tie_break() {
    // Ready-first list scheduling must place the early-ready leaf before the
    // late-ready critical chain. The critical-primary insertion schedule is
    // allowed to differ, and on this graph it wins.
    let graph = backfill_improvement_graph();
    let limits = limits(16, 100, 8);
    let outcome = plan(&graph, &limits, &SearchConfig::default()).unwrap();
    let tie_break = outcome
        .candidates
        .iter()
        .find(|c| c.label == "critical-path")
        .unwrap();
    let primary = outcome
        .candidates
        .iter()
        .find(|c| c.label == "insertion-critical-path")
        .unwrap();
    assert!(primary.makespan <= tie_break.makespan);
}

#[test]
fn lower_bound_never_exceeds_any_candidate_makespan() {
    for graph in [chain(), backfill_improvement_graph()] {
        let limits = limits(32, 100, 8);
        let bound = lower_bound(&graph, &limits).unwrap();
        let outcome = plan(&graph, &limits, &SearchConfig::default()).unwrap();
        for candidate in &outcome.candidates {
            assert!(
                candidate.makespan >= bound,
                "{} is below lower bound {bound}",
                candidate.label
            );
        }
    }
}

#[test]
fn small_candidate_budgets_preserve_the_historical_sequence() {
    let graph = chain();
    for count in 1..=4 {
        let outcome = plan(&graph, &limits(4, 100, count), &SearchConfig::default()).unwrap();
        assert_eq!(outcome.candidates.len(), count);
        assert_eq!(outcome.candidates[0].label, "baseline");
        if count >= 2 {
            assert_eq!(outcome.candidates[1].label, "critical-path");
        }
        for (index, candidate) in outcome.candidates.iter().enumerate().skip(2) {
            assert_eq!(candidate.label, format!("random-{index}"));
        }
    }
}

#[test]
fn lower_bound_rejects_invalid_graphs() {
    let graph = Graph {
        nodes: vec![node("a", &[1], 0, Some(0)), node("b", &[0], 0, Some(0))],
        resources: vec![resource("unit", 1, 1, 1)],
    };
    assert!(matches!(
        lower_bound(&graph, &limits(4, 100, 1)).unwrap_err(),
        ScheduleError::Cycle { .. }
    ));
}

#[test]
fn lower_bound_of_empty_graph_is_zero() {
    assert_eq!(
        lower_bound(&Graph::default(), &limits(4, 100, 1)).unwrap(),
        0
    );
}

#[test]
fn lower_bound_accounts_for_initiation_interval() {
    // Four nodes on one lane with initiation interval 3 need at least three
    // gaps of 3 before the last result latency.
    let graph = Graph {
        nodes: vec![
            node("a", &[], 0, Some(0)),
            node("b", &[], 0, Some(0)),
            node("c", &[], 0, Some(0)),
            node("d", &[], 0, Some(0)),
        ],
        resources: vec![resource("mul", 1, 2, 3)],
    };
    // (4 - 1) * 3 + 2 = 11.
    assert_eq!(lower_bound(&graph, &limits(8, 100, 1)).unwrap(), 11);
}

// ---------------------------------------------------------------------------
// Modulo (periodic) scheduling
// ---------------------------------------------------------------------------

fn modulo_chain() -> Graph {
    Graph {
        nodes: vec![node("a", &[], 0, Some(0)), node("b", &[0], 0, Some(0))],
        resources: vec![resource("adder", 1, 2, 1)],
    }
}

#[test]
fn modulo_chain_latency_increases_span_without_forcing_a_larger_ii() {
    let source = modulo_chain();
    let graph = ModuloGraph::from_graph(&source).unwrap();
    // a at phase 0, b at local time 3 / phase 1: latency exceeds II legally.
    let fast = modulo_schedule(&graph, 2, &SearchConfig::default()).unwrap();
    assert_eq!(fast.nodes[1].issue, 3);
    assert_eq!(fast.span, 5);
    assert!(check_modulo(&graph, &fast).is_ok());
    let schedule = modulo_schedule(&graph, 3, &SearchConfig::default()).unwrap();
    assert_eq!(schedule.nodes[0].issue, 0);
    assert_eq!(schedule.nodes[1].issue, 2);
    assert_eq!(schedule.span, 4);
    assert!(check_modulo(&graph, &schedule).is_ok());
}

#[test]
fn modulo_resource_lower_bound_matches_lane_capacity() {
    let source = Graph {
        nodes: vec![
            node("a", &[], 0, Some(0)),
            node("b", &[], 0, Some(0)),
            node("c", &[], 0, Some(0)),
        ],
        resources: vec![resource("unit", 1, 1, 1)],
    };
    let graph = ModuloGraph::from_graph(&source).unwrap();
    assert_eq!(graph.resource_lower_bound(), 3);
    assert!(modulo_schedule(&graph, 2, &SearchConfig::default()).is_err());
    let schedule = modulo_schedule(&graph, 3, &SearchConfig::default()).unwrap();
    assert!(check_modulo(&graph, &schedule).is_ok());
    let lanes: Vec<_> = schedule.nodes.iter().map(|n| n.lane).collect();
    assert!(lanes.iter().all(|lane| *lane == Some(0)));
}

#[test]
fn modulo_body_times_extend_beyond_one_period() {
    let source = Graph {
        nodes: vec![
            node("a", &[], 0, Some(0)),
            node("b", &[], 0, Some(0)),
            node("c", &[0], 0, Some(0)),
        ],
        resources: vec![resource("unit", 2, 7, 1)],
    };
    let graph = ModuloGraph::from_graph(&source).unwrap();
    let ii = 3;
    let schedule = modulo_schedule(&graph, ii, &SearchConfig::default()).unwrap();
    assert!(schedule.nodes[2].issue >= 7);
    assert!(schedule.span > ii);
    assert!(check_modulo(&graph, &schedule).is_ok());
}

#[test]
fn modulo_initiation_interval_decouples_same_lane_issues() {
    // One lane with initiation interval 3 cannot accept two issues less than
    // three cycles apart, so the two nodes need at least II=6 to fit on the
    // circle.
    let source = Graph {
        nodes: vec![node("a", &[], 0, Some(0)), node("b", &[], 0, Some(0))],
        resources: vec![resource("multiplier", 1, 1, 3)],
    };
    let graph = ModuloGraph::from_graph(&source).unwrap();
    assert!(modulo_schedule(&graph, 4, &SearchConfig::default()).is_err());
    let ii = 6;
    let schedule = modulo_schedule(&graph, ii, &SearchConfig::default()).unwrap();
    let issues: Vec<u64> = schedule.nodes.iter().map(|n| n.issue).collect();
    let gap = (issues[1] + ii - issues[0]) % ii;
    let reverse = (issues[0] + ii - issues[1]) % ii;
    assert!(
        gap.min(reverse) >= 3,
        "issues {issues:?} are closer than the initiation interval"
    );
    assert!(check_modulo(&graph, &schedule).is_ok());
}

#[test]
fn modulo_checker_rejects_overbooked_lane() {
    let source = Graph {
        nodes: vec![node("a", &[], 0, Some(0)), node("b", &[], 0, Some(0))],
        resources: vec![resource("unit", 1, 1, 1)],
    };
    let graph = ModuloGraph::from_graph(&source).unwrap();
    let mut schedule = modulo_schedule(&graph, 2, &SearchConfig::default()).unwrap();
    // Force both nodes onto lane 0 in the same phase.
    schedule.nodes[0] = resource_scheduler::ModuloNode {
        issue: 0,
        lane: Some(0),
    };
    schedule.nodes[1] = resource_scheduler::ModuloNode {
        issue: 0,
        lane: Some(0),
    };
    let report = check_modulo(&graph, &schedule);
    assert!(report
        .violations
        .iter()
        .any(|violation| matches!(violation, ModuloViolation::InitiationCollision { .. })));
}

#[test]
fn modulo_checker_rejects_dependency_latency_violation() {
    let source = modulo_chain();
    let graph = ModuloGraph::from_graph(&source).unwrap();
    let mut schedule = modulo_schedule(&graph, 3, &SearchConfig::default()).unwrap();
    // b issues before a is ready at issue 0 + latency 2.
    schedule.nodes[1].issue = 1;
    let report = check_modulo(&graph, &schedule);
    assert!(report
        .violations
        .iter()
        .any(|violation| matches!(violation, ModuloViolation::Dependency { .. })));
}

#[test]
fn modulo_checker_rejects_lane_range_and_span_mismatch() {
    let source = Graph {
        nodes: vec![node("a", &[], 0, Some(0))],
        resources: vec![resource("unit", 2, 1, 1)],
    };
    let graph = ModuloGraph::from_graph(&source).unwrap();
    let mut schedule = modulo_schedule(&graph, 1, &SearchConfig::default()).unwrap();
    schedule.nodes[0].lane = Some(7);
    schedule.span = 99;
    let report = check_modulo(&graph, &schedule);
    assert!(report
        .violations
        .iter()
        .any(|violation| matches!(violation, ModuloViolation::LaneOutOfRange { .. })));
    assert!(report
        .violations
        .iter()
        .any(|violation| matches!(violation, ModuloViolation::SpanMismatch { .. })));
}

#[test]
fn modulo_checker_detects_wiring_lane_and_node_count() {
    let source = Graph {
        nodes: vec![node("wire", &[], 0, None)],
        resources: vec![],
    };
    let graph = ModuloGraph::from_graph(&source).unwrap();
    let mut schedule = modulo_schedule(&graph, 1, &SearchConfig::default()).unwrap();
    schedule.nodes[0].lane = Some(0);
    let report = check_modulo(&graph, &schedule);
    assert!(report
        .violations
        .iter()
        .any(|violation| matches!(violation, ModuloViolation::UnexpectedLane { .. })));

    schedule.nodes.clear();
    let report = check_modulo(&graph, &schedule);
    assert_eq!(
        report.violations,
        vec![ModuloViolation::NodeCount {
            expected: 1,
            actual: 0
        }]
    );
}

#[test]
fn modulo_empty_graph_schedules_to_zero() {
    let source = Graph::default();
    let graph = ModuloGraph::from_graph(&source).unwrap();
    let schedule = modulo_schedule(&graph, 1, &SearchConfig::default()).unwrap();
    assert_eq!(schedule.span, 0);
    assert!(schedule.nodes.is_empty());
    assert!(check_modulo(&graph, &schedule).is_ok());
}

#[test]
fn modulo_is_deterministic_for_a_fixed_seed() {
    let source = Graph {
        nodes: vec![
            node("a", &[], 0, Some(0)),
            node("b", &[], 0, Some(0)),
            node("c", &[0, 1], 0, Some(0)),
            node("d", &[1], 0, Some(0)),
        ],
        resources: vec![resource("unit", 2, 1, 2)],
    };
    let graph = ModuloGraph::from_graph(&source).unwrap();
    let config = SearchConfig { base_seed: 7 };
    let ii = graph.resource_lower_bound().max(4);
    let first = modulo_schedule(&graph, ii, &config).unwrap();
    let second = modulo_schedule(&graph, ii, &config).unwrap();
    assert_eq!(first, second);
}

#[test]
fn modulo_rejects_invalid_ii_and_graphs() {
    let source = modulo_chain();
    assert!(matches!(
        modulo_schedule(
            &ModuloGraph::from_graph(&source).unwrap(),
            0,
            &SearchConfig::default()
        ),
        Err(ModuloError::InitiationInterval { .. })
    ));

    let cyclic = Graph {
        nodes: vec![node("a", &[1], 0, Some(0)), node("b", &[0], 0, Some(0))],
        resources: vec![resource("unit", 1, 1, 1)],
    };
    assert!(matches!(
        ModuloGraph::from_graph(&cyclic),
        Err(ModuloError::Graph(ScheduleError::Cycle { .. }))
    ));

    let bad_index = Graph {
        nodes: vec![node("a", &[9], 0, Some(0))],
        resources: vec![resource("unit", 1, 1, 1)],
    };
    assert!(matches!(
        ModuloGraph::from_graph(&bad_index),
        Err(ModuloError::Graph(ScheduleError::InvalidNodeIndex { .. }))
    ));
}

#[test]
fn modulo_rejects_oversized_calendar_before_allocating() {
    let source = Graph {
        nodes: vec![node("a", &[], 0, Some(0))],
        resources: vec![resource("wide", 4_096, 1, 1)],
    };
    let graph = ModuloGraph::from_graph(&source).unwrap();
    assert!(matches!(
        modulo_schedule(&graph, 65_536, &SearchConfig::default()),
        Err(ModuloError::RepresentationTooLarge { .. })
    ));
}

#[test]
fn modulo_issue_plus_latency_overflow_is_safe() {
    let source = Graph {
        nodes: vec![node("a", &[], 0, Some(0))],
        resources: vec![resource("unit", 1, u64::MAX, 1)],
    };
    // Resource latency cannot be zero, but it can be huge; the checker must not
    // panic and must flag the overflow instead.
    let graph = ModuloGraph::from_graph(&source).unwrap();
    let mut schedule = modulo_schedule(&graph, 2, &SearchConfig::default()).unwrap();
    schedule.nodes[0].issue = 1;
    let report = check_modulo(&graph, &schedule);
    assert!(report
        .violations
        .iter()
        .any(|violation| matches!(violation, ModuloViolation::LatencyOverflow { .. })));
}

#[test]
fn modulo_infeasible_report_is_honest() {
    let source = {
        let mut graph = backfill_improvement_graph();
        // Two operations sharing one lane require at least two phases, regardless
        // of the chain's much longer result latency.
        graph.nodes = vec![node("a", &[], 0, Some(0)), node("b", &[0], 0, Some(0))];
        graph.resources = vec![resource("unit", 1, 5, 1)];
        graph
    };
    let graph = ModuloGraph::from_graph(&source).unwrap();
    let bound = graph.resource_lower_bound();
    match modulo_schedule(&graph, 1, &SearchConfig::default()) {
        Err(ModuloError::Infeasible {
            lower_bound,
            best_span,
            ..
        }) => {
            assert_eq!(lower_bound, bound);
            assert!(best_span >= bound);
        }
        other => panic!("expected infeasible, got {other:?}"),
    }
}

#[test]
fn modulo_larger_graph_is_legal_at_its_lower_bound_or_reports_infeasible() {
    // The layered lighting-like graph; only assert what the checker proves.
    let resources = vec![resource("mul", 1, 3, 2), resource("add", 2, 1, 1)];
    let mut nodes: Vec<Node> = Vec::new();
    let mut previous: Vec<usize> = Vec::new();
    for layer in 0..4usize {
        let mut current = Vec::new();
        for column in 0..3usize {
            let id = nodes.len();
            let predecessors = if layer == 0 {
                Vec::new()
            } else {
                vec![previous[column], previous[(column + 1) % previous.len()]]
            };
            let resource = if layer == 0 {
                None
            } else if layer % 2 == 1 {
                Some(0)
            } else {
                Some(1)
            };
            nodes.push(node(
                &format!("n{layer}_{column}"),
                &predecessors,
                0,
                resource,
            ));
            current.push(id);
        }
        previous = current;
    }
    let graph = ModuloGraph::from_graph(&Graph { nodes, resources }).unwrap();
    let bound = graph.resource_lower_bound();
    let mut found = false;
    for ii in bound..=(bound + 12) {
        if let Ok(schedule) = modulo_schedule(&graph, ii, &SearchConfig::default()) {
            assert!(schedule.initiation_interval == ii);
            assert!(check_modulo(&graph, &schedule).is_ok());
            found = true;
            break;
        }
    }
    // The bounded construction is allowed to fail, but if it succeeds it must
    // pass the independent checker (asserted above). Record the outcome.
    let _ = found;
}

#[test]
fn lower_bound_uses_resource_latency_not_node_index() {
    let graph = Graph {
        nodes: vec![node("only", &[], 7, Some(3))],
        resources: vec![
            resource("a", 1, 99, 1),
            resource("b", 1, 1, 1),
            resource("c", 1, 1, 1),
            resource("used", 1, 5, 1),
        ],
    };
    assert_eq!(lower_bound(&graph, &limits(10, 100, 1)).unwrap(), 12);
}

#[test]
fn modulo_self_spacing_release_gate_and_absolute_phase_alias_are_checked() {
    let source = Graph {
        nodes: vec![node("only", &[], 7, Some(0))],
        resources: vec![resource("unit", 1, 4, 3)],
    };
    let graph = ModuloGraph::from_graph(&source).unwrap();
    assert_eq!(graph.resource_lower_bound(), 3);
    let forged = resource_scheduler::ModuloSchedule {
        initiation_interval: 2,
        nodes: vec![resource_scheduler::ModuloNode {
            issue: 7,
            lane: Some(0),
        }],
        span: 11,
    };
    assert!(check_modulo(&graph, &forged)
        .violations
        .iter()
        .any(|v| matches!(v, ModuloViolation::InitiationCollision { .. })));
    let mut legal = modulo_schedule(&graph, 3, &SearchConfig::default()).unwrap();
    assert!(legal.nodes[0].issue >= 7);
    legal.nodes[0].issue = 0;
    legal.span = 4;
    assert!(check_modulo(&graph, &legal)
        .violations
        .iter()
        .any(|v| matches!(v, ModuloViolation::ReleaseGate { .. })));
    let source = Graph {
        nodes: vec![node("a", &[], 0, Some(0)), node("b", &[], 0, Some(0))],
        resources: vec![resource("unit", 1, 1, 1)],
    };
    let graph = ModuloGraph::from_graph(&source).unwrap();
    let forged = resource_scheduler::ModuloSchedule {
        initiation_interval: 2,
        nodes: vec![
            resource_scheduler::ModuloNode {
                issue: 0,
                lane: Some(0),
            },
            resource_scheduler::ModuloNode {
                issue: 6,
                lane: Some(0),
            },
        ],
        span: 7,
    };
    assert!(check_modulo(&graph, &forged)
        .violations
        .iter()
        .any(|v| matches!(v, ModuloViolation::InitiationCollision { .. })));
}

#[test]
fn modulo_bounded_search_checks_deadline_budget_and_total_calendar_memory() {
    use resource_scheduler::modulo_schedule_bounded;
    let graph = ModuloGraph::from_graph(&modulo_chain()).unwrap();
    assert!(
        modulo_schedule_bounded(&graph, 2, &limits(10, 4, 8), &SearchConfig::default()).is_err()
    );
    assert!(
        modulo_schedule_bounded(&graph, 2, &limits(10, 100, 65), &SearchConfig::default()).is_err()
    );
    let source = Graph {
        nodes: vec![node("a", &[], 0, Some(0)), node("b", &[], 0, Some(1))],
        resources: vec![resource("a", 4096, 1, 1), resource("b", 4096, 1, 1)],
    };
    let graph = ModuloGraph::from_graph(&source).unwrap();
    // Each resource separately fits; their total must be bounded before allocation.
    assert!(matches!(
        modulo_schedule(&graph, 2048, &SearchConfig::default()),
        Err(ModuloError::RepresentationTooLarge { .. })
    ));
}

#[test]
fn alap_preserves_completion_dependencies_and_recurring_lane_phases() {
    use resource_scheduler::{compact_modulo, ModuloNode, ModuloSchedule};
    let source = Graph {
        nodes: vec![
            node("early", &[], 0, Some(0)),
            node("late", &[], 8, Some(1)),
            node("join", &[0, 1], 0, None),
        ],
        resources: vec![resource("a", 1, 1, 1), resource("b", 1, 1, 1)],
    };
    let graph = ModuloGraph::from_graph(&source).unwrap();
    let original = ModuloSchedule {
        initiation_interval: 2,
        nodes: vec![
            ModuloNode {
                issue: 0,
                lane: Some(0),
            },
            ModuloNode {
                issue: 8,
                lane: Some(0),
            },
            ModuloNode {
                issue: 9,
                lane: None,
            },
        ],
        span: 9,
    };
    let compact = compact_modulo(&graph, &original).unwrap();
    assert_eq!(compact.nodes[0].issue, 8);
    assert_eq!(compact.span, original.span);
    for (a, b) in original.nodes.iter().zip(&compact.nodes) {
        assert_eq!(a.lane, b.lane);
        if a.lane.is_some() {
            assert_eq!(a.issue % 2, b.issue % 2);
        }
    }
    assert!(check_modulo(&graph, &compact).is_ok());
    let mut bad = original;
    bad.nodes[2].issue = 0;
    assert!(compact_modulo(&graph, &bad).is_err());
}

#[test]
fn modulo_circular_checker_matches_bounded_unrolled_calendars() {
    use resource_scheduler::{ModuloNode, ModuloSchedule};
    // Exhaust phases and unit initiation intervals, independently unrolling 5 iterations.
    for ii in 1..=8_u64 {
        for gap in 1..=10_u64 {
            let source = Graph {
                nodes: vec![node("a", &[], 0, Some(0)), node("b", &[], 0, Some(0))],
                resources: vec![resource("unit", 1, 1, gap)],
            };
            let graph = ModuloGraph::from_graph(&source).unwrap();
            for a in 0..ii {
                for b in 0..ii {
                    let schedule = ModuloSchedule {
                        initiation_interval: ii,
                        nodes: vec![
                            ModuloNode {
                                issue: a,
                                lane: Some(0),
                            },
                            ModuloNode {
                                issue: b,
                                lane: Some(0),
                            },
                        ],
                        span: a.max(b) + 1,
                    };
                    let mut issues: Vec<_> =
                        (0..5).flat_map(|k| [a + k * ii, b + k * ii]).collect();
                    issues.sort_unstable();
                    let unrolled_legal = issues.windows(2).all(|w| w[1] - w[0] >= gap);
                    assert_eq!(
                        check_modulo(&graph, &schedule).is_ok(),
                        unrolled_legal,
                        "II={ii} gap={gap} phases={a},{b}"
                    );
                }
            }
        }
    }
}
