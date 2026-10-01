use super::*;

fn limits() -> Limits {
    Limits {
        max_cycle: 128,
        max_events: 1024,
    }
}
fn ports() -> PortShape {
    PortShape {
        read_ports: 1,
        write_ports: 1,
        read_latency: 2,
        max_reads_per_frame: 16,
        max_writes_per_frame: 16,
    }
}

#[test]
fn triangle_and_non_power_of_two_rcp_have_independent_host_goldens() {
    for route in [ProductRoute::Native18Pair, ProductRoute::Wide36] {
        let report = triangle::run(route).unwrap();
        assert!(report.valid, "{:?}", report.faults);
        report.audit().unwrap();
        let raw = |name: &str| report.outputs.iter().find(|o| o.name == name).unwrap().raw;
        // Independent geometry: side lengths four, and barycentrics (1/2,1/4,1/4).
        assert_eq!(raw("area_q8"), 16 * 256);
        assert_eq!(raw("weighted_q16"), 7 * 65536);
        assert_eq!(raw("sample_q8"), 112);
        let n = 2 * 4096;
        let d = 3;
        let exact = (n * 2 + d / 2) / d;
        assert!((raw("two_over_one_point_five_q12") - exact).abs() <= 1);
        assert_eq!(report.hardware.dsp18_units(), 6);
        assert_eq!(report.counts.resources.get(&Resource::Adder(18)), Some(&13));
        assert_eq!(report.counts.resources[&Resource::LeadingZeros(18)], 1);
        assert_eq!(report.counts.resources[&Resource::Shift(18)], 1);
        assert_eq!(report.counts.resources[&Resource::Shift(54)], 1);
        assert!(report.counts.resources[&Resource::Adder(36)] >= 11);
        assert!(report.counts.resources[&Resource::Adder(54)] >= 2);
        assert_eq!(report.counts.logical_products[&(36, 18)], 6);
        assert_eq!(report.counts.resources[&Resource::Read(4)], 2);
        assert!(report
            .events
            .iter()
            .any(|e| matches!(e.operation, Operation::RoundIncrement(_))));
        assert!(report
            .events
            .iter()
            .any(|e| matches!(e.operation, Operation::ProductMapping { .. })));
    }
}

#[test]
fn native_pair_is_two_real_dsp_issues_plus_a_54_bit_add() {
    let mut model = Model::new(Hardware::one_wide_two_narrow()).unwrap();
    let frame = model.begin_frame("mixed_signed_product", limits());
    let value = frame
        .mul::<54, 0, true>(
            Fixed::<36, 0, true>::constant::<-34359738368>(),
            Fixed::<18, 0, true>::constant::<-131072>(),
            ProductRoute::Native18Pair,
        )
        .unwrap();
    frame.publish("product", value).unwrap();
    let report = frame.finish();
    assert!(report.valid, "{:?}", report.faults);
    assert_eq!(report.outputs[0].raw, 1_i128 << 52);
    assert_eq!(report.counts.resources[&Resource::Dsp18], 2);
    assert!(!report.counts.resources.contains_key(&Resource::Dsp36));
    assert_eq!(report.counts.resources[&Resource::Adder(54)], 1);
    assert_eq!(report.cycles, 3);
    assert_eq!(report.issue_histogram[&(Resource::Dsp18, 0)], 2);
}

#[test]
fn wide_route_uses_a_real_36_by_36_port_and_returns_72_bits() {
    let mut model = Model::new(Hardware::one_wide_two_narrow()).unwrap();
    let frame = model.begin_frame("wide_product", limits());
    let value = frame
        .mul::<54, 0, true>(
            Fixed::<36, 0, true>::constant::<34359738367>(),
            Fixed::<18, 0, true>::constant::<131071>(),
            ProductRoute::Wide36,
        )
        .unwrap();
    frame.publish("product", value).unwrap();
    let report = frame.finish();
    assert!(report.valid, "{:?}", report.faults);
    assert_eq!(report.counts.resources[&Resource::Dsp36], 1);
    assert!(!report.counts.resources.contains_key(&Resource::Dsp18));
    let dsp = report
        .events
        .iter()
        .find(|e| e.resource == Some(Resource::Dsp36))
        .unwrap();
    assert!(dsp
        .inputs
        .iter()
        .all(|&v| report.values[v].format.bits == 36));
    assert_eq!(report.values[dsp.output.unwrap()].format.bits, 72);
    assert_eq!(report.cycles, 3);
}

