#[path = "support/texture.rs"]
mod support;
use audited::{Fixed, Model, Operation, Resource};
use gpu_v2::texture::{
    ports::*,
    sim::{counted, oracle},
};
use support::*;

fn raw(report: &audited::FrameReport, name: &str) -> i128 {
    report
        .outputs
        .iter()
        .find(|o| o.name == name)
        .unwrap_or_else(|| panic!("missing {name}"))
        .raw
}
fn compare(q: &QuadInput, s: Slot) -> counted::Preparation {
    let expected = oracle::prepare(q, &[s], Config::counted()).unwrap();
    let got = counted::prepare(q, &[s]).unwrap();
    got.frame.audit().unwrap();
    assert_eq!(got.frame.scheduled_cycles(), None);
    assert_eq!(got.lod, expected.lod.raw, "LOD: {q:?}");
    assert_eq!(
        raw(&got.frame, "overflow"),
        i128::from(expected.lod.overflow)
    );
    for edge in 0..4 {
        for axis in 0..2 {
            assert_eq!(
                raw(&got.frame, &format!("d{edge}.{axis}")),
                (expected.lod.derivatives[edge][axis] * 65536.0) as i128
            );
        }
    }
    if let Some(index) = expected.lod.table_index {
        assert_eq!(raw(&got.frame, "log.index"), i128::from(index));
        assert_eq!(
            raw(&got.frame, "log.exponent"),
            i128::from(expected.lod.exponent)
        );
    }
    let groups: Vec<_> = expected
        .pixels
        .iter()
        .flat_map(|p| p.groups.iter().cloned())
        .collect();
    assert_eq!(got.groups, groups, "groups: {q:?}");
    for p in &expected.pixels {
        assert_eq!(got.lambda, p.lambda);
        for (i, l) in p.layers.iter().enumerate() {
            let label = format!("p{}.l{i}", p.lane);
            assert_eq!(raw(&got.frame, &format!("{label}.n")), i128::from(l.n));
            assert_eq!(
                raw(&got.frame, &format!("{label}.parent")),
                i128::from(l.parent)
            );
            for axis in 0..2 {
                assert_eq!(
                    raw(&got.frame, &format!("{label}.q{axis}")),
                    (l.p[axis] * 256.0).floor() as i128
                );
                assert_eq!(
                    raw(&got.frame, &format!("{label}.i{axis}")),
                    i128::from(l.integer[axis])
                );
                assert_eq!(
                    raw(&got.frame, &format!("{label}.f{axis}")),
                    i128::from(l.fraction[axis])
                );
            }
            for t in 0..4 {
                assert_eq!(
                    raw(&got.frame, &format!("{label}.w{t}")),
                    i128::from(l.coefficients[t])
                );
                for axis in 0..2 {
                    assert_eq!(
                        raw(&got.frame, &format!("{label}.t{t}.{axis}")),
                        i128::from(l.taps[t][axis])
                    );
                }
            }
        }
    }
    for (group, address) in got.groups.iter().zip(&got.addresses) {
        assert_eq!(u64::from(*address), group.key.address(&[s]).unwrap());
    }
    let fifo = got
        .frame
        .memories
        .iter()
        .position(|m| m.name == "Group4_fifo")
        .unwrap();
    assert_eq!(got.frame.memories[fifo].rows, 32);
    assert_eq!(
        got.frame.counts.write_bits.get(&fifo).copied().unwrap_or(0),
        got.groups.len() as u64 * 72
    );
    assert_eq!(raw(&got.frame, "group_count"), got.groups.len() as i128);
    got
}

#[test]
fn preparation_matches_every_stage_across_contract_boundaries() {
    let mut random = 0x6f13_97a2_u32;
    for n in 0..=10 {
        for mip in [false, true] {
            for filter in [Filter::Nearest, Filter::Bilinear, Filter::Trilinear] {
                for case in 0..32 {
                    let mut q = input(n, filter, [0.0; 2]);
                    q.mask = (case % 16) as u8;
                    for uv in &mut q.uv {
                        for value in uv {
                            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                            *value = f64::from(random as i32) / 2147483648.0;
                        }
                    }
                    q.lod_bias = match case % 8 {
                        0 => -1000.0,
                        1 => 1000.0,
                        2 => -0.5,
                        3 => 0.5,
                        _ => 0.0,
                    };
                    if case % 3 != 0 {
                        let start = q.uv[0];
                        let step = 2.0_f64.powi(-i32::from(n) - 2);
                        q.uv = [
                            start,
                            [start[0] + step, start[1]],
                            [start[0], start[1] + step],
                            [start[0] + step * 1.7, start[1] + step * 1.3],
                        ];
                    }
                    if case == 31 {
                        q.mask = 12;
                        q.uv = [
                            [1048576.0, -1048576.0],
                            [-1048576.0, 1048576.0],
                            [0.0, 0.0],
                            [1.0, -1.0],
                        ];
                    }
                    compare(&q, slot(n, mip));
                }
            }
        }
    }
}

