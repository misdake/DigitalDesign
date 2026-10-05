use audited::{Fixed, Model};
use gpu_v2::lighting::{
    ports::*,
    sim::{counted, oracle, pipeline, timed::*},
};

fn rne(value: i128, shift: u32) -> i128 {
    let unit = 1_i128 << shift;
    let floor = value.div_euclid(unit);
    let tail = value.rem_euclid(unit);
    floor + i128::from(2 * tail > unit || 2 * tail == unit && floor & 1 != 0)
}

#[test]
fn reusable_blocks_match_independent_integer_goldens_at_ties_and_saturation() {
    for b in [
        -16385_i128,
        -16384,
        -3,
        -2,
        -1,
        0,
        1,
        2,
        3,
        16383,
        16384,
        16385,
    ] {
        for tail in [0, 16383, 16384, 16385, 32767] {
            for gate in [None, Some(0), Some(1)] {
                let product = (b << 15) + tail;
                let mut m = Model::numerical();
                let p = m.input::<33, 29, true>("product", &[product]).unwrap();
                let z = m
                    .input::<1, 0, false>("zero", &[gate.unwrap_or(0)])
                    .unwrap();
                let f = m.compute("normal output", 128).unwrap();
                let p = f.read(p.at::<0>()).unwrap();
                let z = gate.map(|_| f.read(z.at::<0>()).unwrap());
                let (out, _) = pipeline::normalized_output(&f, p, z).unwrap();
                f.publish("out", out).unwrap();
                let frame = f.finish();
                frame.audit().unwrap();
                let expected = if gate == Some(1) {
                    0
                } else {
                    rne(product, 15).clamp(-16384, 16384)
                };
                assert_eq!(frame.outputs[0].raw, expected);
            }
        }
    }
    // All correction codes: the carry at 65535 must reach bit 8, not wrap.
    for correction in 0..=65535_i128 {
        let mut m = Model::numerical();
        let c = m
            .input::<16, 23, false>("correction", &[correction])
            .unwrap();
        let f = m.compute("reciprocal tail", 64).unwrap();
        let result =
            pipeline::reciprocal_tail(&f, Fixed::constant::<32768>(), f.read(c.at::<0>()).unwrap())
                .unwrap();
        f.publish("out", result).unwrap();
        let frame = f.finish();
        frame.audit().unwrap();
        assert_eq!(frame.outputs[0].raw, 32768 - rne(correction, 8));
    }
    for product in [
        0, 127, 128, 129, 383, 384, 385, 65279, 65280, 65407, 65408, 65535, 65536,
    ] {
        for ambient in [0, 1, 255, 256, 510, 511] {
            let mut m = Model::numerical();
            let p = m.input::<18, 16, false>("product", &[product]).unwrap();
            let a = m.input::<9, 8, false>("ambient", &[ambient]).unwrap();
            let f = m.compute("diffuse tail", 128).unwrap();
            let (out, _, _) = pipeline::diffuse_finish(
                &f,
                f.read(p.at::<0>()).unwrap(),
                f.read(a.at::<0>()).unwrap(),
            )
            .unwrap();
            f.publish("out", out).unwrap();
            let frame = f.finish();
            frame.audit().unwrap();
            assert_eq!(frame.outputs[0].raw, (ambient + rne(product, 8)).min(511));
        }
    }
}

fn pixels(n: usize) -> Vec<CompactPixelInput> {
    (0..n)
        .map(|i| CompactPixelInput {
            normal: [
                [377, -286, 939],
                [-2048, 2047, 1],
                [1, 0, -1],
                [0; 3],
                [1024, 0, 0],
            ][i % 5],
            ndc: [(i as i32 * 7919 % 131073) - 65536, 31457],
        })
        .collect()
}
fn make(full: bool, measured: bool, rom_latency: u64) -> Plan {
    let h = Hardware {
        measured_blocks: measured,
        kernel: counted::Config::compact(),
        cone_depth: 0,
        cone_latency: 1,
        rom_latency,
        ..Hardware::lighting_architecture_ii2()
    };
    plan_compact(
        &pixels(8),
        Material {
            specular_color: if full { [255; 3] } else { [0; 3] },
            ..Default::default()
        },
        Light::default(),
        Projection::default(),
        h,
        Storage::Registers,
        Strategy::Interleaved,
    )
    .unwrap()
}