#[test]
fn negative_ties_round_to_even_and_increment_is_a_counted_adder() {
    let mut hardware = Hardware::one_wide_two_narrow();
    hardware
        .units
        .insert(Resource::RoundControl(18), Unit::pipelined(1, 1));
    let mut model = Model::new(hardware).unwrap();
    let frame = model.begin_frame("signed_ties", limits());
    frame
        .publish(
            "minus_one_half",
            frame
                .round_to::<18, 0, true>(Fixed::<18, 1, true>::constant::<-3>())
                .unwrap(),
        )
        .unwrap();
    frame
        .publish(
            "minus_two_half",
            frame
                .round_to::<18, 0, true>(Fixed::<18, 1, true>::constant::<-5>())
                .unwrap(),
        )
        .unwrap();
    let report = frame.finish();
    assert!(report.valid, "{:?}", report.faults);
    assert_eq!(
        report.outputs.iter().map(|v| v.raw).collect::<Vec<_>>(),
        [-2, -2]
    );
    assert_eq!(report.counts.resources[&Resource::Adder(18)], 2);
    assert_eq!(report.counts.resources[&Resource::RoundControl(18)], 2);
}

#[test]
fn ports_preserve_read_before_write_hazards_and_report_payload_and_cycle() {
    let mut model = Model::new(Hardware::one_wide_two_narrow()).unwrap();
    let m = model.ram::<18, 0, true>("scratch", 1, ports()).unwrap();
    let frame = model.begin_frame("port_hazards", limits());
    frame
        .write(m.at::<0>(), Fixed::<18, 0, true>::constant::<37>())
        .unwrap();
    let old = frame.read(m.at::<0>()).unwrap();
    frame
        .write(m.at::<0>(), Fixed::<18, 0, true>::constant::<91>())
        .unwrap();
    let new = frame.read(m.at::<0>()).unwrap();
    frame.publish("old", old).unwrap();
    frame.publish("new", new).unwrap();
    let report = frame.finish();
    assert!(report.valid, "{:?}", report.faults);
    assert_eq!(
        report.outputs.iter().map(|v| v.raw).collect::<Vec<_>>(),
        [37, 91]
    );
    assert_eq!(report.counts.read_bits[&0], 36);
    assert_eq!(report.counts.write_bits[&0], 36);
    assert_eq!(report.cycles, 6);
    let writes = report
        .events
        .iter()
        .filter(|e| e.resource == Some(Resource::Write(0)))
        .collect::<Vec<_>>();
    let reads = report
        .events
        .iter()
        .filter(|e| e.resource == Some(Resource::Read(0)))
        .collect::<Vec<_>>();
    assert!(writes[1].issue_cycle >= reads[0].ready_cycle);
    assert!(reads[1].issue_cycle >= writes[1].ready_cycle);
}

#[test]
fn frame_io_budget_is_a_failure_not_silently_extra_bandwidth() {
    let mut model = Model::new(Hardware::one_wide_two_narrow()).unwrap();
    let m = model
        .ram::<18, 0, true>(
            "limited",
            1,
            PortShape {
                max_reads_per_frame: 1,
                ..ports()
            },
        )
        .unwrap();
    let frame = model.begin_frame("io_limit", limits());
    frame
        .write(m.at::<0>(), Fixed::<18, 0, true>::constant::<5>())
        .unwrap();
    frame.read(m.at::<0>()).unwrap();
    assert!(matches!(
        frame.read(m.at::<0>()),
        Err(Fault::PortFrameLimit)
    ));
    let report = frame.finish();
    assert!(!report.valid);
    assert_eq!(report.counts.resources[&Resource::Read(0)], 1);
    assert_eq!(report.faults, [Fault::PortFrameLimit]);
}

#[test]
fn values_cannot_cross_frames_but_stored_bits_can_cross_a_counted_port() {
    let mut model = Model::new(Hardware::one_wide_two_narrow()).unwrap();
    let m = model.ram::<18, 0, true>("persistent", 1, ports()).unwrap();
    let first = model.begin_frame("first", limits());
    let old = first
        .add::<18, 0, true>(
            Fixed::<18, 0, true>::constant::<4>(),
            Fixed::<18, 0, true>::constant::<7>(),
        )
        .unwrap();
    first.write(m.at::<0>(), old).unwrap();
    assert!(first.finish().valid);
    let second = model.begin_frame("second", limits());
    assert!(matches!(
        second.add::<18, 0, true>(old, Fixed::<18, 0, true>::constant::<1>()),
        Err(Fault::ForeignValue)
    ));
    let current = second.read(m.at::<0>()).unwrap();
    second.publish("loaded", current).unwrap();
    let report = second.finish();
    assert!(!report.valid);
    assert_eq!(report.outputs[0].raw, 11);
}

