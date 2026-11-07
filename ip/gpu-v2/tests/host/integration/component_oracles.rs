use gpu_v2::{
    framebuffer::{
        ports as fb,
        sim::{functional, oracle as rop},
    },
    lighting::{
        ports::*,
        sim::{counted, oracle as lighting},
    },
    triangle::{ports as tp, sim::oracle as triangle},
};

#[test]
fn compact_transport_all_codes_roundtrip_and_counted_golden() {
    for code in -2048..=2047 {
        let p = CompactPixelInput {
            normal: [code, -1, 2047],
            ndc: [-16384, 16384],
        };
        assert_eq!(CompactPixelInput::from_rows(p.rows().unwrap()).unwrap(), p);
    }
    for raw in i16::MIN..=i16::MAX {
        let p = CompactPixelInput::from_q14(PixelInput {
            normal: [raw, 0, 0],
            ndc: [0, 0],
        })
        .unwrap();
        let floor = i32::from(raw).div_euclid(16);
        let remainder = i32::from(raw).rem_euclid(16);
        let expected = (floor + i32::from(remainder > 8 || remainder == 8 && floor & 1 != 0))
            .clamp(-2048, 2047);
        assert_eq!(i32::from(p.normal[0]), expected);
    }
    assert!(CompactPixelInput {
        normal: [2048, 0, 0],
        ndc: [0, 0]
    }
    .rows()
    .is_err());
    assert!(CompactPixelInput::from_rows([1 << 36, 0]).is_err());
    for code in 0..17 {
        for normal in [[-2048, 1, 2047], [377, -286, 939], [0, 0, 0], [1, -1, 0]] {
            let p = CompactPixelInput {
                normal,
                ndc: [7864, -4332],
            };
            let material = Material {
                shininess_code: code,
                ..Default::default()
            };
            let report = counted::evaluate_compact(
                p,
                material,
                Light::default(),
                Projection::default(),
                4096,
            )
            .unwrap();
            report.frame.audit().unwrap();
            let out = lighting::evaluate_output(
                p.expanded().unwrap(),
                material,
                Light::default(),
                Projection::default(),
                lighting::Config {
                    rounding: lighting::RoundingPolicy {
                        power: lighting::Rounding::Floor,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(report.output, out);
            let wide = counted::evaluate_with_config(
                p.expanded().unwrap(),
                material,
                Light::default(),
                Projection::default(),
                4096,
                counted::Config::architecture(),
            )
            .unwrap();
            let stages = |r: &counted::Report| {
                r.frame
                    .outputs
                    .iter()
                    .map(|o| (o.name.clone(), o.raw))
                    .collect::<Vec<_>>()
            };
            assert_eq!(stages(&report), stages(&wide));
            let memory = report
                .frame
                .memories
                .iter()
                .find(|m| m.name == "pixel.compact-rows")
                .unwrap();
            assert_eq!(memory.rows, 2);
        }
    }
    let diffuse = counted::evaluate_compact(
        CompactPixelInput {
            normal: [300, 0, 1000],
            ndc: [0, 0],
        },
        Material {
            specular_color: [0; 3],
            ..Default::default()
        },
        Light::default(),
        Projection::default(),
        4096,
    )
    .unwrap();
    let memory = diffuse
        .frame
        .memories
        .iter()
        .position(|m| m.name == "pixel.compact-rows")
        .unwrap();
    let reads = diffuse
        .frame
        .events
        .iter()
        .filter(|e| matches!(e.operation,audited::Operation::Read {memory:m,..} if m==memory))
        .count();
    assert_eq!(reads, 1);
}

#[test]
fn continuous_setup_reuses_integer_backend_and_independent_solve() {
    let clip = [
        [-0.8, -0.4, 0.0, 1.0],
        [0.7, -0.3, 0.0, 1.2],
        [0.2, 0.7, 0.0, 1.1],
    ];
    let attrs = [
        [0.0, 0.0, 0.3, 0.4, 0.5, 0.1, 0.3, 0.9],
        [1.0, 0.0, 0.3, 0.4, 0.5, -0.3, 0.1, 0.8],
        [0.5, 1.0, 0.3, 0.4, 0.5, 0.2, -0.2, 0.7],
    ];
    let r = triangle::run_continuous(7, clip, attrs, tp::Config::default()).unwrap();
    for p in [[200.5, 120.5], [180.5, 100.5], [230.5, 133.5]] {
        let a = r.evaluate(p).unwrap();
        let b = r.reference(p).unwrap();
        for (x, y) in a.normal.into_iter().zip(b.normal) {
            assert!((x - y).abs() < 1e-12);
        }
        assert!((a.w - b.w).abs() < 1e-12);
    }
    assert!(triangle::run_continuous(0, [[f64::NAN; 4]; 3], attrs, tp::Config::default()).is_err());
}

#[test]
fn functional_framebuffer_eviction_and_blend_match_direct_surface() {
    let clear = rop::Pixel {
        color: 0x1234,
        depth: 65535,
    };
    let mut cache = functional::Cache::new(400, 240, clear).unwrap();
    let mut direct = vec![clear; 400 * 240];
    let c = fb::Context {
        depth: fb::DepthFunc::Less,
        depth_write: true,
        blend: fb::Blend::SrcOver,
    };
    for i in 0..300 {
        let x = (i * 137 % 400) as u16;
        let y = (i * 83 % 240) as u16;
        let source = fb::Fragment {
            rgba: [i as u8, 127, 233, 177],
            depth: (500 - i) as u16,
        };
        let index = y as usize * 400 + x as usize;
        direct[index] = rop::pixel(direct[index], source, true, c).pixel;
        cache.apply(x, y, source, c).unwrap();
    }
    assert_eq!(cache.materialize(), direct);
    assert!(cache.writebacks > 8);
}
