//! Deterministic candidate dump for a synthetic layered DAG.
//!
//! Run with:
//! `cargo run -p resource-scheduler --example compare --release`

use resource_scheduler::{check, plan, Graph, Limits, Node, Resource, SearchConfig};

fn main() {
    let resources = vec![
        Resource {
            name: "add".to_owned(),
            lanes: 2,
            latency: 1,
            initiation_interval: 1,
        },
        Resource {
            name: "mul".to_owned(),
            lanes: 1,
            latency: 4,
            initiation_interval: 2,
        },
    ];

    // Four layers of four nodes. Layer 0 is wiring (array reads); later layers
    // alternate between the multiply and add resources and each node consumes
    // two predecessors from the previous layer.
    let layers = 4usize;
    let width = 4usize;
    let mut nodes: Vec<Node> = Vec::new();
    let mut previous: Vec<usize> = Vec::new();
    for layer in 0..layers {
        let mut current = Vec::new();
        for column in 0..width {
            let id = nodes.len();
            let predecessors = if layer == 0 {
                Vec::new()
            } else {
                vec![previous[column], previous[(column + 1) % width]]
            };
            let resource = if layer == 0 {
                None
            } else if layer % 2 == 1 {
                Some(1)
            } else {
                Some(0)
            };
            nodes.push(Node {
                name: format!("n{layer}_{column}"),
                predecessors,
                earliest: 0,
                resource,
            });
            current.push(id);
        }
        previous = current;
    }

    let graph = Graph { nodes, resources };
    let limits = Limits::new(64, 1_000, 32);
    let outcome = plan(&graph, &limits, &SearchConfig::default()).expect("feasible plan");

    println!(
        "nodes={} resources={}",
        graph.nodes.len(),
        graph.resources.len()
    );
    println!("candidates:");
    for candidate in &outcome.candidates {
        let report = check(&graph, &limits, &candidate.schedule);
        println!(
            "  {:<14} seed={:>20} makespan={:>3} live={:>2} deadline_ok={} checker_ok={}",
            candidate.label,
            candidate
                .seed
                .map(|seed| seed.to_string())
                .unwrap_or_else(|| "-".to_owned()),
            candidate.makespan,
            candidate.live_pressure,
            candidate.within_deadline,
            report.is_ok(),
        );
    }
    let best = outcome.best_candidate();
    println!(
        "best: {} makespan={} live={}",
        best.label, best.makespan, best.live_pressure
    );
    let baseline = &outcome.candidates[0];
    println!(
        "baseline makespan={} improvement={} cycles",
        baseline.makespan,
        baseline.makespan.saturating_sub(best.makespan)
    );
    println!("best schedule:");
    for (id, assignment) in best.schedule.nodes.iter().enumerate() {
        println!(
            "  {:>2} {:<6} issue={:>2} ready={:>2} lane={:?}",
            id, graph.nodes[id].name, assignment.issue, assignment.ready, assignment.lane
        );
    }
    assert!(check(&graph, &limits, &best.schedule).is_ok());
}