#[test]
fn audit_rejects_correct_numbers_when_the_resource_or_counter_is_missing() {
    let good = triangle::run(ProductRoute::Native18Pair).unwrap();
    assert!(good.valid);
    let mut missing = good.clone();
    let adder = missing
        .events
        .iter_mut()
        .find(|e| e.resource == Some(Resource::Adder(54)))
        .unwrap();
    adder.resource = None;
    adder.lane = None;
    adder.ready_cycle = adder.issue_cycle;
    assert!(missing.audit().is_err());
    let mut missing = good.clone();
    *missing.counts.resources.get_mut(&Resource::Dsp18).unwrap() -= 1;
    assert!(missing.audit().is_err());
    let mut missing = good.clone();
    let read = missing
        .events
        .iter()
        .find(|e| matches!(e.operation, Operation::Read { .. }))
        .unwrap()
        .output
        .unwrap();
    missing.values[read].raw += 1;
    assert!(missing.audit().is_err());
    let mut missing = good.clone();
    missing.cycles = 1;
    assert!(missing.audit().is_err());
}

#[test]
fn signed_partial_products_cover_both_pin_extremes_and_nonzero_low_parts() {
    fn case<const A: i128, const B: i128>() {
        for route in [ProductRoute::Native18Pair, ProductRoute::Wide36] {
            let mut model = Model::new(Hardware::one_wide_two_narrow()).unwrap();
            let frame = model.begin_frame("product_corner", limits());
            let p = frame
                .mul::<54, 0, true>(
                    Fixed::<36, 0, true>::constant::<A>(),
                    Fixed::<18, 0, true>::constant::<B>(),
                    route,
                )
                .unwrap();
            frame.publish("product", p).unwrap();
            let report = frame.finish();
            assert!(report.valid, "{:?}", report.faults);
            assert_eq!(report.outputs[0].raw, A * B);
        }
    }
    case::<{ -34359738368 }, { -131072 }>();
    case::<34359738367, 131071>();
    case::<{ -34359738367 }, 131071>();
    case::<34359738367, { -131072 }>();
    case::<{ -1 }, { -1 }>();
    case::<262143, { -17 }>();
}

#[test]
fn a_nineteen_bit_add_must_use_the_declared_36_bit_adder() {
    let mut model = Model::new(Hardware::one_wide_two_narrow()).unwrap();
    let frame = model.begin_frame("width_promotion", limits());
    let p = frame
        .add::<19, 0, true>(
            Fixed::<18, 0, true>::constant::<131071>(),
            Fixed::<18, 0, true>::constant::<1>(),
        )
        .unwrap();
    frame.publish("sum", p).unwrap();
    let report = frame.finish();
    assert!(report.valid);
    assert_eq!(report.outputs[0].raw, 131072);
    assert_eq!(report.counts.resources[&Resource::Adder(36)], 1);
    assert!(!report.counts.resources.contains_key(&Resource::Adder(18)));
}

#[test]
fn address_conversion_failure_and_event_limit_are_recorded() {
    let mut model = Model::new(Hardware::one_wide_two_narrow()).unwrap();
    let m = model.ram::<18, 0, true>("one_row", 1, ports()).unwrap();
    let frame = model.begin_frame("address_limit", limits());
    assert!(matches!(
        frame.read(m.indexed(Fixed::<126, 0, false>::constant::<
            1267650600228229401496703205376,
        >())),
        Err(Fault::Address)
    ));
    assert!(!frame.finish().valid);
    let frame = model.begin_frame(
        "event_limit",
        Limits {
            max_events: 1,
            ..limits()
        },
    );
    assert!(matches!(
        frame.add::<18, 0, true>(
            Fixed::<18, 0, true>::constant::<1>(),
            Fixed::<18, 0, true>::constant::<2>()
        ),
        Err(Fault::EventLimit)
    ));
    assert!(!frame.finish().valid);
}

