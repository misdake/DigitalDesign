//! Command source-Q POT/RTT coordinates, exact six-row lifetime and old credits.
#[allow(dead_code)]
#[path = "../support/vliw_pixel_source.rs"]
mod actual;
#[path = "../support/vliw_sampling_direct.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "../support/vliw_cycle/mod.rs"]
mod transport_support;
#[allow(dead_code)]
#[path = "../support/vliw_vertex/mod.rs"]
mod vertex_support;
use gpu_v2::system::vliw_cycle::{sampling::direct::*, vertex::Bits};
use std::collections::VecDeque;
#[derive(Clone, Copy)]
struct Edge {
    i: Input,
    o: Output,
}
fn request(n: usize) -> Input {
    let view = n % 7 == 6;
    Input {
        uv: [
            0,
            1,
            0x7fff,
            0x8000,
            0xffff,
            0xfeca_0171,
            0xffff_ffff,
            0x8000_8000,
            0x1234_abcd,
        ][n % 9],
        quad: (n % 16) as u8,
        lane: (n % 4) as u8,
        mask: 1 << (n % 4),
        lod: [0, 1, 4, 5, 7, 8, 15, 48, 79, 80, 255][n % 11],
        max_n: if view { 9 } else { [0, 1, 2, 6, 10][n / 3 % 5] },
        mip: !view && (n / 5).is_multiple_of(2),
        filter: (n / 2 % 3) as u8,
        framebuffer: view,
        ..Default::default()
    }
}
fn golden(i: Input) -> (u64, u64) {
    let n = if i.mip {
        i.max_n.saturating_sub(i.lod / 8)
    } else {
        i.max_n
    };
    let frac = if i.mip && i.lod / 8 < i.max_n && i.filter == 2 {
        i.lod % 8
    } else {
        0
    };
    let lambda = (u64::from(frac) * 511 + 4) / 8;
    let owner = (511 - lambda)
        | (lambda << 9)
        | (u64::from(i.filter == 0) << 18)
        | (u64::from(n) << 19)
        | (u64::from(n.saturating_sub(1)) << 23)
        | (u64::from(i.quad * 4 + i.lane) << 31)
        | (u64::from(lambda == 0) << 37);
    let mut result = 0;
    for a in 0..2 {
        let uv = u64::from((i.uv >> (a * 16)) as u16);
        let side = if i.framebuffer {
            [400, 240][a]
        } else {
            1u64 << n.max(1)
        };
        let q = (uv * side / 256) as i64 - if i.filter == 0 { 0 } else { 128 };
        let (coordinate, fraction) = if i.framebuffer {
            let c = q.clamp(0, ((side - 1) * 256) as i64) as u64;
            (c / 256, c % 256)
        } else {
            (
                (q.div_euclid(256)).rem_euclid(side as i64) as u64,
                q.rem_euclid(256) as u64,
            )
        };
        result |= fraction << (a * 8);
        result |= coordinate << (16 + a * 10);
    }
    (owner, result)
}
fn run(native: bool) -> Vec<Edge> {
    let mut d = if native {
        Direct::native()
    } else {
        Direct::default()
    };
    let mut expected = VecDeque::new();
    let mut rows = Vec::new();
    let mut n = 0;
    let mut v_loan = false;
    let mut full = 0;
    let mut received = 0;
    let mut enabled = 0usize;
    for wall in 0..3000 {
        let i = Input {
            reset: wall == 0 || wall == 275,
            ce: wall % 5 != 2,
            commit_allow: wall % 7 != 4,
            valid: n < 160 && !v_loan,
            out_ready: wall > 60 && wall % 11 > 2,
            ..request(n)
        };
        let o = d.outputs(i);
        if o.valid {
            let (owner, result, at): (u64, u64, usize) = *expected.front().unwrap();
            assert_eq!(o.owner, Bits::new(owner, 38), "owner wall{wall}");
            assert_eq!(o.result, Bits::new(result, 36), "result wall{wall}");
            assert!(enabled - at >= 3);
            if o.advance && i.out_ready {
                expected.pop_front();
                received += 1;
            }
        }
        if o.issue {
            let (owner, result) = golden(i);
            expected.push_back((owner, result, enabled));
        }
        rows.push(Edge { i, o });
        d.clock(i)
            .unwrap_or_else(|e| panic!("wall{wall} case{n}: {e}"));
        full += usize::from(o.occupied == 6);
        if i.reset {
            expected.clear();
            v_loan = false;
            enabled = 0;
        } else if o.advance {
            enabled += 1;
            if v_loan {
                v_loan = false;
                n += 1;
            } else if o.issue {
                v_loan = true;
            }
        }
        if n == 160 && d.occupied == 0 {
            break;
        }
    }
    assert!(n == 160 && received > 130 && full > 2 && expected.is_empty());
    println!(
        "DIRECT_C6 walls={} received={received} full={full}",
        rows.len()
    );
    rows
}
#[test]
fn source_q_coordinates_match_independent_integer_layout() {
    run(false);
    run(true);
}
fn root(label: &str) -> std::path::PathBuf {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-draw")
        .join(format!(
            "{label}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[test]
#[ignore = "two finite actual command coordinate/C6 implementations"]
fn direct_lod_matches_actual_source_q_and_six_rows() {
    let root = root("direct");
    for native in [false, true] {
        let rows = run(native);
        let pins = fixture::fields(&rows[0]);
        vertex_support::rtl::replay(
            &root,
            if native { "native" } else { "portable" },
            rows.iter().map(fixture::fields).collect(),
            fixture::production(&pins, native),
            false,
            None,
        );
    }
    println!("DIRECT_ROOT {}", root.display());
}
#[test]
#[ignore = "two compiled DirectLOD mutations and original mask/source-Q monitors"]
fn coordinate_credit_and_source_q_boundaries_reject_counterexamples() {
    let rows = run(false);
    let root = root("direct-negative");
    let pins = fixture::fields(&rows[0]);
    let source = fixture::production(&pins, false);
    for (name, old, new) in [
        (
            "old_six_row_credit",
            "assign in_ready=!reset && count<CAP;",
            "assign in_ready=!reset && (count<CAP || take);",
        ),
        (
            "v_is_a_separate_axis",
            "wire[15:0]axis_uv=phase[0]?in_wrapped_uv_0:in_wrapped_uv_1;",
            "wire[15:0]axis_uv=in_wrapped_uv_0;",
        ),
    ] {
        assert_eq!(source.matches(old).count(), 1, "{name}");
        vertex_support::rtl::replay(
            &root,
            name,
            rows.iter().map(fixture::fields).collect(),
            source.replacen(old, new, 1),
            true,
            None,
        );
    }
    for change_v in [false, true] {
        let mut d = Direct::default();
        let mut bad_rows = Vec::new();
        let mut rejected = false;
        for wall in 0..5 {
            let i = Input {
                reset: wall == 0,
                ce: true,
                commit_allow: true,
                valid: wall == 1,
                mask: if change_v { 1 } else { 0 },
                uv: 0x1234_abcd ^ if change_v && wall == 2 { 0x10000 } else { 0 },
                max_n: 6,
                filter: 1,
                ..Default::default()
            };
            bad_rows.push(Edge { i, o: d.outputs(i) });
            if let Err(e) = d.clock(i) {
                let expected = if change_v {
                    "coordinate V input changed before its source-Q consumption"
                } else {
                    "GPU contract: preflight_bad"
                };
                assert_eq!(e.rule, expected);
                vertex_support::rtl::replay(
                    &root,
                    if change_v {
                        "changed_V"
                    } else {
                        "uncovered_lane"
                    },
                    bad_rows.iter().map(fixture::fields).collect(),
                    source.clone(),
                    false,
                    Some(expected),
                );
                rejected = true;
                break;
            }
        }
        assert!(rejected);
    }
    println!("DIRECT_NEGATIVE_ROOT {}", root.display());
}
