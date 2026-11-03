//! Physical scheduled products, sole weight writes and mask/metadata publication.
#[allow(dead_code)]
#[path = "support/vliw_pixel_source.rs"]
mod actual;
#[path = "support/vliw_sampling_coefficient.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "support/vliw_cycle/mod.rs"]
mod transport_support;
#[allow(dead_code)]
#[path = "support/vliw_vertex/mod.rs"]
mod vertex_support;
use gpu_v2::system::vliw_cycle::{sampling::coefficient::*, vertex::Bits};
use std::collections::{BTreeMap, VecDeque};
#[derive(Clone)]
struct Edge {
    i: Input,
    o: Output,
    s: Coefficient,
}
fn request(n: usize) -> Input {
    let level = [0, 1, 2, 6, 10][n % 5];
    let nearest = n.is_multiple_of(7);
    let coarse = if nearest || level == 0 {
        0
    } else {
        [0, 64, 256, 319, 511][n % 5]
    };
    let side = 1u64 << level.max(1);
    let x = (n as u64 * 17 + 1) % side;
    let y = (n as u64 * 29) % side;
    let fu = [0, 1, 127, 128, 255][n / 3 % 5];
    let fv = [255, 128, 127, 1, 0][n / 7 % 5];
    Input {
        owner: Bits::new(
            (511 - coarse)
                | (coarse << 9)
                | (u64::from(nearest) << 18)
                | (level << 19)
                | (level.saturating_sub(1) << 23)
                | (((n % 64) as u64) << 31)
                | (u64::from(coarse == 0) << 37),
            38,
        ),
        result: Bits::new(fu | (fv << 8) | (x << 16) | (y << 26), 36),
        ..Default::default()
    }
}
fn golden(i: Input) -> (u64, [u64; 2], [(u8, u64); 2]) {
    let owner = i.owner.value;
    let n = (owner >> 19) & 15;
    let key = (owner >> 31) & 63;
    let x = (i.result.value >> 16) & 1023;
    let y = (i.result.value >> 26) & 1023;
    let fu = i.result.value & 255;
    let fv = i.result.value >> 8 & 255;
    let nearest = owner >> 18 & 1 != 0;
    let meta = (key << 30)
        | (n << 22)
        | (u64::from(y.is_multiple_of(2) && fv < 128) << 21)
        | (u64::from(x.is_multiple_of(2) && fu < 128) << 20)
        | (y << 10)
        | x;
    let mut masks = [0u64; 2];
    let mut writes = [(0, 0); 2];
    for plane in 0..2 {
        let p = if plane == 0 {
            owner & 511
        } else {
            owner >> 9 & 511
        };
        let f = |coordinate: u64, fraction: u64| {
            if nearest {
                0
            } else if plane == 1 && n > 1 {
                (((coordinate * 256 + fraction + 128) / 2) as i64 - 128).rem_euclid(256) as u64
            } else {
                fraction
            }
        };
        let u = f(x, fu);
        let v = f(y, fv);
        let bottom = p * v / 256;
        let top = p - bottom;
        let br = bottom * u / 256;
        let tr = top * u / 256;
        let corners = [top - tr, tr, bottom - br, br];
        let mut weights = 0;
        for (c, w) in corners.into_iter().enumerate() {
            weights |= w << (c * 9);
            masks[plane] |= u64::from(w != 0) << (c * 9);
        }
        assert_eq!(corners.into_iter().sum::<u64>(), p);
        writes[plane] = ((key * 2 + ((n + plane as u64) % 2)) as u8, weights);
    }
    (meta, masks, writes)
}
fn run(native: bool) -> Vec<Edge> {
    let mut c = if native {
        Coefficient::native()
    } else {
        Coefficient::default()
    };
    let mut rows = Vec::new();
    let mut expected: VecDeque<(u64, [u64; 2])> = VecDeque::new();
    let mut scheduled: BTreeMap<usize, (u8, u64)> = BTreeMap::new();
    let mut enabled = 0;
    let mut n = 0;
    let mut writes = 0;
    let mut consumed = 0;
    let mut full = 0;
    for wall in 0..2500 {
        let reset = wall == 0 || wall == 391;
        let i = Input {
            reset,
            proposal_ce: !reset && wall % 5 != 2,
            commit_allow: wall % 7 != 3,
            valid: !reset && n < 192,
            available: if wall % 19 < 5 { 0 } else { 16 },
            out_ready: wall > 35 && wall % 17 > 5,
            ..request(n)
        };
        let o = c.outputs(i);
        if !reset && o.valid {
            let (meta, masks) = *expected.front().unwrap();
            assert_eq!(o.meta, Bits::new(meta, 36), "metadata wall{wall}");
            assert_eq!(
                [o.fine, o.coarse],
                masks.map(|v| Bits::new(v, 36)),
                "masks wall{wall}"
            );
            if o.consumed {
                expected.pop_front();
                consumed += 1;
            }
        }
        if !reset && o.advance {
            let expected_write = scheduled.remove(&enabled);
            assert_eq!(
                o.weight_write,
                expected_write.is_some(),
                "write date wall{wall}"
            );
            if let Some((address, value)) = expected_write {
                assert_eq!(
                    o.weight_address,
                    Bits::new(address.into(), 7),
                    "address wall{wall}"
                );
                assert_eq!(o.weight_value, Bits::new(value, 36), "weights wall{wall}");
                writes += 1;
            }
        }
        if o.accepted {
            let (meta, masks, w) = golden(i);
            expected.push_back((meta, masks));
            assert!(scheduled.insert(enabled + 3, w[0]).is_none());
            if i.owner.value >> 19 & 15 != 0 {
                assert!(scheduled.insert(enabled + 4, w[1]).is_none());
            }
            assert_eq!(
                o.reserved,
                u8::from(i.owner.value & 511 != 0) + u8::from(i.owner.value >> 9 & 511 != 0)
            );
        }
        rows.push(Edge { i, o, s: c.clone() });
        c.clock(i)
            .unwrap_or_else(|e| panic!("wall{wall} case{n}: {e}"));
        full += usize::from(c.queued == 2);
        if reset {
            expected.clear();
            scheduled.clear();
            enabled = 0;
        } else if o.advance {
            enabled += 1;
        }
        if o.accepted {
            n += 1;
        }
        if n == 192 && c.queued == 0 && c.cohorts == 0 {
            break;
        }
    }
    assert!(
        n == 192
            && writes > 300
            && consumed > 180
            && full > 10
            && expected.is_empty()
            && scheduled.is_empty()
    );
    println!(
        "COEFFICIENT walls={} cohorts={n} consumed={consumed} weight_writes={writes} full={full}",
        rows.len()
    );
    rows
}
#[test]
fn scheduled_coefficient_writes_conserve_integer_plane_weights() {
    run(false);
    run(true);
}
fn root() -> std::path::PathBuf {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-draw")
        .join(format!(
            "coefficient-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[test]
#[ignore = "two finite actual direct-weight coefficient profiles"]
fn sole_weight_writes_match_actual_command_coefficient() {
    let root = root();
    for native in [false, true] {
        let rows = run(native);
        let p = fixture::fields(&rows[0]);
        vertex_support::rtl::replay(
            &root,
            if native { "hard_dsp" } else { "portable" },
            rows.iter().map(fixture::fields).collect(),
            fixture::production(&p, native),
            false,
            None,
        );
    }
    println!("COEFFICIENT_ROOT {}", root.display());
}
#[test]
#[ignore = "two compiled weight/mask mutations and original raw proposal monitor"]
fn immutable_weight_and_publication_boundaries_detect_counterexamples() {
    let rows = run(false);
    let root = root();
    let pins = fixture::fields(&rows[0]);
    let source = fixture::production(&pins, false);
    for (name, old, new) in [
        (
            "mip_parity_address",
            "assign direct_weight_address={weight_key1[5:0],weight_key1[6]^(v4&&!weight_key1[7])};",
            "assign direct_weight_address={weight_key1[5:0],weight_key1[6]};",
        ),
        (
            "mask_write_is_E4",
            "wire ready_we=!reset && advance && v4",
            "wire ready_we=!reset && advance && v3",
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
    let mut d = Coefficient::default();
    let mut bad_rows = Vec::new();
    for wall in 0..2 {
        let i = Input {
            reset: wall == 0,
            proposal_ce: wall == 1,
            commit_allow: false,
            valid: wall == 1,
            available: 17,
            ..request(0)
        };
        bad_rows.push(Edge {
            i,
            o: d.outputs(i),
            s: d.clone(),
        });
        let result = d.clock(i);
        if wall == 1 {
            assert_eq!(result.unwrap_err().rule, "GPU contract: preflight_bad");
        } else {
            result.unwrap();
        }
    }
    vertex_support::rtl::replay(
        &root,
        "work_credit_while_commit_refused",
        bad_rows.iter().map(fixture::fields).collect(),
        source,
        false,
        Some("GPU contract: preflight_bad"),
    );
    println!("COEFFICIENT_NEGATIVE_ROOT {}", root.display());
}
