use super::*;
#[test]
fn reviewed_workbench_matches_cycle_rtl_and_has_explicit_quantization() {
    {
        let profile = LightingProfile::Fast;
        for quantization in [
            LightingQuantization::CompensatedFloor,
            LightingQuantization::NearestEven,
        ] {
            let options = LightingRtlOptions::lit_queue_resource_profile(profile, quantization);
            let rtl = crate::lighting::rtl::generate_with_options(profile, options).unwrap();
            for full in [false, true] {
                let w = Workbench::with_quantization(profile, full, quantization).unwrap();
                assert_eq!(
                    w.latency,
                    if full {
                        rtl.latency
                    } else {
                        rtl.diffuse_latency
                    }
                );
                assert_eq!(
                    w.baseline.initiation_interval as usize,
                    if full {
                        rtl.specular_ii
                    } else {
                        rtl.diffuse_ii
                    }
                );
                let p = Program::with_retiming(
                    profile,
                    full,
                    options.dedicated_dsp,
                    options.kernel(),
                    options.role_schedule,
                    options.logic_depth,
                    options.retiming,
                )
                .unwrap();
                assert_eq!(w.baseline, p.schedule);
                for n in &w.nodes {
                    assert_eq!(
                        w.baseline.nodes[n.id].issue as usize,
                        p.instructions
                            .iter()
                            .find(|i| i.root == n.id)
                            .map_or(p.latency - 1, |i| i.issue)
                    );
                }
                let biased = w
                    .nodes
                    .iter()
                    .any(|n| n.label.contains("POWER_MIDPOINT_Q15"));
                assert_eq!(
                    biased,
                    full && quantization == LightingQuantization::CompensatedFloor
                );
            }
        }
    }
}
#[test]
fn ports_use_physical_fusion_operands_and_real_output_values() {
    for full in [false, true] {
        let w = Workbench::new(LightingProfile::Fast, full).unwrap();
        let options = LightingRtlOptions::lit_queue_resource_profile(
            LightingProfile::Fast,
            LightingQuantization::CompensatedFloor,
        );
        let p = Program::with_retiming(
            LightingProfile::Fast,
            full,
            options.dedicated_dsp,
            options.kernel(),
            options.role_schedule,
            options.logic_depth,
            options.retiming,
        )
        .unwrap();
        let mut fused = 0;
        for n in &w.nodes {
            if let Some(i) = p.instructions.iter().find(|i| i.root == n.id) {
                assert_eq!(
                    n.inputs.iter().map(|p| p.value).collect::<Vec<_>>(),
                    i.inputs
                );
                assert_eq!(n.outputs[0].value, p.frame.events[n.id].output.unwrap());
                if i.members.len() > 1 {
                    fused += 1;
                }
            } else {
                assert_eq!(
                    n.inputs.iter().map(|p| p.value).collect::<Vec<_>>(),
                    p.output_values
                );
                assert!(n.outputs.is_empty());
            }
            for input in &n.inputs {
                assert!(!input.label.is_empty());
                assert!(!input.label.starts_with(&format!("v{}", input.value)));
                assert!(input.sources.iter().all(|s| n.parents.contains(s)));
                if p.frame.events[p.frame.values[input.value].producer].operation
                    == audited::Operation::Literal
                {
                    assert!(
                        input.sources.is_empty(),
                        "a branch gate is not a literal producer"
                    );
                }
            }
        }
        assert!(fused > 0);
    }
}
#[test]
fn block_details_preserve_constants_fusion_and_pipeline_contract() {
    {
        let profile = LightingProfile::Fast;
        for full in [false, true] {
            for quantization in [
                LightingQuantization::CompensatedFloor,
                LightingQuantization::NearestEven,
            ] {
                let w = Workbench::with_quantization(profile, full, quantization).unwrap();
                assert!(w.graph.resources.iter().all(|r| r.initiation_interval == 1));
                let abs = w
                    .nodes
                    .iter()
                    .find(|n| n.recipe.iter().any(|line| line.contains("normal.abs.x")))
                    .unwrap();
                for step in ["subtract", "compare", "select"] {
                    assert!(abs.steps.iter().any(|s| s == step));
                }
                assert!(abs.inputs.iter().any(|p| !p.constant));
                assert!(abs.inputs.iter().any(|p| p.constant));
                assert!(abs.recipe.iter().any(|line| line.contains(" < ")));
                assert!(abs.recipe.iter().any(|line| line.contains("select(")));
                // In a deeper contraction safe-magnitude wiring need not
                // remain a separate block; every surviving select must
                // still carry its complete choice and arithmetic recipe.
                for n in &w.nodes {
                    assert_eq!(n.choice.is_some(), n.steps.iter().any(|s| s == "select"));
                    assert_eq!(
                        n.recipe,
                        n.program_lines
                            .iter()
                            .map(|&line| {
                                let p = &w.program[line - 1];
                                format!("{}{}{}", p.before, p.operator, p.after)
                            })
                            .collect::<Vec<_>>()
                    );
                    for p in n.inputs.iter().filter(|p| p.constant) {
                        assert!(p.sources.is_empty(), "a constant must have no runtime wire");
                    }
                    // Context/ROM outputs remain runtime operands even when
                    // the template used identical fixture values.
                    if n.read_memory.is_some() {
                        assert!(n.outputs.iter().all(|p| !p.constant));
                    }
                }
            }
        }
    }
}
#[test]
fn readable_program_preserves_fusion_members_and_kernel_locations() {
    for full in [false, true] {
        let w = Workbench::new(LightingProfile::Fast, full).unwrap();
        let options = LightingRtlOptions::lit_queue_resource_profile(
            LightingProfile::Fast,
            LightingQuantization::CompensatedFloor,
        );
        let p = Program::with_retiming(
            LightingProfile::Fast,
            full,
            options.dedicated_dsp,
            options.kernel(),
            options.role_schedule,
            options.logic_depth,
            options.retiming,
        )
        .unwrap();
        for n in &w.nodes {
            assert!(!n.program_lines.is_empty(), "missing program for {}", n.id);
            for &line in &n.program_lines {
                let info = &w.program[line - 1];
                assert!(!info.operator.is_empty());
                if let Some(i) = p.instructions.iter().find(|i| i.root == n.id) {
                    assert!(i.members.contains(&info.event));
                    let source = if info.source_file.ends_with("lighting/sim/counted.rs") {
                        include_str!("counted.rs")
                    } else if info.source_file.ends_with("lighting/sim/pipeline.rs") {
                        include_str!("pipeline.rs")
                    } else if info.source_file.ends_with("lighting/sim/quantization.rs") {
                        include_str!("quantization.rs")
                    } else {
                        panic!("unexpected kernel location: {}", info.source_file);
                    };
                    assert!(source.lines().nth(info.source_line as usize - 1).is_some());
                }
            }
            for port in n.inputs.iter().chain(&n.outputs) {
                assert!(port.label.len() <= 28, "{}", port.label);
                assert!(!port.label.contains(['(', ')', '+', '×']));
            }
        }
        assert!(w.program.iter().any(|p| p.operator == "+"));
        assert!(w.program.iter().any(|p| p.operator == "*"));
    }
}
#[test]
fn single_pixel_attains_independent_critical_path_bound() {
    {
        let profile = LightingProfile::Fast;
        for full in [false, true] {
            let w = Workbench::new(profile, full).unwrap();
            let single = w.single_pixel();
            let mg = ModuloGraph::from_graph(&w.graph).unwrap();
            assert_eq!(single.span, mg.critical_path());
            assert!(single.span <= w.baseline.span);
            assert!(single.span <= MAX_ISSUE);
            for n in &w.nodes {
                for &parent in &n.parents {
                    let latency =
                        w.graph.resources[w.graph.nodes[parent].resource.unwrap()].latency;
                    assert!(single.issues[n.id] >= single.issues[parent] + latency);
                }
            }
        }
    }
}
#[test]
fn lit_queue_projection_bypasses_unlit_and_spreads_reads_only_in_slack() {
    for quantization in [
        LightingQuantization::CompensatedFloor,
        LightingQuantization::NearestEven,
    ] {
        {
            let profile = LightingProfile::Fast;
            for full in [false, true] {
                let w = Workbench::with_quantization(profile, full, quantization).unwrap();
                let old = w.single_pixel();
                let single = w.lit_queue_single_pixel();
                assert!(single.bypassed.is_empty());
                let mut specialized = w.graph.clone();
                for &id in &single.bypassed {
                    specialized.nodes[id].resource = None;
                }
                assert_eq!(
                    single.span,
                    ModuloGraph::from_graph(&specialized)
                        .unwrap()
                        .critical_path()
                );
                assert_eq!(single.span, old.span);
                for (id, n) in specialized.nodes.iter().enumerate() {
                    for &parent in &n.predecessors {
                        let latency = specialized.nodes[parent]
                            .resource
                            .map_or(0, |r| specialized.resources[r].latency);
                        assert!(single.issues[id] >= single.issues[parent] + latency);
                    }
                    let latency = n.resource.map_or(0, |r| specialized.resources[r].latency);
                    assert!(single.issues[id] + latency <= single.span);
                }
                if !full {
                    let square: Vec<_> = w
                        .nodes
                        .iter()
                        .filter(|n| n.read_memory.as_deref() == Some("SQ"))
                        .map(|n| single.issues[n.id])
                        .collect();
                    assert_eq!(square.len(), 3);
                    // A fused multi-output sum/address block can consume
                    // all three reads immediately, leaving no stagger slack.
                    // Read movement must not postpone that consumer.
                    for n in w
                        .nodes
                        .iter()
                        .filter(|n| n.read_memory.as_deref() == Some("SQ"))
                    {
                        assert!(single.issues[n.id] >= old.issues[n.id]);
                        for consumer in w.nodes.iter().filter(|m| m.parents.contains(&n.id)) {
                            assert!(
                                single.issues[n.id] + specialized.resources[n.resource].latency
                                    <= old.issues[consumer.id]
                            );
                        }
                    }
                }
            }
        }
    }
}
#[test]
fn finite_lifetimes_count_all_overlapping_instances() {
    assert_eq!(periodic_live(3, 8, 0, 2), 2);
    assert_eq!(periodic_live(3, 8, 1, 2), 3);
    assert_eq!(periodic_live(4, 4, 0, 2), 0);
}
#[test]
fn baselines_and_edits_use_independent_periodic_checker() {
    {
        let profile = LightingProfile::Fast;
        for full in [false, true] {
            let w = Workbench::new(profile, full).unwrap();
            let capacities: Vec<_> = w.graph.resources.iter().map(|r| r.lanes).collect();
            let mut slots = w.slots(&w.baseline);
            let ii = w.baseline.initiation_interval;
            let a = w.inspect(ii, &slots, &capacities).unwrap();
            assert!(a.conflicts.is_empty(), "{:?}", a.conflicts);
            assert_eq!(a.schedule, w.baseline);
            assert!(a.live_bits_by_phase.is_some());
            let consumer = w.nodes.iter().find(|n| !n.parents.is_empty()).unwrap();
            slots
                .iter_mut()
                .find(|s| s.id == consumer.id)
                .unwrap()
                .issue = 0;
            assert!(w
                .inspect(ii, &slots, &capacities)
                .unwrap()
                .conflicts
                .iter()
                .any(|c| c.message.contains("predecessor")));
            let mut slots = w.slots(&w.baseline);
            let resource = w
                .nodes
                .iter()
                .find(|n| w.nodes.iter().filter(|m| m.resource == n.resource).count() > 1)
                .unwrap()
                .resource;
            for s in &mut slots {
                if w.graph.nodes[s.id].resource == Some(resource) {
                    s.lane = 0;
                    s.issue = 0;
                }
            }
            assert!(w
                .inspect(ii, &slots, &capacities)
                .unwrap()
                .conflicts
                .iter()
                .any(|c| c.message.contains("lane")));
            slots[0].id = usize::MAX;
            assert!(w.inspect(ii, &slots, &capacities).is_err());
            assert!(w.search(ii, &capacities).unwrap().conflicts.is_empty());
        }
    }
}
