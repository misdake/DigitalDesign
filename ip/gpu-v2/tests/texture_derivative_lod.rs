//! Independent D/LOD contracts and bounded registered-calendar qualification.
use audited::FrameReport;
use gpu_v2::texture::emu::{
    derivative::{self, Calendar, DerivativeEmu, Field},
    lod::{self, LodEmu},
};
use gpu_v2::texture::{ports::*, sim::staged::bound};
use std::io::Write;

const D_OUTPUTS: &[&str] = &[
    "uv0", "uv1", "uv2", "uv3", "uv4", "uv5", "uv6", "uv7", "slope", "bias", "quad", "mask",
    "slot", "max_n", "has_mip", "filter",
];
const L_OUTPUTS: &[&str] = &[
    "shift0",
    "nearest",
    "halve",
    "side0",
    "side1",
    "parent0",
    "parent1",
    "n0",
    "n1",
    "last_fine",
    "quad",
    "mask",
    "slot",
];
fn calendar(
    plan: &bound::StagePlan,
    frame: &FrameReport,
    outputs: &[&str],
    poison: bool,
) -> Calendar {
    let evidence = gpu_v2::texture::sim::staged::binding::Evidence::build(frame).unwrap();
    let mut cones = evidence.lowering.logic_cones(frame, 1).unwrap();
    if frame.name == "texture_lod_context" {
        let shared = gpu_v2::texture::sim::staged::binding::lod_shared_h_cone(frame).unwrap();
        for id in [shared.absorbed_events[0], shared.result_event] {
            cones.push(audited::physical::LogicCone::singleton(frame, id, 1).unwrap());
        }
    }
    let fields = plan
        .packed_fields
        .iter()
        .map(|f| Field {
            value: f.value,
            source_low: f.source_low,
            width: f.width,
            birth: f.birth as u8,
            last_read: f.last_read as u8,
            lows: (0..plan.packed.period / plan.ii())
                .map(|i| {
                    plan.packed
                        .placements
                        .iter()
                        .find(|p| {
                            p.value == f.value && p.source_low == f.source_low && p.iteration == i
                        })
                        .unwrap()
                        .low
                })
                .collect(),
        })
        .collect();
    let mut frame = frame.clone();
    if poison {
        for value in &mut frame.values {
            if frame.events[value.producer].operation != audited::Operation::Literal {
                value.raw ^= 12345;
            }
        }
        for value in &mut frame.outputs {
            value.raw ^= 12345;
        }
    }
    Calendar::from_structure(
        &frame,
        &plan.times,
        &cones,
        &evidence.lowering.wiring_adds,
        fields,
        plan.packed.ff_bits,
        plan.span() as u8,
        plan.packed.period as u32,
        plan.ii() as u32,
        outputs,
    )
    .unwrap()
}
fn canonical() -> bound::Preparation {
    let mut q = QuadInput {
        force_coarsest: false,
        quad_id: 0,
        mask: 1,
        uv: [[0.003, 0.003]; 4],
        slot: 0,
        material_size_log2: 9,
        filter: Filter::Trilinear,
        lod_bias: 0.5,
    };
    q.uv[1][0] += 1.0 / 512.0;
    bound::prepare(
        &q,
        &[Slot {
            base_address: 4096,
            max_size_log2: 9,
            has_full_mip: true,
            valid: true,
        }],
    )
    .unwrap()
}
fn d_golden(i: derivative::Input) -> derivative::Output {
    let slope = [(0, 1), (2, 3), (0, 2), (1, 3)]
        .into_iter()
        .flat_map(|(a, b)| {
            (0..2).map(move |axis| (i.uv[2 * b + axis] - i.uv[2 * a + axis]).unsigned_abs())
        })
        .max()
        .unwrap();
    derivative::Output {
        uv: i.uv.map(|v| v.rem_euclid(1 << 16) as u32),
        slope: if i.force_coarsest { 131073 } else { slope },
        bias: i.bias,
        header: i.header,
    }
}
fn l_golden(i: lod::Input) -> lod::Output {
    let maximum = if i.header.has_mip {
        i32::from(i.header.max_n) * 256
    } else {
        0
    };
    let l = if i.slope == 0 {
        0
    } else if i.slope > 131072 {
        maximum
    } else {
        let h = 63 - i.slope.leading_zeros();
        let norm = i.slope << (19 - h);
        let tail = norm & ((1 << 19) - 1);
        let quotient = tail / 8192;
        let remainder = tail % 8192;
        let k = quotient
            + u64::from(remainder > 4096 || remainder == 4096 && !quotient.is_multiple_of(2));
        let exponent = i32::from(i.header.max_n) + h as i32 - 16 + i32::from(k == 64);
        let log = ((1.0 + (k % 64) as f64 / 64.0).log2() * 256.0).round_ties_even() as i32;
        (256 * exponent + log + i32::from(i.bias)).clamp(0, maximum)
    };
    let fine = i.header.max_n - (l / 256) as u8;
    let levels = [fine, fine.saturating_sub(1)];
    let fraction = l % 256;
    let lambda = if i.header.filter == 2 {
        (2 * fraction - i32::from(fraction > 128)) as u16
    } else {
        0
    };
    lod::Output {
        context: lod::CoordinateContext {
            shift: i32::from(fine.max(1)) - 8,
            nearest: i.header.filter == 0,
            halve: fine > 1,
            side: levels.map(|n| 1_i16 << n.max(1)),
            parents: [511 - lambda, lambda],
            levels,
            last_fine: lambda == 0,
        },
        quad: i.header.quad,
        mask: i.header.mask,
        slot: i.header.slot,
    }
}
fn stimulus(slope: u64, id: usize) -> (QuadInput, Slot) {
    let slot = Slot {
        base_address: 4096,
        max_size_log2: (id % 11) as u8,
        has_full_mip: id.is_multiple_of(2),
        valid: true,
    };
    let mut q = QuadInput {
        force_coarsest: false,
        quad_id: (id % 16) as u8,
        mask: 1,
        uv: [[-0.75, 0.125]; 4],
        slot: 0,
        material_size_log2: slot.max_size_log2,
        filter: [Filter::Nearest, Filter::Bilinear, Filter::Trilinear][id % 3],
        lod_bias: [-32.0, -0.5, 0.0, 0.5, 32.0][id % 5],
    };
    q.uv[1][0] += slope as f64 / 65536.0;
    (q, slot)
}
#[test]
fn overlapping_registered_cuts_integer_goldens_rom_calendar_and_ce() {
    let b = bound::Binding::build().unwrap();
    let c = canonical();
    let dcal = calendar(&b.derivative, &c.derivative.frame, D_OUTPUTS, true);
    let lcal = calendar(&b.lod, &c.lod.frame, L_OUTPUTS, true);
    let df = dcal.fields().to_vec();
    let lf = lcal.fields().to_vec();
    let mut d = DerivativeEmu::new(dcal).unwrap();
    let mut l = LodEmu::new(lcal).unwrap();
    let mut slopes = vec![
        0,
        1,
        2,
        3,
        127,
        255,
        256,
        257,
        65536,
        65536,
        131072,
        524289,
        1 << 17,
    ];
    slopes.extend((0..64).map(|k| 65536 + k * 1024 + 512));
    // Every exact RNE tie, including odd63->64 carry, crossed with all filters,
    // mip availability and boundary biases; negative helper UV is retained.
    let samples: Vec<_> = (0..6)
        .flat_map(|_| slopes.iter().copied())
        .enumerate()
        .map(|(id, s)| stimulus(s, id))
        .collect();
    let inputs: Vec<_> = samples
        .iter()
        .map(|(q, s)| derivative::Input::capture(q, *s).unwrap())
        .collect();
    let drefs: Vec<_> = samples
        .iter()
        .map(|(q, s)| bound::prepare(q, &[*s]).unwrap())
        .collect();
    let mut owners = [None; 4];
    let mut accepted = 0;
    let mut completed = 0;
    let mut enabled = 0;
    let mut per_cut = 0;
    let mut roms = [0; 3];
    let mut writes = 0;
    let mut frozen = 0;
    let mut csv = std::env::var_os("GPU_DERIVATIVE_LOD_OUTPUT").map(|p| {
        std::fs::File::create(std::path::PathBuf::from(p).join("standalone-cuts.csv")).unwrap()
    });
    if let Some(f) = csv.as_mut() {
        writeln!(
            f,
            "wall,enabled,stage,event,age,iteration,value,operands,raw,ROM"
        )
        .unwrap();
    }
    for wall in 0..100_000 {
        let ce = wall % 17 < 11;
        let old_d = (d.bank().to_vec(), d.phase(), d.output().unwrap());
        let old_l = (l.bank().to_vec(), l.phase(), l.output().unwrap());
        if ce {
            if let Some(o) = old_d.2 {
                assert_eq!(
                    o,
                    d_golden(
                        inputs[owners[((enabled + 32 - usize::from(derivative::SPAN)) % 32) / 8]
                            .unwrap()]
                    )
                );
            }
            if let Some(o) = old_l.2 {
                let id = owners[((enabled + 32 - usize::from(lod::SPAN)) % 32) / 8].unwrap();
                assert_eq!(o, l_golden(d_golden(inputs[id]).into()));
                completed += 1;
            }
        }
        let offer = if ce && enabled % 8 == 0 && accepted < inputs.len() {
            owners[(enabled % 32) / 8] = Some(accepted);
            let i = inputs[accepted];
            accepted += 1;
            Some(i)
        } else {
            None
        };
        let de = d.tick(ce, offer).unwrap();
        let le = l
            .tick(ce, offer.map(|i| lod::Input::from(d_golden(i))))
            .unwrap();
        if !ce {
            assert_eq!((d.bank().to_vec(), d.phase(), d.output().unwrap()), old_d);
            assert_eq!((l.bank().to_vec(), l.phase(), l.output().unwrap()), old_l);
            assert!(de.calculations.is_empty() && le.writes.is_empty());
            frozen += 1;
            continue;
        }
        for (name, edge, fields) in [("D", de, &df), ("LOD", le, &lf)] {
            for calc in &edge.calculations {
                let id = owners[calc.iteration].unwrap();
                let frame = if name == "D" {
                    &drefs[id].derivative.frame
                } else {
                    &drefs[id].lod.frame
                };
                assert_eq!(
                    calc.raw, frame.values[calc.value].raw,
                    "{name} sample{id} age{} event{}",
                    calc.age, calc.event
                );
                per_cut += 1;
                if let Some((port, index)) = calc.rom {
                    match port {
                        0 => {
                            assert_eq!(calc.age, 9);
                            assert_eq!(
                                calc.raw,
                                ((1.0 + index as f64 / 64.0).log2() * 256.0).round_ties_even()
                                    as i128
                            );
                            roms[0] += 1;
                        }
                        1 => {
                            assert!([22, 25].contains(&calc.age));
                            let prefix: i128 = (0..index)
                                .map(|n| 1_i128 << (2 * n.saturating_sub(3)))
                                .sum();
                            assert_eq!(calc.raw, prefix);
                            roms[if calc.age == 22 { 1 } else { 2 }] += 1;
                        }
                        _ => panic!("unknown ROM"),
                    }
                }
                if let Some(f) = csv.as_mut() {
                    writeln!(
                        f,
                        "{wall},{enabled},{name},{},{},{},{},{:?},{},{:?}",
                        calc.event,
                        calc.age,
                        calc.iteration,
                        calc.value,
                        calc.operands,
                        calc.raw,
                        calc.rom
                    )
                    .unwrap();
                }
            }
            for w in &edge.writes {
                let field = fields
                    .iter()
                    .find(|f| f.value == w.value && f.source_low == w.source_low)
                    .unwrap();
                // Per-operation values independently checked above. Destination
                // and width must be one of the exact paid physical slices.
                assert!(field.lows.contains(&w.low));
                assert_eq!(field.width, w.width);
                writes += 1;
            }
        }
        enabled += 1;
        if accepted == inputs.len() && d.idle() && l.idle() {
            break;
        }
    }
    assert_eq!(completed, inputs.len());
    assert_eq!(roms, [inputs.len(); 3]);
    assert!(frozen > 100 && per_cut > 10_000 && writes > 10_000);
    println!("DLOD samples={} cuts={per_cut} writes={writes} ROM={roms:?} frozen={frozen} enabled={enabled}",inputs.len());
}
#[test]
fn each_actual_cut_matches_partial_helper_sample() {
    let b = bound::Binding::build().unwrap();
    let c = canonical();
    let slot = Slot {
        base_address: 4096,
        max_size_log2: 5,
        has_full_mip: true,
        valid: true,
    };
    let mut q = QuadInput {
        force_coarsest: false,
        quad_id: 3,
        mask: 1,
        uv: [[0.13, 0.07]; 4],
        slot: 0,
        material_size_log2: 5,
        filter: Filter::Bilinear,
        lod_bias: 0.0,
    };
    q.uv[3][0] += 0.25;
    let p = bound::prepare(&q, &[slot]).unwrap();
    let mut d = DerivativeEmu::new(calendar(
        &b.derivative,
        &c.derivative.frame,
        D_OUTPUTS,
        true,
    ))
    .unwrap();
    let input = derivative::Input::capture(&q, slot).unwrap();
    let mut output = None;
    for age in 0..=derivative::SPAN {
        if let Some(o) = d.output().unwrap() {
            output = Some(o);
        }
        let e = d.tick(true, (age == 0).then_some(input)).unwrap();
        for calc in e.calculations {
            assert_eq!(
                calc.raw, p.derivative.frame.values[calc.value].raw,
                "D age{age} event{} operands{:?}",
                calc.event, calc.operands
            );
        }
    }
    let output = output.unwrap();
    for i in 0..8 {
        assert_eq!(
            i128::from(output.uv[i]),
            p.derivative
                .frame
                .outputs
                .iter()
                .find(|o| o.name == format!("uv{i}"))
                .unwrap()
                .raw
        );
    }
    assert_eq!(output.header.quad, 3);
    let mut l = LodEmu::new(calendar(&b.lod, &c.lod.frame, L_OUTPUTS, true)).unwrap();
    for age in 0..=lod::SPAN {
        if let Some(o) = l.output().unwrap() {
            for (name, raw) in [
                ("shift0", i128::from(o.context.shift)),
                ("nearest", i128::from(o.context.nearest)),
                ("halve", i128::from(o.context.halve)),
                ("last_fine", i128::from(o.context.last_fine)),
                ("mask", i128::from(o.mask)),
                ("slot", i128::from(o.slot)),
            ] {
                assert_eq!(
                    raw,
                    p.lod
                        .frame
                        .outputs
                        .iter()
                        .find(|o| o.name == name)
                        .unwrap()
                        .raw,
                    "returned LOD {name}"
                );
            }
            for i in 0..2 {
                assert_eq!(
                    i128::from(o.context.side[i]),
                    p.lod
                        .frame
                        .outputs
                        .iter()
                        .find(|o| o.name == format!("side{i}"))
                        .unwrap()
                        .raw
                );
                assert_eq!(
                    i128::from(o.context.parents[i]),
                    p.lod
                        .frame
                        .outputs
                        .iter()
                        .find(|o| o.name == format!("parent{i}"))
                        .unwrap()
                        .raw
                );
                assert_eq!(
                    i128::from(o.context.levels[i]),
                    p.lod
                        .frame
                        .outputs
                        .iter()
                        .find(|o| o.name == format!("n{i}"))
                        .unwrap()
                        .raw
                );
            }
            assert_eq!(o.quad, 3);
        }
        let e = l.tick(true, (age == 0).then_some(output.into())).unwrap();
        for calc in e.calculations {
            assert_eq!(
                calc.raw, p.lod.frame.values[calc.value].raw,
                "LOD age{age} event{} operands{:?}",
                calc.event, calc.operands
            );
        }
    }
}