#[test]
fn overflow_uninitialized_memory_missing_adder_and_deadline_invalidate_frames() {
    let mut model = Model::new(Hardware::one_wide_two_narrow()).unwrap();
    let m = model.ram::<18, 0, true>("empty", 1, ports()).unwrap();
    let frame = model.begin_frame("overflow", limits());
    assert!(matches!(
        frame.add::<18, 0, true>(
            Fixed::<18, 0, true>::constant::<131071>(),
            Fixed::<18, 0, true>::constant::<1>()
        ),
        Err(Fault::Range)
    ));
    assert!(!frame.finish().valid);
    let frame = model.begin_frame("uninitialized", limits());
    assert!(matches!(frame.read(m.at::<0>()), Err(Fault::Uninitialized)));
    assert!(!frame.finish().valid);
    let frame = model.begin_frame(
        "deadline",
        Limits {
            max_cycle: 1,
            ..limits()
        },
    );
    assert!(matches!(
        frame.mul::<36, 0, true>(
            Fixed::<18, 0, true>::constant::<2>(),
            Fixed::<18, 0, true>::constant::<3>(),
            ProductRoute::Native18
        ),
        Err(Fault::Deadline)
    ));
    assert!(!frame.finish().valid);
    let frame = model.begin_frame("no_72_bit_adder", limits());
    assert!(matches!(
        frame.add::<72, 0, true>(
            Fixed::<72, 0, true>::constant::<1>(),
            Fixed::<72, 0, true>::constant::<2>()
        ),
        Err(Fault::MissingResource)
    ));
    assert!(!frame.finish().valid);
}

#[test]
fn independent_width_adders_can_issue_together_and_every_width_is_counted() {
    let mut model = Model::new(Hardware::one_wide_two_narrow()).unwrap();
    let frame = model.begin_frame("widths", limits());
    let a = frame
        .add::<18, 0, true>(
            Fixed::<18, 0, true>::constant::<1>(),
            Fixed::<18, 0, true>::constant::<2>(),
        )
        .unwrap();
    let b = frame
        .sub::<36, 0, true>(
            Fixed::<36, 0, true>::constant::<1000000>(),
            Fixed::<36, 0, true>::constant::<3>(),
        )
        .unwrap();
    let c = frame
        .add::<54, 0, true>(
            Fixed::<54, 0, true>::constant::<1125899906842624>(),
            Fixed::<54, 0, true>::constant::<11>(),
        )
        .unwrap();
    frame.publish("18", a).unwrap();
    frame.publish("36", b).unwrap();
    frame.publish("54", c).unwrap();
    let report = frame.finish();
    assert!(report.valid);
    for width in [18, 36, 54] {
        assert_eq!(report.counts.resources[&Resource::Adder(width)], 1);
        assert_eq!(report.issue_histogram[&(Resource::Adder(width), 0)], 1);
    }
    assert_eq!(report.cycles, 1);
}

#[test]
fn indexed_reads_and_writes_keep_address_dependency_and_format() {
    let mut model = Model::new(Hardware::one_wide_two_narrow()).unwrap();
    let m = model.ram::<18, 4, true>("indexed", 2, ports()).unwrap();
    let frame = model.begin_frame("indexed_io", limits());
    let address = frame
        .add_same(
            Fixed::<18, 0, false>::constant::<0>(),
            Fixed::<18, 0, false>::constant::<1>(),
        )
        .unwrap();
    frame
        .write(m.indexed(address), Fixed::<18, 4, true>::constant::<37>())
        .unwrap();
    let value = frame.read(m.indexed(address)).unwrap();
    frame.publish("loaded", value).unwrap();
    let good = frame.finish();
    assert!(good.valid, "{:?}", good.faults);
    assert_eq!(good.outputs[0].raw, 37);
    assert_eq!(good.outputs[0].format, Fixed::<18, 4, true>::FORMAT);
    assert_eq!(good.counts.read_bits[&0], 18);
    assert_eq!(good.counts.write_bits[&0], 18);
    let write = good
        .events
        .iter()
        .find(|e| e.resource == Some(Resource::Write(0)))
        .unwrap();
    let read = good
        .events
        .iter()
        .find(|e| e.resource == Some(Resource::Read(0)))
        .unwrap();
    assert_eq!(write.inputs.len(), 2);
    assert_eq!(read.inputs.len(), 1);
    assert_eq!(write.inputs[1], read.inputs[0]);
    assert!(write.issue_cycle >= good.values[write.inputs[1]].ready_cycle);
    assert!(read.issue_cycle >= write.ready_cycle);
    let mut forged = good.clone();
    let write = forged
        .events
        .iter_mut()
        .find(|e| e.resource == Some(Resource::Write(0)))
        .unwrap();
    write.operation = Operation::Write { memory: 0, row: 0 };
    assert!(forged.audit().is_err());
}

