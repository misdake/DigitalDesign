use gpu_v2::lighting::{
    calendars::UnifiedCalendar,
    emu::LightingEmu,
    rtl::{self, LightingRtlOptions},
    sim::workbench::{SchedulePlan, Workbench},
    LightingProfile, LightingQuantization,
};

fn plans() -> [SchedulePlan; 2] {
    [true, false].map(|full| {
        let w = Workbench::new(LightingProfile::Fast, full)
            .unwrap()
            .preconnected_modes();
        SchedulePlan {
            ii: 2,
            capacities: w.graph.resources.iter().map(|r| r.lanes).collect(),
            slots: w.slots(&w.baseline),
            preconnected_modes: true,
        }
    })
}

#[test]
fn unified_lit_rejects_independently_edited_diffuse_calendars() {
    let options = LightingRtlOptions {
        unified_lit: true,
        ..LightingRtlOptions::lit_queue_resource_profile(
            LightingProfile::Fast,
            LightingQuantization::CompensatedFloor,
        )
    };
    let edited = plans();
    assert!(rtl::generate_with_schedule_plans(LightingProfile::Fast, options, &edited).is_err());
    assert!(
        LightingEmu::with_schedule_plans(LightingProfile::Fast, options, &edited, 4096).is_err()
    );
}

#[test]
fn reviewed_calendars_bind_one_complete_program_and_pass_recurring_collision_checks() {
    for quantization in [
        LightingQuantization::CompensatedFloor,
        LightingQuantization::NearestEven,
    ] {
        assert_eq!(
            UnifiedCalendar::selected(quantization),
            if quantization == LightingQuantization::CompensatedFloor {
                UnifiedCalendar::Free
            } else {
                UnifiedCalendar::TwoEdge
            }
        );
        for calendar in [UnifiedCalendar::Free, UnifiedCalendar::TwoEdge] {
            let plans = calendar.plans(quantization).unwrap();
            let options = calendar.options(quantization);
            let rtl =
                rtl::generate_with_schedule_plans(LightingProfile::Fast, options, &plans).unwrap();
            assert_eq!((rtl.specular_ii, rtl.diffuse_ii), (2, 2));
            assert_eq!(rtl.latency, rtl.diffuse_latency);
            assert!(rtl::physical_calendar_with_schedule_plans(
                LightingProfile::Fast,
                options,
                &plans
            )
            .unwrap()
            .iter()
            .all(|op| op.full));
            LightingEmu::with_schedule_plans(LightingProfile::Fast, options, &plans, 4096).unwrap();
        }
    }
}

#[test]
fn input_mode_projection_preserves_numeric_comparisons_and_atomic_recipes() {
    for full in [false, true] {
        let original = Workbench::new(LightingProfile::Fast, full).unwrap();
        let projected = Workbench::new(LightingProfile::Fast, full)
            .unwrap()
            .preconnected_modes();
        assert!(original
            .nodes
            .iter()
            .any(|n| n.outputs.iter().any(|p| p.label.starts_with("mode."))));
        assert!(!projected
            .nodes
            .iter()
            .any(|n| n.outputs.iter().any(|p| p.label.starts_with("mode."))));
        assert!(projected
            .nodes
            .iter()
            .any(|n| n.steps.iter().any(|s| s == "compare")));
        for n in &projected.nodes {
            let before = original.nodes.iter().find(|b| b.id == n.id).unwrap();
            assert_eq!(n.recipe, before.recipe);
            assert_eq!(n.multipliers, before.multipliers);
        }
        let capacities = projected
            .graph
            .resources
            .iter()
            .map(|r| r.lanes)
            .collect::<Vec<_>>();
        assert!(projected
            .inspect(
                projected.baseline.initiation_interval,
                &projected.slots(&projected.baseline),
                &capacities
            )
            .unwrap()
            .conflicts
            .is_empty());
    }
}

#[test]
fn explicit_plan_rejects_duplicate_and_periodically_colliding_assignments() {
    let options = LightingRtlOptions::lit_queue_resource_profile(
        LightingProfile::Fast,
        LightingQuantization::CompensatedFloor,
    );
    let mut duplicate = plans();
    duplicate[0].slots[1] = duplicate[0].slots[0];
    assert!(rtl::generate_with_schedule_plans(LightingProfile::Fast, options, &duplicate).is_err());
    assert!(
        LightingEmu::with_schedule_plans(LightingProfile::Fast, options, &duplicate, 4096).is_err()
    );
    let mut colliding = plans();
    let w = Workbench::new(LightingProfile::Fast, true)
        .unwrap()
        .preconnected_modes();
    let multiply = w
        .nodes
        .iter()
        .filter(|n| w.graph.resources[n.resource].name == "SmallMultiply")
        .take(2)
        .map(|n| n.id)
        .collect::<Vec<_>>();
    for slot in &mut colliding[0].slots {
        if multiply.contains(&slot.id) {
            slot.issue = 100;
            slot.lane = 0;
        }
    }
    assert!(rtl::generate_with_schedule_plans(LightingProfile::Fast, options, &colliding).is_err());
}

#[test]
fn explicit_diffuse_ii2_is_used_by_both_executor_and_hdl_calendar() {
    let options = LightingRtlOptions::lit_queue_resource_profile(
        LightingProfile::Fast,
        LightingQuantization::CompensatedFloor,
    );
    let edited = plans();
    let r = rtl::generate_with_schedule_plans(LightingProfile::Fast, options, &edited).unwrap();
    assert_eq!((r.specular_ii, r.diffuse_ii), (2, 2));
    LightingEmu::with_schedule_plans(LightingProfile::Fast, options, &edited, 4096).unwrap();
    // The original public constructor keeps its separately qualified II1 path.
    assert_eq!(
        rtl::generate_with_options(LightingProfile::Fast, options)
            .unwrap()
            .diffuse_ii,
        1
    );
}

#[test]
fn shared_prefix_cannot_be_enabled_on_the_unedited_ii1_calendar() {
    let options = LightingRtlOptions {
        shared_prefix: true,
        ..LightingRtlOptions::lit_queue_resource_profile(
            LightingProfile::Fast,
            LightingQuantization::CompensatedFloor,
        )
    };
    assert!(rtl::generate_with_options(LightingProfile::Fast, options)
        .err()
        .unwrap()
        .contains("explicit II=2"));
}
