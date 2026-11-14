//! Four-phase compute with real parameters; samples are explicit bounded peers.
#[allow(dead_code)]
#[path = "../support/vliw_cycle/mod.rs"]
mod transport_support;
#[allow(dead_code)]
#[path = "../support/vliw_vertex/mod.rs"]
mod vertex_support;
use gpu_v2::system::vliw_cycle::{
    compute::{Compute, Input, Output},
    vertex::Bits,
};
#[path = "../support/vliw_compute.rs"]
mod fixture;
struct Edge {
    i: Input,
    o: Output,
    p: Compute,
}
fn attributes(rgb: bool) -> [[i64; 6]; 3] {
    let mut a = [
        [-512, 200, 900, 1000, 30000, 0],
        [700, -400, 500, 60000, 1200, 0],
        [5, 15, -200, 25000, 10000, 0],
    ];
    if rgb {
        a[0][3..].copy_from_slice(&[17, 100, 200]);
        a[1][3..].copy_from_slice(&[250, 120, 40]);
        a[2][3..].copy_from_slice(&[91, 21, 150]);
    }
    a
}
const H: [[u32; 3]; 4] = [
    [16384, 24576, 32768],
    [50000, 7002, 90000],
    [1024, 0, 0],
    [0, 0, 0],
];
fn run(rgb: bool) -> Vec<Edge> {
    run_bias(rgb, 8)
}
fn run_bias(rgb: bool, depth_bias: i8) -> Vec<Edge> {
    let attrs = attributes(rgb);
    let mut p = Compute::default();
    let mut rows = Vec::new();
    let mut tick = 0;
    for wall in 0..10000 {
        let phase = p.state.phase;
        let mut i = Input {
            reset: wall < 2,
            ce: wall % 7 > 1,
            read_bank: false,
            write_bank: false,
            cfg_far: 20 << 16,
            cfg_rgb: rgb,
            cfg_z_bias: depth_bias,
            cfg_tex_u: 6,
            cfg_tex_v: 5,
            cfg_max_mip: 6,
            cfg_bias: -3,
            ..Default::default()
        };
        if wall >= 2 {
            if tick < 6 {
                let k = tick as usize;
                let base = attrs[2][k];
                let d0 = attrs[0][k] - base;
                let d1 = attrs[1][k] - base;
                i.param_valid = true;
                i.param_index = (k / 2) as u8;
                i.param_bank = !k.is_multiple_of(2);
                i.param_data = Bits::new(
                    ((base as u64 & 0x1ffff) << 34)
                        | ((d1 as u64 & 0x1ffff) << 17)
                        | (d0 as u64 & 0x1ffff),
                    51,
                );
            }
            if (6..9).contains(&tick) {
                let n = (tick - 6) as usize;
                i.coefficient_bwe = true;
                i.coefficient_bwa = [0, 1, 4][n];
                i.coefficient_bwd = Bits::new([-8i64, 4, 1 << 28][n] as u64, 36);
            }
            if (6..10).contains(&tick) {
                i.coefficient_cwe = true;
                i.coefficient_cwa = (tick - 6) as u8;
                i.coefficient_cwd = Bits::new(
                    [1i64 << 30, 1 << 29, -(1 << 28), 1 << 31][(tick - 6) as usize] as u64,
                    36,
                );
            }
            i.cfg_valid = tick == 10;
            if (16..176).contains(&tick) {
                let pixel = (tick - 16) / 4;
                let lane = pixel % 4;
                let q = pixel / 4;
                i.sample_valid = phase == 0;
                i.sample_pid = (pixel & 31) as u8;
                i.sample_h = if phase < 3 {
                    H[lane as usize][phase as usize]
                } else {
                    0
                };
                i.sample_cover = true;
                i.sample_anchor = lane == 0;
                i.quad_end_valid = lane == 3 && phase == 2;
                i.quad_end_qid = (q & 7) as u8;
                i.quad_end_mask = 15;
            }
        }
        let o = p.outputs(i);
        rows.push(Edge { i, o, p: p.clone() });
        p.clock(i)
            .unwrap_or_else(|e| panic!("rgb{rgb}/wall{wall}/tick{tick}/phase{phase}: {e}"));
        if i.ce && !i.reset {
            tick += 1;
        }
        if tick >= 220 {
            assert!(!p.outputs(Input::default()).busy);
            return rows;
        }
    }
    panic!("compute exceeded10000 wall edges");
}
fn rne(n: u64, d: u64) -> u64 {
    let q = n / d;
    let r = n % d;
    q + u64::from(r > d - r || (r == d - r && q & 1 != 0))
}
fn expected(rgb: bool, lane: usize) -> ([u64; 3], u16, bool) {
    let h = H[lane];
    let d: u64 = h.into_iter().map(u64::from).sum();
    let positive = d.max(1);
    let e = 63 - positive.leading_zeros();
    let mant = (positive << 17) >> e;
    let j = (mant >> 9) & 255;
    let frac = mant & 511;
    let seed = rne(1 << 25, 256 + j);
    let next = rne(1 << 25, 257 + j);
    let r = seed - rne((seed - next) * frac, 512);
    let w0 = ((r * ((u64::from(h[0]) << 17) >> (e + 1))) >> 17).min(65536);
    let w1 = ((r * ((u64::from(h[1]) << 17) >> (e + 1))) >> 17).min(65536 - w0);
    let attrs = attributes(rgb);
    let a: [u64; 6] = std::array::from_fn(|k| {
        let sum = attrs[2][k] * 65536
            + (attrs[0][k] - attrs[2][k]) * w0 as i64
            + (attrs[1][k] - attrs[2][k]) * w1 as i64
            + 32768;
        (sum >> 16) as u64 & 0x3ffff
    });
    let zshift = 8 + e;
    let wide = (1u64 << 28) * r;
    let overflow = zshift < 15 || (zshift == 15 && wide & (1 << 47) != 0);
    let zword = if !(15..48).contains(&zshift) {
        0
    } else {
        (wide >> (zshift + 8)) & 0x1ffffff
    };
    let z = zword as u16;
    let pass = d != 0 && !overflow && zword >> 16 == 0 && z <= 5120;
    let out = [
        (u64::from(pass) << 35) | ((a[1] & 4095) << 12) | (a[0] & 4095),
        ((a[3] & if rgb { 255 } else { 0x3ffff }) << 12) | (a[2] & 4095),
        if rgb {
            (u64::from(z) << 16) | ((a[5] & 255) << 8) | (a[4] & 255)
        } else {
            (u64::from(z) << 18) | a[4]
        },
    ];
    (out, z, pass)
}

