//! Public-API tests for the resource scheduler.

use resource_scheduler::{
    check, plan, Graph, Limits, Node, Resource, ScheduleError, SearchConfig, Violation,
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