#[test]
fn addresses_reject_foreign_memory_old_values_and_invalid_rows() {
    let mut other = Model::new(Hardware::one_wide_two_narrow()).unwrap();
    let foreign = other.ram::<18, 0, true>("foreign", 2, ports()).unwrap();
    let mut model = Model::new(Hardware::one_wide_two_narrow()).unwrap();
    let m = model.ram::<18, 0, true>("local", 2, ports()).unwrap();
    let first = model.begin_frame("first", limits());
    let stale = first
        .add_same(
            Fixed::<18, 0, false>::constant::<0>(),
            Fixed::<18, 0, false>::constant::<1>(),
        )
        .unwrap();
    assert!(first.finish().valid);
    let frame = model.begin_frame("bad_addresses", limits());
    let v = Fixed::<18, 0, true>::constant::<37>();
    assert!(matches!(
        frame.write(foreign.at::<0>(), v),
        Err(Fault::ForeignMemory)
    ));
    assert!(matches!(
        frame.write(m.indexed(stale), v),
        Err(Fault::ForeignValue)
    ));
    assert!(matches!(frame.write(m.at::<2>(), v), Err(Fault::Address)));
    assert!(matches!(
        frame.write(m.indexed(Fixed::<18, 0, true>::constant::<-1>()), v),
        Err(Fault::Address)
    ));
    assert!(!frame.finish().valid);
}

#[test]
fn a_later_api_read_scheduled_earlier_cannot_erase_the_longer_read_hazard() {
    let mut model = Model::new(Hardware::one_wide_two_narrow()).unwrap();
    let m = model
        .ram::<18, 0, true>(
            "two_read_lanes",
            1,
            PortShape {
                read_ports: 2,
                ..ports()
            },
        )
        .unwrap();
    let frame = model.begin_frame("out_of_order_reads", limits());
    frame
        .write(m.at::<0>(), Fixed::<18, 0, true>::constant::<37>())
        .unwrap();
    let mut address = Fixed::<18, 0, false>::constant::<0>();
    for _ in 0..6 {
        address = frame
            .add_same(address, Fixed::<18, 0, false>::constant::<0>())
            .unwrap();
    }
    let later_return = frame.read(m.indexed(address)).unwrap();
    let earlier_return = frame.read(m.at::<0>()).unwrap();
    frame
        .write(m.at::<0>(), Fixed::<18, 0, true>::constant::<91>())
        .unwrap();
    frame.publish("late", later_return).unwrap();
    frame.publish("early", earlier_return).unwrap();
    let current = frame.read(m.at::<0>()).unwrap();
    frame.publish("new", current).unwrap();
    let report = frame.finish();
    assert!(report.valid, "{:?}", report.faults);
    assert_eq!(
        report.outputs.iter().map(|v| v.raw).collect::<Vec<_>>(),
        [37, 37, 91]
    );
    let reads = report
        .events
        .iter()
        .filter(|e| e.resource == Some(Resource::Read(0)))
        .collect::<Vec<_>>();
    let rewrite = report
        .events
        .iter()
        .filter(|e| e.resource == Some(Resource::Write(0)))
        .nth(1)
        .unwrap();
    assert!(reads[0].ready_cycle > reads[1].ready_cycle);
    assert!(rewrite.issue_cycle >= reads[0].ready_cycle);
}

#[test]
fn selection_is_clocked_and_missing_resource_cannot_be_hidden_as_wiring() {
    let mut model = Model::new(Hardware::one_wide_two_narrow()).unwrap();
    let frame = model.begin_frame("select", limits());
    let p = frame
        .less(
            Fixed::<18, 0, true>::constant::<1>(),
            Fixed::<18, 0, true>::constant::<2>(),
        )
        .unwrap();
    let value = frame
        .select(
            p,
            Fixed::<36, 8, true>::constant::<37>(),
            Fixed::<36, 8, true>::constant::<91>(),
        )
        .unwrap();
    frame.publish("selected", value).unwrap();
    let good = frame.finish();
    assert!(good.valid);
    assert_eq!(good.outputs[0].raw, 37);
    assert_eq!(good.counts.resources[&Resource::Select(36)], 1);
    assert_eq!(good.cycles, 2);
    let mut forged = good.clone();
    let select = forged
        .events
        .iter_mut()
        .find(|e| e.operation == Operation::Select)
        .unwrap();
    select.resource = None;
    select.lane = None;
    select.ready_cycle = select.issue_cycle;
    assert!(forged.audit().is_err());
    let mut hardware = Hardware::one_wide_two_narrow();
    hardware
        .units
        .retain(|r, _| !matches!(r, Resource::Select(_)));
    let mut model = Model::new(hardware).unwrap();
    let frame = model.begin_frame("no_selector", limits());
    assert!(matches!(
        frame.select(
            Fixed::<1, 0, false>::constant::<0>(),
            Fixed::<18, 0, true>::constant::<37>(),
            Fixed::<18, 0, true>::constant::<91>()
        ),
        Err(Fault::MissingResource)
    ));
    assert!(!frame.finish().valid);
}

