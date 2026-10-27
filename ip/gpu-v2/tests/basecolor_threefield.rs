use gpu_v2::{
    material_color::ColorSource as Mode,
    triangle::basecolor_threefield::{Delta9, PreparedBasis, Profile, Weight9},
};
#[path = "common/basecolor_fixture.rs"]
mod common;
#[test]
fn endpoints_sign_scale_and_integer_assembly() {
    for p in [Profile::Unorm9, Profile::Q9] {
        for code in -4096..=4096 {
            let v = f64::from(code) / 1024.0;
            let w = Weight9::quantize(v, p).unwrap();
            assert!(w.magnitude <= 511);
            assert_eq!(w.negative, v.is_sign_negative());
            let bound = 2.0f64.powi(i32::from(w.exponent))
                / if p == Profile::Unorm9 { 1022.0 } else { 1024.0 };
            assert!((w.value(p) - v).abs() <= bound + 1e-12);
        }
        for endpoint in [-1.0, 0.0, 1.0, 2.0, -4.0] {
            let w = Weight9::quantize(endpoint, p).unwrap();
            assert_eq!(w.value(p), endpoint);
            if p == Profile::Q9 && endpoint.abs() == 1.0 {
                assert!(w.endpoint);
            }
        }
    }
    for value in -4095..=4095 {
        let d = Delta9::encode(value);
        assert!(d.magnitude <= 511 && d.exponent <= 4);
        assert!((d.value() - value).abs() <= 1i32 << d.exponent);
    }
    let (r, a, m) = common::fixture(0, 0.1);
    let b = PreparedBasis::prepare(&r, a, m).unwrap();
    // Valid helper outside the triangle has a negative beta; no clamping/invalid policy.
    let mut negative = false;
    for x in [-20.5, 0.5, 30.5, 380.5, 420.5] {
        for y in [0.5, 20.5, 239.5] {
            for p in [Profile::Unorm9, Profile::Q9] {
                let v = b.recover([x, y], p, true).unwrap();
                negative |= v.beta.iter().any(|&v| v < 0.0);
                assert!(
                    (v.represented[0] + v.represented[1] + v.represented[2] - 1.0).abs() < 1e-14
                );
                let raw = b.recover_integer_q16(v.weights, p).unwrap();
                for (actual, expected) in raw.iter().zip(v.raw).take(b.channels) {
                    assert!((*actual as f64 / 65536.0 - expected).abs() <= 0.5001 / 65536.0);
                }
            }
        }
    }
    assert!(negative);
    assert!(b.source.iter().flatten().any(|&v| v > 511));
    for point in [
        [4097.0, 0.0],
        [0.0, -4097.0],
        [f64::INFINITY, 0.0],
        [f64::NAN, 0.0],
    ] {
        assert!(r.evaluate(point).is_err());
        assert!(b.recover(point, Profile::Q9, true).is_err());
    }
    let (mut r, _, _) = common::fixture(3, 0.1);
    for a in &mut r.source.as_mut().unwrap().attributes {
        a[5] = 0.5;
        a[6] = -0.25;
        a[7] = 900.0 / 1024.0;
    }
    let a = [gpu_v2::material_color::Albedo::Color([7, 13, 251]); 3];
    let b = PreparedBasis::prepare(&r, a, Mode::VertexColor).unwrap();
    for profile in [Profile::Unorm9, Profile::Q9] {
        let v = b.recover([-20.5, 20.5], profile, true).unwrap();
        assert_eq!(v.raw, [7.0, 13.0, 251.0, 512.0, -256.0, 900.0]);
        assert_eq!(v.small_products, 0);
        assert_eq!(
            b.recover_integer_q16(v.weights, profile).unwrap(),
            [7i128, 13, 251, 512, -256, 900].map(|v| v << 16)
        );
    }
}
#[test]
fn finite_threefield_unsnapped_and_mode_recovery() {
    let mut points = 0;
    let mut quads = 0;
    let mut outside = 0;
    let mut fans = 0;
    let mut max_uv = 0f64;
    let mut max_lod = 0f64;
    for scene in 0..common::SCENES {
        for frame in 0..12 {
            let (r, a, m) = common::fixture(scene, frame as f64 / 12.0);
            let b = PreparedBasis::prepare(&r, a, m).unwrap();
            let s = r.source.as_ref().unwrap();
            assert_eq!(b.fields, s.fields);
            assert_eq!(b.coefficients, s.cache.coefficients.as_slice());
            fans += r.triangles.len();
            assert_eq!(b.channels, if m == Mode::Texture { 5 } else { 6 });
            for y in (16..224).step_by(26) {
                for x in (16..384).step_by(46) {
                    let point = [x as f64 + 0.5, y as f64 + 0.5];
                    let Ok(g) = r.reference(point) else {
                        continue;
                    };
                    let e = b.recover(point, Profile::Exact, false).unwrap();
                    for (v, w) in e.beta.into_iter().zip(g.beta) {
                        assert!((v - w).abs() < 1e-8);
                    }
                    assert!((e.w - g.w).abs() < 1e-8);
                    let actual = if m == Mode::Texture {
                        e.uv().to_vec()
                    } else {
                        e.rgb().to_vec()
                    };
                    let gold = if m == Mode::Texture {
                        g.uv.to_vec()
                    } else {
                        g.rgb.to_vec()
                    };
                    for (v, w) in actual.iter().zip(gold) {
                        assert!((v - w).abs() < 1e-8);
                    }
                    for (v, w) in e.normal(m).into_iter().zip(g.normal) {
                        assert!((v - w).abs() < 1e-8);
                    }
                    if e.beta.iter().any(|&v| v < 0.0) {
                        outside += 1;
                    }
                    for p in [Profile::Unorm9, Profile::Q9] {
                        let q = b.recover(point, p, true).unwrap();
                        assert_eq!(q.w, e.w);
                        assert!(q.raw.iter().all(|v| v.is_finite()));
                        if m == Mode::Texture {
                            for (v, w) in q.uv().into_iter().zip(g.uv) {
                                max_uv = max_uv.max((v - w).abs() * 256.0);
                            }
                        }
                    }
                    points += 1;
                    if m == Mode::Texture {
                        let pp = std::array::from_fn::<_, 4, _>(|i| {
                            [point[0] + (i % 2) as f64, point[1] + (i / 2) as f64]
                        });
                        let helpers = pp.map(|p| r.evaluate(p));
                        if helpers.iter().any(|v| v.is_err()) {
                            for (p, g) in pp.into_iter().zip(helpers) {
                                if g.is_err() {
                                    assert!(b.recover(p, Profile::Exact, false).is_err());
                                }
                            }
                            continue;
                        }
                        let gold = std::array::from_fn(|i| helpers[i].as_ref().unwrap().uv);
                        let exact = pp.map(|p| b.recover(p, Profile::Exact, false).unwrap().uv());
                        assert!(
                            (common::lod(gold, 256.0) - common::lod(exact, 256.0)).abs() < 1e-8
                        );
                        for profile in [Profile::Unorm9, Profile::Q9] {
                            let uv = pp.map(|p| b.recover(p, profile, true).unwrap().uv());
                            max_lod = max_lod
                                .max((common::lod(uv, 256.0) - common::lod(gold, 256.0)).abs());
                        }
                        quads += 1;
                    }
                }
            }
        }
    }
    assert!(points > 4000 && quads > 3000 && outside > 0 && fans > 84);
    println!("PASS bounded scenes={} frames=12 points={points} quads={quads} negative-or-extrapolated={outside} source-fans={fans} maxUVtexels={max_uv} maxLOD={max_lod}",common::SCENES);
}