#[test]
fn full_physical_width_and_rejected_inputs_never_add_or_mutate_storage() {
    let b = bound::Binding::build().unwrap();
    let c = canonical();
    let mut d = DerivativeEmu::new(calendar(
        &b.derivative,
        &c.derivative.frame,
        D_OUTPUTS,
        false,
    ))
    .unwrap();
    let mut l = LodEmu::new(calendar(&b.lod, &c.lod.frame, L_OUTPUTS, false)).unwrap();
    let good = derivative::Input {
        force_coarsest: false,
        uv: [-(1 << 17), 0, (1 << 17) - 1, 0, 0, 0, 0, 0],
        bias: 8192,
        header: derivative::Header {
            quad: 15,
            mask: 9,
            slot: 15,
            max_n: 10,
            has_mip: true,
            filter: 2,
        },
    };
    let mut bad = good;
    bad.uv[0] = 1 << 17;
    let old = (d.bank().to_vec(), d.phase());
    assert!(d.tick(true, Some(bad)).is_err());
    assert_eq!((d.bank().to_vec(), d.phase()), old);
    assert!(!d.tick(false, Some(bad)).unwrap().accepted);
    assert_eq!((d.bank().to_vec(), d.phase()), old);
    for age in 0..=derivative::SPAN {
        if let Some(o) = d.output().unwrap() {
            assert_eq!(o, d_golden(good));
            assert_eq!(o.slope, (1 << 18) - 1);
        }
        d.tick(true, (age == 0).then_some(good)).unwrap();
    }
    assert!(d.idle());
    assert_eq!(d.bank().len(), derivative::NUMERIC_BITS.div_ceil(64));
    let i = lod::Input::from(d_golden(good));
    let mut bad = i;
    bad.slope = 1 << 18;
    let old = (l.bank().to_vec(), l.phase());
    assert!(l.tick(true, Some(bad)).is_err());
    assert_eq!((l.bank().to_vec(), l.phase()), old);
    for age in 0..=lod::SPAN {
        if let Some(o) = l.output().unwrap() {
            assert_eq!(o, l_golden(i));
            assert_eq!(o.context.levels, [0, 0]);
        }
        l.tick(true, (age == 0).then_some(i)).unwrap();
    }
    assert!(l.idle());
    assert_eq!(l.bank().len(), lod::NUMERIC_BITS.div_ceil(64));
    let (mut q, slot) = stimulus(1, 0);
    q.uv[0][0] = f64::NAN;
    assert!(derivative::Input::capture(&q, slot).is_err());
}

