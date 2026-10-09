//! Deterministic probe for the scheduler upgrade: finite backfill insertion,
//! critical-path priority, resource lower bounds, and modulo (periodic)
//! scheduling with the independent periodic checker.
//!
//! Writes `summary.csv` and a per-graph candidate dump into the report
//! directory given as the first argument (default `target/scheduler-upgrade`).
//! Run with:
//! `cargo run -p gpu-v2 --example scheduler_upgrade_probe -- <dir>`

use resource_scheduler::{
    check, check_modulo, lower_bound, modulo_schedule, plan, Graph, Limits, ModuloError,
    ModuloGraph, Node, Resource, SearchConfig,
};
use std::fs;
use std::path::{Path, PathBuf};

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

/// A graph where a critical late-ready node is placed first and earlier-ready
/// work can be backfilled into the hole it leaves.
fn backfill_graph() -> Graph {
    Graph {
        nodes: vec![
            // Two independent roots on the slow resource.
            node("fast_a", &[], 0, Some(0)),
            node("fast_b", &[], 0, Some(0)),
            // A long dependency chain that only frees its sink late.
            node("chain", &[], 0, Some(0)),
            node("gate", &[2], 0, None),
            node("late", &[3], 0, Some(0)),
            // Independent short work that is ready early.
            node("fill", &[], 0, Some(0)),
        ],
        resources: vec![resource("slow", 1, 5, 2)],
    }
}

/// A layered DAG mixing a pipelined multiply unit (II=2) and an add unit.
fn layered_graph() -> Graph {
    let resources = vec![resource("mul", 1, 3, 2), resource("add", 2, 1, 1)];
    let mut nodes = Vec::new();
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
    Graph { nodes, resources }
}

fn report_graph(dir: &Path, title: &str, graph: &Graph) -> String {
    let limits = Limits::new(256, 100_000, 32);
    let outcome = plan(graph, &limits, &SearchConfig::default()).expect("feasible plan");
    let bound = lower_bound(graph, &limits).expect("valid graph");
    let mut lines = String::new();
    lines.push_str(&format!(
        "{title},nodes={},resources={},lower_bound={}\n",
        graph.nodes.len(),
        graph.resources.len(),
        bound
    ));
    lines.push_str("label,seed,makespan,live_pressure,within_deadline,checker_ok,gap_to_bound\n");
    for candidate in &outcome.candidates {
        let report = check(graph, &limits, &candidate.schedule);
        assert!(
            report.is_ok(),
            "{title} {}: {:?}",
            candidate.label,
            report.violations
        );
        lines.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            candidate.label,
            candidate
                .seed
                .map(|s| s.to_string())
                .unwrap_or_else(|| "-".to_owned()),
            candidate.makespan,
            candidate.live_pressure,
            candidate.within_deadline,
            report.is_ok(),
            candidate.makespan.saturating_sub(bound),
        ));
    }
    let baseline = &outcome.candidates[0];
    let best = outcome.best_candidate();
    lines.push_str(&format!(
        "best={},baseline={},improvement={}\n",
        best.label,
        baseline.makespan,
        baseline.makespan.saturating_sub(best.makespan)
    ));
    fs::write(dir.join(format!("{title}.txt")), &lines).expect("write report");
    format!(
        "{title},{},{},{},{},{}\n",
        bound,
        baseline.makespan,
        best.makespan,
        baseline.makespan.saturating_sub(best.makespan),
        best.label
    )
}

/// Modulo probe: try increasing II from the resource lower bound and report the
/// first legal calendar with its span, independently rechecked.
fn report_modulo(dir: &Path, title: &str, graph: &Graph) -> String {
    let modulo = ModuloGraph::from_graph(graph).expect("valid graph");
    let lower = modulo.resource_lower_bound();
    let span_bound = modulo.span_lower_bound();
    let mut chosen: Option<(u64, u64)> = None;
    for ii in lower..=(lower + 8) {
        if let Ok(schedule) = modulo_schedule(&modulo, ii, &SearchConfig::default()) {
            let report = check_modulo(&modulo, &schedule);
            assert!(report.is_ok(), "{title} II={ii}: {:?}", report.violations);
            chosen = Some((ii, schedule.span));
            break;
        }
    }
    let text = match chosen {
        Some((ii, span)) => format!(
            "{title},resources_bound={lower},span_bound={span_bound},ii={ii},span={span},legal=true\n"
        ),
        None => format!(
            "{title},resources_bound={lower},span_bound={span_bound},legal=false,reason={:?}\n",
            modulo_schedule(&modulo, lower, &SearchConfig::default()).unwrap_err()
        ),
    };
    fs::write(dir.join(format!("{title}.modulo.txt")), &text).expect("write modulo report");
    text
}