#[test]
fn measured_blocks_preserve_rom_dsp_and_all_stage_goldens() {
    for full in [false, true] {
        let primitive = make(full, false, 1);
        let measured = make(full, true, 1);
        measured.audit().unwrap();
        assert!(!measured.logic_cones().is_empty());
        assert_eq!(measured.template.counts, primitive.template.counts);
        assert_eq!(measured.outputs, primitive.outputs);
        assert_eq!(
            measured.hardware.multiplier_half_slots(),
            primitive.hardware.multiplier_half_slots()
        );
        assert_eq!(measured.fused_groups(), primitive.fused_groups());
        for cone in measured.logic_cones() {
            cone.audit(&measured.template).unwrap();
            for id in std::iter::once(cone.result_event).chain(cone.absorbed_events.iter().copied())
            {
                assert!(!matches!(
                    measured.template.events[id].operation,
                    audited::Operation::Read { .. }
                        | audited::Operation::Multiply
                        | audited::Operation::ProductMapping { .. }
                ));
            }
        }
        for p in pixels(8) {
            let m = Material {
                specular_color: if full { [255; 3] } else { [0; 3] },
                ..Default::default()
            };
            let r = counted::evaluate_with_config(
                p.expanded().unwrap(),
                m,
                Light::default(),
                Projection::default(),
                2048,
                counted::Config::compact(),
            )
            .unwrap();
            let o = oracle::evaluate(
                p.expanded().unwrap(),
                m,
                Light::default(),
                Projection::default(),
                oracle::Config {
                    rounding: oracle::RoundingPolicy {
                        power: oracle::Rounding::Floor,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                r.frame
                    .outputs
                    .iter()
                    .map(|v| (v.name.clone(), v.raw))
                    .collect::<Vec<_>>(),
                o.stages
            );
        }
        let p = PeriodicSchedule::search(&primitive, 2, 8).unwrap();
        let candidate = PeriodicSchedule::search(&measured, 2, 8).unwrap();
        candidate.audit(&measured).unwrap();
        assert!(candidate.latency < p.latency);
        let live = candidate.retained_values(&measured).unwrap();
        let primitive_adders = p.adder_inventory(&primitive).unwrap();
        let measured_adders = candidate.adder_inventory(&measured).unwrap();
        assert_eq!(
            primitive_adders.normal_sites_by_width,
            measured_adders.normal_sites_by_width
        );
        assert_eq!(
            primitive_adders.increment_sites_by_width,
            measured_adders.increment_sites_by_width
        );
        println!(
            "full={full} primitive_latency={} measured_latency={} cones={} peak_bits={}",
            p.latency,
            candidate.latency,
            measured.logic_cones().len(),
            live.peak_bits
        );
        for rom in [1, 2] {
            let model = make(full, true, rom);
            for slot in &model.events {
                if matches!(
                    slot.kind,
                    Some(LaneKind::NormalizeRead | LaneKind::PowerRead | LaneKind::ContextRead)
                ) {
                    assert_eq!(slot.ready - slot.issue, rom);
                }
            }
        }
        let mut corrupt = candidate.clone();
        let root = measured.logic_cones()[0].result_event;
        corrupt.slots[root].ready = corrupt.slots[root].issue;
        assert!(corrupt.audit(&measured).is_err());
        let mut corrupt = candidate.clone();
        let member = measured.logic_cones()[0].absorbed_events[0];
        corrupt.slots[member].ready = 0;
        corrupt.slots[member].issue = 0;
        assert!(corrupt.audit(&measured).is_err());
    }
}

#[test]
fn measured_mode_rejects_arbitrary_depth_and_unbudgeted_lanes() {
    for (depth, latency, lanes) in [(4, 1, 3), (0, 0, 3), (0, 2, 3), (0, 1, 0), (0, 1, 17)] {
        let h = Hardware {
            cone_depth: depth,
            cone_latency: latency,
            cone_lanes_per_shape: lanes,
            ..Hardware::lighting_measured_ii2()
        };
        assert!(plan_compact(
            &pixels(1),
            Material::default(),
            Light::default(),
            Projection::default(),
            h,
            Storage::Registers,
            Strategy::Interleaved
        )
        .is_err());
    }
}

#[test]
fn measured_function_side_results_match_independent_coordinates() {
    // Threshold transitions and bounded restoration, independently computed.
    for q in [
        1 << 26,
        (1 << 27) - 1,
        1 << 27,
        (1 << 28) - 1,
        1 << 28,
        (1 << 29) - 1,
        1 << 29,
        3 << 28,
    ] {
        let mut m = Model::numerical();
        let input = m.input::<30, 28, false>("q", &[q]).unwrap();
        let f = m.compute("inverse head independent", 128).unwrap();
        let h = pipeline::inverse_head(&f, f.read(input.at::<0>()).unwrap()).unwrap();
        f.publish("address", h.address).unwrap();
        f.publish("fraction", h.fraction).unwrap();
        f.publish("restore", h.restore).unwrap();
        let frame = f.finish();
        frame.audit().unwrap();
        let z = (q as u32).leading_zeros() as i32 - 2;
        let exponent = 1 - z;
        let mantissa = q >> (15 - z);
        assert_eq!(
            frame.outputs.iter().map(|v| v.raw).collect::<Vec<_>>(),
            vec![
                i128::from(exponent & 1) * 64 + ((mantissa >> 8) & 63),
                mantissa & 255,
                i128::from(exponent < 0)
            ]
        );
    }
    let rows: Vec<Vec<i128>> = include_str!("../spec/power-segments.csv")
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| l.split(',').map(|s| s.parse().unwrap()).collect())
        .collect();
    let mut offset = 0;
    for row in rows {
        let (boundary, wide, fine) = (row[1], row[2], row[3]);
        // Golden segment walk; no dynamic shifting/select formula from the DUT.
        let mut starts = Vec::new();
        let mut p = 0;
        while p < 32768 {
            starts.push(p);
            p += 1 << if p < boundary { wide } else { fine };
        }
        let base_f = (offset + (boundary >> wide) - (boundary >> fine)) & 1023;
        let context = boundary | (wide << 15) | (fine << 19) | (offset << 23) | (base_f << 33);
        let mut xs = vec![0, 32767, boundary - 1, boundary, boundary + 1];
        xs.extend((0..64).map(|i| (i * 7919) % 32768));
        for x in xs.into_iter().filter(|&x| (0..32768).contains(&x)) {
            let segment = starts.partition_point(|&start| start <= x) - 1;
            let mut m = Model::numerical();
            let sx = m.input::<16, 15, false>("x", &[x]).unwrap();
            let ctx = m.input::<43, 0, false>("context", &[context]).unwrap();
            let f = m.compute("power coordinates independent", 128).unwrap();
            let h = pipeline::power_head(
                &f,
                f.read(sx.at::<0>()).unwrap(),
                f.read(ctx.at::<0>()).unwrap(),
            )
            .unwrap();
            f.publish("address", h.address).unwrap();
            f.publish("tail", h.tail).unwrap();
            f.publish("negative shift", h.neg).unwrap();
            let frame = f.finish();
            frame.audit().unwrap();
            let shift = if starts[segment] < boundary {
                wide
            } else {
                fine
            };
            assert_eq!(
                frame.outputs.iter().map(|v| v.raw).collect::<Vec<_>>(),
                vec![offset + segment as i128, x - starts[segment], -shift]
            );
        }
        offset += starts.len() as i128;
    }
    assert_eq!(offset, 886);
}

#[test]
fn multi_output_timed_compact_plan_preserves_outputs_and_rom_latencies() {
    for full in [false, true] {
        let material = Material {
            specular_color: if full { [255; 3] } else { [0; 3] },
            ..Default::default()
        };
        let old = make(full, false, 1);
        let measured = plan_compact(
            &pixels(8),
            material,
            Light::default(),
            Projection::default(),
            Hardware::lighting_functions_ii2(),
            Storage::Registers,
            Strategy::Interleaved,
        )
        .unwrap();
        measured.audit().unwrap();
        assert_eq!(measured.outputs, old.outputs);
        assert_eq!(measured.template.counts, old.template.counts);
        assert!(measured
            .logic_cones()
            .iter()
            .any(|c| !c.exported_events.is_empty()));
        assert!(measured
            .events
            .iter()
            .filter(|e| matches!(e.kind, Some(LaneKind::NormalizeRead | LaneKind::PowerRead)))
            .all(|e| e.ready - e.issue == 1));
    }
}