#[test]
fn lod_grid_ties_carry_and_helper_edges_are_audited() {
    for exponent in -6..=2 {
        for k in 0..64 {
            for offset in [-1, 0, 1] {
                let slope = (1.0 + (f64::from(k) + 0.5) / 64.0) * 2.0_f64.powi(exponent - 9)
                    + f64::from(offset) / 65536.0;
                let mut q = input(9, Filter::Trilinear, [-0.25, 0.0]);
                q.mask = 1;
                // Only the uncovered bottom helper edge has the maximal difference.
                q.uv = [
                    [-0.25, 0.0],
                    [-0.25, 0.0],
                    [-0.25, 0.0],
                    [-0.25 + slope, 0.0],
                ];
                compare(&q, slot(9, true));
            }
        }
    }
    for bias in [-33.0, -0.5 / 256.0, 0.5 / 256.0, 1.5 / 256.0, 33.0] {
        let mut q = input(10, Filter::Trilinear, [0.0; 2]);
        q.uv[1][0] = 1.0 / 1024.0;
        q.lod_bias = bias;
        compare(&q, slot(10, true));
    }
    let p = compare(&input(9, Filter::Trilinear, [0.0; 2]), slot(9, true));
    assert!(!p.frame.outputs.iter().any(|o| o.name == "log.index"));
}

#[test]
fn full_parent_uses_two_products_and_fractional_mips_use_six() {
    let mut q = input(9, Filter::Trilinear, [0.137, 0.281]);
    let single = compare(&q, slot(9, true));
    assert_eq!(single.frame.counts.logical_products.get(&(9, 8)), Some(&8));
    q.uv[1][0] += 1.0 / 512.0;
    q.lod_bias = 0.5;
    let dual = compare(&q, slot(9, true));
    assert_eq!(dual.frame.counts.logical_products.get(&(9, 8)), Some(&24));
    assert_eq!(dual.frame.counts.resources.get(&Resource::Dsp18), Some(&24));
    let mut forged = dual.frame.clone();
    *forged.counts.resources.get_mut(&Resource::Dsp18).unwrap() -= 1;
    assert!(forged.audit().is_err());
    let mut forged = dual.frame;
    forged
        .outputs
        .iter_mut()
        .find(|o| o.name == "lod")
        .unwrap()
        .raw += 1;
    assert!(forged.audit().is_err());
}

#[test]
fn seam_case_fills_all_32_group_rows_and_wrap_guard_covers_nearest_last_texel() {
    let mut q = input(10, Filter::Trilinear, [0.0; 2]);
    q.uv[3][0] = 1.0 / 65536.0;
    q.lod_bias = 6.5;
    let p = compare(&q, slot(10, true));
    assert_eq!(p.groups.len(), 32);
    assert_eq!(p.groups.iter().filter(|g| g.first).count(), 4);
    assert_eq!(p.groups.iter().filter(|g| g.last).count(), 4);
    for coordinate in [0.0, 1.0 - 1.0 / 65536.0, -1.0 / 65536.0] {
        compare(
            &input(10, Filter::Nearest, [coordinate; 2]),
            slot(10, false),
        );
    }
    // Slot and quad identifiers occupy the top representable field values.
    q.slot = 15;
    q.quad_id = 15;
    let mut slots = vec![slot(10, true); 16];
    slots[15].base_address = 0x21000;
    let actual = counted::prepare(&q, &slots).unwrap();
    let expected = oracle::prepare(&q, &slots, Config::counted()).unwrap();
    assert_eq!(
        actual.groups,
        expected
            .pixels
            .into_iter()
            .flat_map(|p| p.groups)
            .collect::<Vec<_>>()
    );
    for (g, address) in actual.groups.iter().zip(&actual.addresses) {
        assert_eq!(g.key.address(&slots).unwrap(), u64::from(*address));
    }
    let mut last_slot = slot(0, true);
    last_slot.base_address = 0xffff_ff80;
    let last = counted::prepare(&input(0, Filter::Bilinear, [0.0; 2]), &[last_slot]).unwrap();
    assert!(last.addresses.iter().all(|&a| a == 0xffff_ff80));
}