/// Bounded deterministic survey of small random graphs.
///
/// Reports how often each deterministic candidate class strictly improves on
/// the append-only baseline. This keeps the probe honest about whether the new
/// candidate kinds actually help.
fn survey_candidates() -> String {
    let mut state = 0x6ce4_231b_1978_02abu64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let limits = Limits::new(64, 100_000, 8);
    let mut graphs = 0u64;
    let mut improvement = [0u64; 4]; // critical-path, insertion, insertion-critical-path, random
    for _ in 0..4_000 {
        let node_count = 4 + (next() % 6) as usize;
        let resources = vec![
            resource(
                "a",
                1 + (next() % 2) as usize,
                1 + next() % 4,
                1 + next() % 4,
            ),
            resource(
                "b",
                1 + (next() % 2) as usize,
                1 + next() % 3,
                1 + next() % 3,
            ),
        ];
        let mut nodes = Vec::new();
        for id in 0..node_count {
            let mut predecessors = Vec::new();
            let mut count = (next() % 3) as usize;
            while count > 0 {
                let candidate = (next() as usize) % id.max(1);
                if id > 0 && !predecessors.contains(&candidate) {
                    predecessors.push(candidate);
                }
                count -= 1;
            }
            let resource = if next() % 4 == 0 {
                None
            } else {
                Some((next() % 2) as usize)
            };
            nodes.push(node(&format!("n{id}"), &predecessors, next() % 4, resource));
        }
        let graph = Graph { nodes, resources };
        let Ok(outcome) = plan(&graph, &limits, &SearchConfig::default()) else {
            continue;
        };
        graphs += 1;
        let baseline = outcome.candidates[0].makespan;
        for candidate in &outcome.candidates {
            if candidate.makespan >= baseline {
                continue;
            }
            match candidate.label.as_str() {
                "critical-path" => improvement[0] += 1,
                "insertion" => improvement[1] += 1,
                "insertion-critical-path" => improvement[2] += 1,
                label if label.starts_with("random-") => improvement[3] += 1,
                _ => {}
            }
        }
    }
    format!(
        "scan,graphs={graphs},critical_path={},insertion={},insertion_critical={},random={}\n",
        improvement[0], improvement[1], improvement[2], improvement[3]
    )
}

fn find_insertion_improvement() -> Option<(Graph, u64, u64)> {
    let mut state = 0x6ce4_231b_1978_02abu64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let limits = Limits::new(64, 100_000, 8);
    for _ in 0..4_000 {
        let node_count = 4 + (next() % 6) as usize;
        let resources = vec![
            resource(
                "a",
                1 + (next() % 2) as usize,
                1 + next() % 4,
                1 + next() % 4,
            ),
            resource(
                "b",
                1 + (next() % 2) as usize,
                1 + next() % 3,
                1 + next() % 3,
            ),
        ];
        let mut nodes = Vec::new();
        for id in 0..node_count {
            let mut predecessors = Vec::new();
            let mut count = (next() % 3) as usize;
            while count > 0 {
                let candidate = (next() as usize) % id.max(1);
                if id > 0 && !predecessors.contains(&candidate) {
                    predecessors.push(candidate);
                }
                count -= 1;
            }
            let resource = if next() % 4 == 0 {
                None
            } else {
                Some((next() % 2) as usize)
            };
            nodes.push(node(&format!("n{id}"), &predecessors, next() % 4, resource));
        }
        let graph = Graph { nodes, resources };
        let Ok(outcome) = plan(&graph, &limits, &SearchConfig::default()) else {
            continue;
        };
        let baseline = outcome.candidates[0].makespan;
        if let Some(best) = outcome
            .candidates
            .iter()
            .find(|c| c.label == "insertion-critical-path" && c.makespan < baseline)
        {
            return Some((graph, baseline, best.makespan));
        }
    }
    None
}

fn main() {
    let dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/scheduler-upgrade"));
    fs::create_dir_all(&dir).expect("create report directory");

    let mut summary = String::from("title,lower_bound,baseline,best,improvement,best_label\n");
    for (title, graph) in [("backfill", backfill_graph()), ("layered", layered_graph())] {
        summary.push_str(&report_graph(&dir, title, &graph));
    }
    println!("{summary}");

    let mut modulo_summary = String::new();
    for (title, graph) in [("backfill", backfill_graph()), ("layered", layered_graph())] {
        modulo_summary.push_str(&report_modulo(&dir, title, &graph));
    }
    println!("{modulo_summary}");

    // Demonstrate honest infeasibility reporting at an unattainably small II.
    let graph = backfill_graph();
    let modulo = ModuloGraph::from_graph(&graph).unwrap();
    match modulo_schedule(&modulo, 1, &SearchConfig::default()) {
        Err(ModuloError::Infeasible {
            initiation_interval,
            lower_bound,
            best_span,
        }) => println!(
            "ii=1 infeasible, resource_lower_bound={lower_bound}, best_span={best_span}, ii={initiation_interval}"
        ),
        other => println!("unexpected ii=1 outcome: {other:?}"),
    }

    match find_insertion_improvement() {
        Some((graph, baseline, insertion)) => {
            println!(
                "insertion improvement found: nodes={} baseline={baseline} insertion={insertion}",
                graph.nodes.len()
            );
            println!("resources:");
            for unit in &graph.resources {
                println!(
                    "  ({}, {}, {}, {}),",
                    unit.name, unit.lanes, unit.latency, unit.initiation_interval
                );
            }
            println!("nodes:");
            for spec in &graph.nodes {
                println!(
                    "  ({:?}, {:?}, {}, {:?}),",
                    spec.name, spec.predecessors, spec.earliest, spec.resource
                );
            }
        }
        None => println!("bounded scan found no graph where insertion strictly beats the baseline"),
    }
    println!("{}", survey_candidates());

    println!("reports: {}", dir.display());
}