// Integer binary-log refinement at Q63, rounded to Q8. It independently
// generates the mathematical values, without reading the candidate ROM.
fn log_q8(index: u64) -> u16 {
    let mut value = u128::from(64 + index) << 57;
    let mut fraction = 0u32;
    for _ in 0..24 {
        value = (value * value) >> 63;
        fraction <<= 1;
        if value >= 1u128 << 64 {
            value >>= 1;
            fraction |= 1;
        }
    }
    ((fraction + 32768) >> 16) as u16
}
fn expected_lod() -> u8 {
    let (out, _, _) = expected(false, 0);
    let anchor = [(out[1] >> 12) & 0x3ffff, out[2] & 0x3ffff];
    let gradients = [1i64 << 30, 1 << 29, -(1 << 28), 1 << 31];
    let mut maximum = 0i64;
    for (axis, g) in gradients.into_iter().enumerate() {
        let value = (g + anchor[axis / 2] as i64 * [-8, 4][axis % 2]).unsigned_abs();
        if value != 0 {
            let exp = 63 - value.leading_zeros();
            let mant = (value << 17) >> exp;
            let j = (mant >> 11) & 63;
            maximum = maximum.max(i64::from(exp + [6, 5][axis / 2]) * 256 + i64::from(log_q8(j)));
        }
    }
    let d: u64 = H[0].into_iter().map(u64::from).sum();
    let exp = 63 - d.leading_zeros();
    let mant = (d << 17) >> exp;
    let dlog = i64::from(exp) * 256 + i64::from(log_q8(((mant >> 9) & 255) / 4));
    ((maximum - 3712 - 48 - dlog).clamp(0, 6 * 256) >> 5) as u8
}
#[test]
fn reciprocal_weights_depth_and_attributes_match_integer_reference() {
    for rgb in [true, false] {
        let rows = run(rgb);
        let result: Vec<_> = rows.iter().filter(|r| r.i.ce && r.o.result_we).collect();
        let depth: Vec<_> = rows.iter().filter(|r| r.i.ce && r.o.depth_valid).collect();
        let lod: Vec<_> = rows.iter().filter(|r| r.i.ce && r.o.lod_valid).collect();
        assert_eq!(result.len(), 120);
        assert_eq!(depth.len(), 40);
        assert_eq!(lod.len(), 10);
        assert!(
            rows.iter().any(|r| r.i.ce
                && r.o.phase == 0
                && r.p.state.valid_pipe & 2 != 0
                && r.p.multiply.result & 256 != 0
                && (r.p.multiply.result & 255 != 0 || r.p.multiply.result & 512 != 0)),
            "Newton RNE increment was not exercised"
        );
        for (n, r) in result.into_iter().enumerate() {
            let (want, _, _) = expected(rgb, n / 3 % 4);
            assert_eq!(r.o.result_address, (((n / 3) & 31) * 4 + n % 3) as u8);
            assert_eq!(
                r.o.result_data.get(36, "computed attribute").unwrap(),
                want[n % 3],
                "rgb{rgb}/result{n}"
            );
        }
        for (n, r) in depth.into_iter().enumerate() {
            let (_, z, pass) = expected(rgb, n % 4);
            assert_eq!(
                (r.o.depth_z16, r.o.depth_pass),
                (z, pass),
                "rgb{rgb}/depth{n}"
            );
        }
        assert!(rows
            .iter()
            .any(|r| r.i.ce && r.o.depth_valid && r.o.depth_pass));
        assert!(rows
            .iter()
            .any(|r| r.i.ce && r.o.depth_valid && !r.o.depth_pass));
        for (n, r) in lod.into_iter().enumerate() {
            assert_eq!(r.o.lod_id, (n & 7) as u8);
            if rgb {
                assert_eq!(r.o.lod_value, 0);
            } else {
                assert_eq!(r.o.lod_value, expected_lod());
            }
        }
    }
}