#[test]
fn derivative_lod_binding_intake() {
    let binding = bound::Binding::build().unwrap();
    let mut q = QuadInput {
        force_coarsest: false,
        quad_id: 0,
        mask: 1,
        uv: [[0.003, 0.003]; 4],
        slot: 0,
        material_size_log2: 9,
        filter: Filter::Trilinear,
        lod_bias: 0.5,
    };
    q.uv[1][0] += 1.0 / 512.0;
    let p = bound::prepare(
        &q,
        &[Slot {
            base_address: 4096,
            max_size_log2: 9,
            has_full_mip: true,
            valid: true,
        }],
    )
    .unwrap();
    if let Some(path) = std::env::var_os("GPU_DERIVATIVE_LOD_OUTPUT") {
        std::fs::create_dir_all(&path).unwrap();
        let path = std::path::PathBuf::from(path);
        for (name, plan, frame) in [
            ("D", &binding.derivative, &p.derivative.frame),
            ("LOD", &binding.lod, &p.lod.frame),
        ] {
            let mut f = std::fs::File::create(path.join(format!("intake-{name}.txt"))).unwrap();
            writeln!(
                f,
                "II={} span={} Dedicated={} Packed={} control={} ROM={}",
                plan.ii(),
                plan.span(),
                plan.fixed_ff_bits,
                plan.packed.ff_bits,
                plan.packed.control_ff_bits,
                plan.rom_ram16_cells
            )
            .unwrap();
            for (i, event) in frame.events.iter().enumerate() {
                let format = event.output.map(|v| frame.values[v].format);
                writeln!(
                    f,
                    "event{i} {:?} inputs{:?} output{:?} {:?} issue{} ready{} resource{:?}",
                    event.operation,
                    event.inputs,
                    event.output,
                    format,
                    plan.times[i].issue,
                    plan.times[i].ready,
                    plan.graph.nodes[i]
                        .resource
                        .map(|r| &plan.graph.resources[r])
                )
                .unwrap();
            }
            writeln!(f, "outputs={:?}", frame.outputs).unwrap();
            writeln!(f,"selector_metrics dedicated_read={} packed_read={} packed_write={} packed_boolean={}",plan.ff_banks.iter().map(|f|u64::from(f.width)*f.slots.saturating_sub(1)).sum::<u64>(),plan.packed.read_selector_tree_bits,plan.packed.write_selector_tree_bits,plan.packed.control_boolean_gates).unwrap();
            writeln!(f, "packed fields={:?}", plan.packed_fields).unwrap();
            writeln!(f, "placements={:?}", plan.packed.placements).unwrap();
        }
    }
    assert_eq!(
        (
            binding.derivative.ii(),
            binding.derivative.span(),
            binding.derivative.packed.ff_bits
        ),
        (8, 17, 858)
    );
    assert_eq!(
        (
            binding.lod.ii(),
            binding.lod.span(),
            binding.lod.packed.ff_bits,
            binding.lod.rom_ram16_cells
        ),
        (8, 28, 264, 45)
    );
}
