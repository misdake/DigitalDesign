//! Frozen pre-row Runtime independently computes complete color sequences.
//! Changed row timing is intentional; numerical/public ownership is invariant.
use super::{qualification as fixture, row_reference, *};
use crate::texture::{emu::context_store::Raw, ports::Filter};

const BOUND: u64 = 40000;
fn hardware() -> timed::Hardware {
    timed::Hardware {
        prefetch: false,
        group_capacity: 32,
        read_latency: 1,
        result_capacity: crate::texture::emu::color::RESULT_CAPACITY,
        ..Default::default()
    }
}
fn inputs() -> Vec<RawQuadInput> {
    (0..48)
        .map(|i| {
            let filter = [Filter::Nearest, Filter::Bilinear, Filter::Trilinear][i % 3];
            let mut q = fixture::input(
                (i % 16) as u8,
                [15, 1, 6, 8, 0][i % 5],
                [
                    [0.13, 0.07],
                    [7.0 / 32.0, 7.0 / 32.0],
                    [-0.03125, 0.46875],
                    [0.96875, 0.96875],
                ][i % 4],
                filter,
            );
            q.lod_bias = [-0.5, 0.0, 0.5][i % 3];
            q.force_coarsest = i % 11 == 0;
            // Uncovered helpers still influence the actual D calculation.
            q.uv[3][0] += if i % 7 == 0 { 0.25 } else { 0.0 };
            RawQuadInput::capture(&q).unwrap()
        })
        .collect()
}
fn raw(q: &RawQuadInput, slot: Slot) -> Raw {
    let input = derivative::Input::from_raw(q, slot).unwrap();
    Raw {
        bias: input.bias,
        force_coarsest: input.force_coarsest,
        header: input.header,
    }
}
fn canonical_trace(trace: &super::super::runtime_preparation::Trace) {
    use crate::texture::emu::{compact, coordinate_store::canonical};
    let edge = trace.coordinate_store_edge;
    if let Some((_, word)) = edge.owner_write {
        assert!(word < 1 << 24, "actual canonical storage width");
        let offered = trace.coordinate_store_tick.issue.unwrap();
        assert_eq!(canonical::unpack(word as u32), offered);
        // Separate concatenation for the physical word; the frozen wide
        // reference is unchanged and still stores owner38.
        let expected = u64::from(offered.parents[1])
            | u64::from(offered.nearest) << 9
            | u64::from(offered.levels[0]) << 10
            | u64::from(offered.slot) << 14
            | u64::from(offered.key) << 18;
        assert_eq!(word, expected);
    }
    if let Some((_, owner, result)) = edge.consume {
        assert!(owner < 1 << 24 && result < 1 << 36);
        let decoded = canonical::unpack(owner as u32);
        let expanded = decoded.finish(compact::expand_result(result as u64, decoded.levels[0]));
        assert_eq!(expanded, trace.coefficient_tick.unwrap().input.unwrap());
    }
}
fn run(paused: bool, continuous_prepare: bool) {
    let (slots, bytes) = fixture::fixture();
    let inputs = inputs();
    let prep = PreparationHardware {
        max_cycles: BOUND,
        ..Default::default()
    };
    let mut reference = row_reference::Runtime::new(&slots, prep, hardware(), BOUND).unwrap();
    let mut compatible = Runtime::new(&slots, prep, hardware(), BOUND).unwrap();
    let mut compatible_mc = fixture::physical::Physical::new(4096, bytes.clone(), true, paused);
    let mut old_mc = fixture::physical::Physical::new(4096, bytes.clone(), true, paused);
    let mut expected = vec![];
    let mut admitted = 0;
    let mut rng = 0x813579adu32;
    for wall in 0..BOUND {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        let step = reference
            .step_raw(
                &mut old_mc,
                inputs.get(admitted),
                timed::Control {
                    ce: !paused || rng & 7 > 1,
                    result_ready: !paused || wall > 500 && rng % 13 > 2,
                },
            )
            .unwrap();
        let legacy = compatible
            .step_raw(&mut compatible_mc, inputs.get(admitted), step.control)
            .unwrap();
        canonical_trace(&compatible.preparation.trace);
        assert_eq!(
            (legacy.input_ready, legacy.accepted, legacy.effective_ce),
            (step.input_ready, step.accepted, step.effective_ce),
            "atomic contract wall {wall}"
        );
        assert_eq!(
            legacy.results, step.results,
            "atomic exact result edges {wall}"
        );
        assert_eq!(
            legacy.snapshot.preparation_live,
            step.snapshot.preparation_live
        );
        assert_eq!(legacy.snapshot.result_lanes, step.snapshot.result_lanes);
        assert_eq!(legacy.snapshot.cache, step.snapshot.cache);
        assert_eq!(legacy.snapshot.color, step.snapshot.color);
        admitted += usize::from(step.accepted);
        expected.extend(step.results);
        if admitted == inputs.len() && reference.idle() {
            break;
        }
    }
    assert_eq!(admitted, inputs.len());
    assert!(reference.idle());
    assert!(compatible.idle());
    let mut rows =
        Runtime::new_rows_profile(&slots, prep, hardware(), BOUND, continuous_prepare).unwrap();
    let mut row_mc = fixture::physical::Physical::new(4096, bytes, true, paused);
    let mut actual = vec![];
    let mut quad = 0;
    let mut lane = 0;
    let mut row_takes = 0;
    let mut commits = 0;
    let mut paused_partial = 0;
    let mut partial_edges = 0;
    let mut raw_writes = 0;
    let mut c_returns = 0;
    let mut ce0_returns = 0;
    let mut blocked_results = 0;
    let mut rng = 0x813579adu32;
    for wall in 0..BOUND {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        let offered = inputs.get(quad).map(|q| RowInput {
            raw: raw(q, slots[usize::from(q.slot)]),
            uv_q16: q.uv_q16[lane],
        });
        let before = rows.preparation.frontend_state();
        let step = rows
            .step_row(
                &mut row_mc,
                offered.as_ref(),
                timed::Control {
                    ce: !paused || rng & 7 > 1,
                    result_ready: !paused || wall > 500 && rng % 13 > 2,
                },
            )
            .unwrap();
        let trace = &rows.preparation.trace;
        canonical_trace(trace);
        assert_eq!(trace.uv_row_tick.row.is_some(), step.row_accepted);
        blocked_results +=
            usize::from(!step.control.result_ready && step.snapshot.color.queued != 0);
        raw_writes += usize::from(trace.uv_row.raw_write.is_some());
        c_returns += usize::from(trace.uv_row.returned.is_some());
        ce0_returns += usize::from(trace.uv_row.returned.is_some() && !step.effective_ce);
        if lane != 0 {
            partial_edges += 1;
            paused_partial += usize::from(!step.effective_ce);
            if !step.accepted {
                assert_eq!(
                    before.allocate,
                    rows.preparation.frontend_state().allocate,
                    "partial allocation changes only on publication"
                );
            }
        }
        if step.row_accepted {
            row_takes += 1;
            assert_eq!(step.accepted, lane == 3);
            if lane < 3 {
                assert_eq!(
                    rows.preparation.live_mask() >> (inputs[quad].quad_id) & 1,
                    0
                );
                assert_eq!(trace.uv_row.committed, None);
                assert_eq!(rows.preparation.frontend_state().allocate, before.allocate);
            }
            lane += 1;
            if lane == 4 {
                lane = 0;
                quad += 1;
                commits += 1;
            }
        } else {
            assert!(!step.accepted);
        }
        actual.extend(step.results);
        if quad == inputs.len() && rows.idle() {
            break;
        }
    }
    assert_eq!(quad, inputs.len());
    assert!(rows.idle());
    assert_eq!(row_takes, inputs.len() * 4);
    assert_eq!(commits, inputs.len());
    assert_eq!(raw_writes, inputs.len());
    assert!(partial_edges > inputs.len() * 2);
    if paused {
        assert!(paused_partial > 0);
        assert!(ce0_returns > 0);
        assert!(blocked_results > 0);
    }
    assert_eq!(
        actual, expected,
        "complete nearest/bilinear/trilinear/seam/miss results"
    );
    assert!(row_mc.cycles > 0 && old_mc.cycles > 0);
    assert!(rows.cache_stats().refills > 0 && reference.cache_stats().refills > 0);
    assert!(c_returns > 0);
    assert_eq!(rows.stats.admissions, inputs.len() as u64);
    assert_eq!(rows.preparation.stats.accepted, inputs.len() as u64);
    println!("PASS rows continuous={continuous_prepare} paused={paused}: {} quads,{} rows,{} results,{} C returns ({} CE0),{} partial edges,{} result-blocked edges,{} refills",inputs.len(),row_takes,actual.len(),c_returns,ce0_returns,partial_edges,blocked_results,rows.cache_stats().refills);
}
#[test]
fn rows_match_frozen_runtime_for_all_filters_misses_and_stalls() {
    for continuous_prepare in [false, true] {
        run(false, continuous_prepare);
        run(true, continuous_prepare);
    }
}
