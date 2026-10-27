use super::shared_common_basecolor_uv12_fixture as common;
use gpu_v2::{
    material_color::ColorSource as Mode,
    triangle::{
        basecolor_threefield::{PreparedBasis, Profile},
        basecolor_uv12::{recover, term_q16, SplitWeight, Work},
    },
};

#[test]
fn split_weight_single_round_and_real_unsigned_products() {
    let mut cases = 0;
    for bits in 9..=12 {
        for code in -32768..=32768 {
            let v = f64::from(code) / 8192.0;
            let w = SplitWeight::quantize(v, bits).unwrap();
            assert!(w.high9 <= 511 && w.low3 <= 7);
            assert_eq!(w.negative, v.is_sign_negative());
            assert!(
                (w.value() - v).abs() <= 2.0f64.powi(i32::from(w.exponent) - i32::from(bits) - 1)
            );
            for delta in [-4095, -511, -1, 0, 1, 511, 4095] {
                let mut work = Work::default();
                let term = term_q16(delta, 0, w, &mut work).unwrap();
                // Independent full-width exact dyadic expression; no split code is used.
                let gold = (f64::from(delta) * w.value() * 65536.0).round_ties_even() as i128;
                assert_eq!(term, gold, "bits={bits} v={v} delta={delta}");
                cases += 1;
            }
        }
    }
    let w = SplitWeight::quantize(4095.0 / 4096.0, 12).unwrap();
    assert_eq!((w.high9, w.low3), (511, 7));
    assert_eq!(w.value(), 4095.0 / 4096.0);
    // A rounded Q9 high part would become an endpoint and lose the low correction.
    assert_eq!(
        gpu_v2::triangle::basecolor_threefield::Weight9::quantize(w.value(), Profile::Q9)
            .unwrap()
            .value(Profile::Q9),
        1.0
    );
    let mut work = Work::default();
    assert_eq!(term_q16(511, 0, w, &mut work).unwrap(), 511 * 4095 * 16);
    assert_eq!(
        (
            work.unsigned9_products,
            work.fabric9x3_terms,
            work.fabric_adds
        ),
        (1, 1, 3)
    );
    for endpoint in [-8.0, -2.0, -1.0, 1.0, 2.0, 8.0] {
        let w = SplitWeight::quantize(endpoint, 12).unwrap();
        assert!(w.endpoint);
        let mut work = Work::default();
        assert_eq!(
            term_q16(-511, 4, w, &mut work).unwrap(),
            (-511.0 * 16.0 * endpoint * 65536.0) as i128
        );
        assert_eq!(work.unsigned9_products, 0);
        assert_eq!(work.fabric9x3_terms, 0);
    }
    assert!(SplitWeight::quantize(f64::NAN, 12).is_err());
    assert!(SplitWeight::quantize(0.5, 13).is_err());
    println!(
        "PASS split-product independent cases={cases} unsigned511 low7 signs endpoints/exponents"
    );
}

#[test]
fn finite_uv12_helpers_and_independent_color_normal_q9() {
    let (mut points, mut negative, mut fans) = (0, 0, 0);
    let mut work = Work::default();
    let mut max_lod = 0f64;
    for scene in 0..common::SCENES {
        for frame in 0..12 {
            let (r, a, mode) = common::fixture(scene, frame as f64 / 12.0);
            let b = PreparedBasis::prepare(&r, a, mode).unwrap();
            fans += r.triangles.len();
            for y in (16..224).step_by(26) {
                for x in (16..384).step_by(46) {
                    let pp = std::array::from_fn::<_, 4, _>(|i| {
                        [
                            x as f64 + 0.5 + (i % 2) as f64,
                            y as f64 + 0.5 + (i / 2) as f64,
                        ]
                    });
                    if mode == Mode::Texture && pp.iter().all(|p| r.evaluate(*p).is_ok()) {
                        let golden = pp.map(|p| r.evaluate(p).unwrap().uv);
                        for narrow in [false, true] {
                            let uv = pp.map(|p| recover(&b, p, Some(12), narrow).unwrap().uv());
                            max_lod = max_lod
                                .max((common::lod(uv, 256.0) - common::lod(golden, 256.0)).abs());
                        }
                    }
                    for point in pp {
                        let gold = r.reference(point);
                        if gold.is_err() {
                            assert!(recover(&b, point, Some(12), true).is_err());
                            continue;
                        }
                        let exact = b.recover(point, Profile::Exact, false).unwrap();
                        let gold = gold.unwrap();
                        for (v, w) in exact.beta.into_iter().zip(gold.beta) {
                            assert!((v - w).abs() < 1e-8);
                        }
                        for bits in 9..=12 {
                            for narrow in [false, true] {
                                let v = recover(&b, point, Some(bits), narrow).unwrap();
                                let old = b.recover(point, Profile::Q9, narrow).unwrap();
                                assert_eq!(v.normal(mode), old.normal(mode));
                                if mode == Mode::VertexColor {
                                    assert_eq!(v.rgb(), old.rgb());
                                }
                                assert_eq!(v.w, exact.w);
                                assert_eq!(v.beta, exact.beta);
                                if mode == Mode::Texture {
                                    assert_eq!(
                                        v.uv_transport_q16().unwrap(),
                                        v.uv().map(|u| (u * 65536.0).round_ties_even() as i128)
                                    );
                                }
                                for (integer, float) in
                                    v.integer_q16.unwrap().iter().zip(v.raw).take(b.channels)
                                {
                                    assert!(
                                        (*integer as f64 / 65536.0 - float).abs()
                                            <= 0.5001 / 65536.0
                                    );
                                }
                                work.unsigned9_products += v.work.unsigned9_products;
                                work.fabric9x3_terms += v.work.fabric9x3_terms;
                                work.fabric_adds += v.work.fabric_adds;
                                negative += usize::from(v.beta.iter().any(|v| *v < 0.0));
                            }
                        }
                        points += 1;
                    }
                }
            }
        }
    }
    let (r, a, mode) = common::fixture(0, 0.1);
    let b = PreparedBasis::prepare(&r, a, mode).unwrap();
    for p in [[4097.0, 0.0], [f64::NAN, 0.0]] {
        assert!(recover(&b, p, Some(12), true).is_err());
    }
    // Constant bypass keeps full source codes, not nine-bit approximations.
    let (mut r, _, _) = common::fixture(3, 0.1);
    for a in &mut r.source.as_mut().unwrap().attributes {
        a[5] = 0.5;
        a[6] = -0.25;
        a[7] = 900.0 / 1024.0;
    }
    let a = [gpu_v2::material_color::Albedo::Color([7, 13, 251]); 3];
    let b = PreparedBasis::prepare(&r, a, Mode::VertexColor).unwrap();
    let v = recover(&b, [-20.5, 20.5], Some(12), true).unwrap();
    assert_eq!(v.raw, [7.0, 13.0, 251.0, 512.0, -256.0, 900.0]);
    assert_eq!(v.work, Work::default());
    assert!(points > 16000 && negative > 0 && fans > 84);
    println!(
        "PASS finite UV12 helper points={points} negative-profile-evaluations={negative} source-fans={fans} diagnostic outside-helper maxLOD={max_lod} actual unsigned9products={} fabric9x3terms={} fabricadds={}",
        work.unsigned9_products, work.fabric9x3_terms, work.fabric_adds
    );
}