#[test]
fn internal_zero_depth_boundary_preserves_positive_d16_zero_and_ce_hold() {
    for reciprocal in [65536u64, 131072] {
        for k in [1u64 << 30, (1 << 31) - 1] {
            let product = reciprocal * k;
            for shift in 45..=48 {
                let mut p = Compute::default();
                p.state.phase = 3;
                p.state.valid_pipe = 4;
                p.state.cover_pipe = 4;
                p.state.z_shift = shift;
                p.wide.result = Bits::new(product, 54);
                let input = Input {
                    cfg_far: 80 << 16,
                    ..Input::default()
                };
                p.clock(input).unwrap();
                assert!(!p.state.z_early_pass, "CE0 must retain the old result");
                p.clock(Input { ce: true, ..input }).unwrap();
                assert_eq!(p.state.z_early_pass, product >> shift > 0);
                assert_eq!(p.state.z_early, 0);
            }
        }
    }
}

#[test]
#[ignore = "compiled numeric RAM/Newton correction/attribute DSP mutations"]
fn compute_negative_controls_are_sensitive() {
    use std::{
        fs,
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-draw")
        .join(format!(
            "compute-mutants-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&root).unwrap();
    let rows = run(false);
    let pins = fixture::fields(&rows[0]);
    for (name, module, old, new) in [
        (
            "combinational_M1",
            "iz_numeric_ram",
            "assign q=rd;",
            "assign q=memory[ra];",
        ),
        (
            "truncated_Newton",
            "rc_compute",
            "wire correction_inc=mul_result[8]&&((|mul_result[7:0])||mul_result[9]);",
            "wire correction_inc=1'b0;",
        ),
        (
            "live_pair_C",
            "iz_pairmac",
            "r<=sw0*bd0+sw1*bd1+bc;",
            "r<=sw0*bd0+sw1*bd1+base;",
        ),
    ] {
        let mut source = fixture::production(&pins);
        let start = source.find(&format!("module {module}(")).unwrap();
        let end = start + source[start..].find("endmodule").unwrap();
        let body = &source[start..end];
        assert_eq!(body.matches(old).count(), 1);
        let body = body.replacen(old, new, 1);
        source.replace_range(start..end, &body);
        vertex_support::rtl::replay(
            &root,
            name,
            rows.iter().map(fixture::fields).collect(),
            source,
            true,
            None,
        );
    }
    println!("COMPUTE_MUTANTS {}", root.display());
}

#[test]
#[ignore = "actual production four-phase compute/RAM/DSP/LOD closed loop"]
fn every_compute_and_lod_edge_matches_production() {
    use std::{
        fs,
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-draw")
        .join(format!(
            "compute-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&root).unwrap();
    for rgb in [true, false] {
        let rows = run(rgb);
        let pins = fixture::fields(&rows[0]);
        vertex_support::rtl::replay(
            &root,
            &format!("rgb{rgb}"),
            rows.iter().map(fixture::fields).collect(),
            fixture::production(&pins),
            false,
            None,
        );
    }
    for bias in 28..=32 {
        let rows = run_bias(true, bias);
        let pins = fixture::fields(&rows[0]);
        vertex_support::rtl::replay(
            &root,
            &format!("depth-bias-{bias}"),
            rows.iter().map(fixture::fields).collect(),
            fixture::production(&pins),
            false,
            None,
        );
    }
    println!("COMPUTE_CHECKPOINT {}", root.display());
}