#[test]
fn normalization_exhausts_the_entire_17_bit_color_contract() {
    for start in (0..=130305).step_by(512) {
        let values: Vec<i128> = (start..=(start + 511).min(130305))
            .map(i128::from)
            .collect();
        let mut m = Model::numerical();
        let input = m.input::<17, 0, false>("accumulator", &values).unwrap();
        let f = m.compute("normalize_exhaustive", 20000).unwrap();
        let mut address = Fixed::<10, 0, false>::constant::<0>();
        for i in 0..values.len() {
            let result =
                counted::normalize_color(&f, f.read(input.indexed(address)).unwrap()).unwrap();
            f.publish(&format!("c{i}"), result).unwrap();
            address = f
                .add_same(address, Fixed::<10, 0, false>::constant::<1>())
                .unwrap();
        }
        let report = f.finish();
        report.audit().unwrap();
        for (output, value) in report.outputs.iter().zip(values) {
            assert_eq!(output.raw, i128::from(oracle::rne_div(value as u64, 511)));
        }
        assert!(!report
            .events
            .iter()
            .any(|e| matches!(e.operation, Operation::Multiply)));
    }
}

#[test]
fn colors_partial_accumulators_and_memory_requests_match_oracle() {
    for n in [0, 1, 2, 3, 5, 9, 10] {
        for mip in [false, true] {
            let s = slot(n, mip);
            let bytes = asset(s, pattern);
            let mut image1 = Image {
                bytes: bytes.clone(),
                requests: vec![],
            };
            let mut image2 = Image {
                bytes,
                requests: vec![],
            };
            let mut cache1 = oracle::Cache::new(vec![s]).unwrap();
            let mut cache2 = oracle::Cache::new(vec![s]).unwrap();
            for case in 0..24 {
                let mut q = input(
                    n,
                    if case % 3 == 0 {
                        Filter::Nearest
                    } else if case % 3 == 1 {
                        Filter::Bilinear
                    } else {
                        Filter::Trilinear
                    },
                    [-0.6 + f64::from(case) * 0.02, 0.125],
                );
                q.mask = (case % 16) as u8;
                q.lod_bias = f64::from(case % 7) / 4.0;
                q.uv[1][0] += 2.0_f64.powi(-i32::from(n));
                q.uv[3][1] += 2.0_f64.powi(-i32::from(n) - 1);
                let expected =
                    oracle::sample(&q, &mut cache1, &mut image1, Config::counted()).unwrap();
                let got = counted::sample(&q, &mut cache2, &mut image2)
                    .unwrap_or_else(|e| panic!("n={n},mip={mip},case={case},q={q:?}: {e:?}"));
                assert_eq!(got.pixels.len(), expected.pixels.len());
                for (actual, golden) in got.pixels.iter().zip(&expected.pixels) {
                    assert_eq!((actual.lane, actual.rgb), (golden.lane, golden.rgb));
                    actual.frame.audit().unwrap();
                    assert_eq!(
                        actual.frame.counts.logical_products.get(&(9, 8)),
                        Some(&(golden.groups.len() as u64 * 12))
                    );
                    for (i, g) in golden.groups.iter().enumerate() {
                        let mut banks = Vec::new();
                        for tap in 0..4 {
                            let (bank, local) = oracle::bank_local(
                                g.group.top_left_local[0] + (tap & 1) as u8,
                                g.group.top_left_local[1] + (tap >> 1) as u8,
                            );
                            assert_eq!(
                                raw(&actual.frame, &format!("g{i}.t{tap}.bank")),
                                bank as i128
                            );
                            assert_eq!(
                                raw(&actual.frame, &format!("g{i}.t{tap}.address")) % 16,
                                local as i128
                            );
                            banks.push(bank);
                        }
                        banks.sort();
                        assert_eq!(banks, vec![0, 1, 2, 3]);
                        for c in 0..3 {
                            assert_eq!(
                                raw(&actual.frame, &format!("g{i}.partial{c}")),
                                i128::from(g.partial[c])
                            );
                            assert_eq!(
                                raw(&actual.frame, &format!("g{i}.acc{c}")),
                                i128::from(g.accumulator[c])
                            );
                            for tap in 0..4 {
                                assert_eq!(
                                    raw(&actual.frame, &format!("g{i}.t{tap}.c{c}")),
                                    i128::from(g.expanded[tap][c])
                                );
                            }
                        }
                    }
                }
                assert_eq!(image1.requests, image2.requests);
                assert_eq!(cache1.stats, cache2.stats);
            }
        }
    }
}

#[test]
fn invalid_inputs_are_rejected_before_external_memory() {
    let mut q = input(10, Filter::Trilinear, [0.0; 2]);
    q.uv[0][0] = f64::NAN;
    assert!(counted::prepare(&q, &[slot(10, true)]).is_err());
    q.uv[0][0] = 1048576.1;
    assert!(counted::prepare(&q, &[slot(10, true)]).is_err());
    q.uv[0][0] = 0.0;
    q.material_size_log2 = 9;
    assert!(counted::prepare(&q, &[slot(10, true)]).is_err());
}