#[test]
fn positive_normalization_handles_exponents_and_u18_boundaries() {
    fn case<const RAW: i128, const F: u32>() {
        let mut model = Model::new(Hardware::one_wide_two_narrow()).unwrap();
        let frame = model.begin_frame("normalize", limits());
        let (mantissa, exponent) = frame
            .normalize_positive(Fixed::<18, F, false>::constant::<RAW>())
            .unwrap();
        frame.publish("mantissa", mantissa).unwrap();
        frame.publish("exponent", exponent).unwrap();
        let report = frame.finish();
        assert!(report.valid, "{:?}", report.faults);
        let m = report.outputs[0].raw;
        let e = report.outputs[1].raw;
        let value = RAW as f64 * 2_f64.powi(-(F as i32));
        assert_eq!(e as i32, value.log2().floor() as i32);
        assert!((65536..131072).contains(&m));
        let reconstructed_raw = m as f64 / 65536.0 * 2_f64.powi(e as i32 + F as i32);
        assert!((reconstructed_raw - RAW as f64).abs() <= 1.0);
        assert_eq!(report.counts.resources[&Resource::LeadingZeros(18)], 1);
        assert_eq!(report.counts.resources[&Resource::Shift(18)], 1);
        assert_eq!(report.counts.resources[&Resource::Adder(18)], 2);
    }
    case::<1, 8>();
    case::<3, 8>();
    case::<4096, 8>();
    case::<6001, 8>();
    case::<65535, 8>();
    case::<65536, 8>();
    case::<131071, 8>();
    case::<131072, 8>();
    case::<262143, 8>();
    case::<3, 0>();
    case::<3, 16>();
    case::<3, 24>();
    let mut model = Model::new(Hardware::one_wide_two_narrow()).unwrap();
    let frame = model.begin_frame("zero", limits());
    assert!(matches!(
        frame.normalize_positive(Fixed::<18, 8, false>::constant::<0>()),
        Err(Fault::Range)
    ));
    assert!(!frame.finish().valid);
}

#[test]
fn dynamic_shift_sign_overflow_and_resource_audit() {
    let mut model = Model::new(Hardware::one_wide_two_narrow()).unwrap();
    let frame = model.begin_frame("shift", limits());
    let amount = frame
        .sub_same(
            Fixed::<18, 0, true>::constant::<1>(),
            Fixed::<18, 0, true>::constant::<2>(),
        )
        .unwrap();
    let value = frame
        .shift(Fixed::<36, 0, true>::constant::<-3>(), amount)
        .unwrap();
    frame.publish("shifted", value).unwrap();
    let good = frame.finish();
    assert!(good.valid);
    assert_eq!(good.outputs[0].raw, -2);
    assert_eq!(good.counts.resources[&Resource::Shift(36)], 1);
    assert_eq!(good.cycles, 2);
    let mut forged = good.clone();
    forged.values[forged.outputs[0].value].raw = -1;
    assert!(forged.audit().is_err());
    let frame = model.begin_frame("overflow", limits());
    assert!(matches!(
        frame.shift(
            Fixed::<18, 0, false>::constant::<262143>(),
            Fixed::<18, 0, true>::constant::<1>()
        ),
        Err(Fault::Range)
    ));
    assert!(!frame.finish().valid);
    let mut hardware = Hardware::one_wide_two_narrow();
    hardware
        .units
        .retain(|r, _| !matches!(r, Resource::Shift(_)));
    let mut model = Model::new(hardware).unwrap();
    let frame = model.begin_frame("no_shifter", limits());
    assert!(matches!(
        frame.shift(
            Fixed::<18, 0, true>::constant::<37>(),
            Fixed::<18, 0, true>::constant::<0>()
        ),
        Err(Fault::MissingResource)
    ));
    assert!(!frame.finish().valid);
}
