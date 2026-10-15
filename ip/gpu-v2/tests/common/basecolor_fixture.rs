use gpu_v2::{
    material_color::{Albedo, ColorSource as Mode},
    triangle::{
        ports::{Config, Input},
        sim::oracle::{self, Report},
    },
    vertex::ports::Transformed,
};
pub const SCENES: usize = 7;
pub fn fixture(scene: usize, phase: f64) -> (Report, [Albedo; 3], Mode) {
    let shift = (phase * std::f64::consts::TAU).sin();
    let width = 400.0;
    let height = 240.0;
    let (screen, w, mode) = match scene {
        0 => (
            [
                [55.0 + 18.0 * shift, 40.0],
                [325.0, 55.0 + 8.0 * shift],
                [115.0, 218.0],
            ],
            [1.0, 1.25, 0.9],
            Mode::Texture,
        ),
        1 => (
            [[12.0, 10.0], [388.0, 22.0], [28.0 + 10.0 * shift, 231.0]],
            [1.0, 1.1, 0.9],
            Mode::Texture,
        ),
        2 => (
            [[35.0 + 10.0 * shift, 40.0], [365.0, 42.0], [120.0, 226.0]],
            [0.3, 2.8, 1.2],
            Mode::Texture,
        ),
        3 => (
            [[55.0 + 14.0 * shift, 40.0], [335.0, 50.0], [180.0, 222.0]],
            [0.8, 1.4, 1.0],
            Mode::VertexColor,
        ),
        4 => (
            [[65.0, 28.0], [355.0, 44.0], [180.0 + 24.0 * shift, 224.0]],
            [1.0, 1.2, 0.85],
            Mode::VertexColor,
        ),
        5 => (
            [[55.0, 118.0 + shift], [340.0, 124.0], [92.0, 133.0 + shift]],
            [0.55, 1.8, 0.9],
            Mode::Texture,
        ),
        _ => (
            [[-45.0, 15.0], [430.0, 35.0], [135.0 + 12.0 * shift, 267.0]],
            [0.7, 1.4, 1.0],
            Mode::Texture,
        ),
    };
    let uv0 = [[200, 100], [3600, 400], [600, 3800]];
    let uv = uv0.map(|v| v.map(|q| (f64::from(q) + 140.0 * shift).round_ties_even() as u16));
    let rgb = [[224, 90, 36], [40, 224, 120], [70, 60, 245]];
    let albedo = std::array::from_fn(|i| {
        if mode == Mode::Texture {
            Albedo::Texture(uv[i])
        } else {
            Albedo::Color(rgb[i])
        }
    });
    let angle = 0.3 * shift;
    let normals0 = [
        [-320.0, 128.0, 960.0],
        [320.0, 256.0, 832.0],
        [-128.0, -384.0, 1000.0],
    ];
    let normal = normals0.map(|n| {
        [
            (n[0] * angle.cos() + n[2] * angle.sin()).round_ties_even() as i16,
            n[1] as i16,
            (-n[0] * angle.sin() + n[2] * angle.cos()).round_ties_even() as i16,
        ]
    });
    let input = Input {
        id: scene as u32,
        vertices: std::array::from_fn(|i| Transformed {
            clip: [
                ((screen[i][0] / width * 2.0 - 1.0) * w[i] * 65536.0).round_ties_even() as i32,
                ((1.0 - screen[i][1] / height * 2.0) * w[i] * 65536.0).round_ties_even() as i32,
                0,
                (w[i] * 65536.0).round_ties_even() as i32,
            ],
            normal: normal[i],
            uv: uv[i],
            rgb565: 0,
        }),
    };
    let config = Config {
        guard_log2: if scene == 6 { 0 } else { 3 },
        ..Config::default()
    };
    let mut r = oracle::run(&input, config).unwrap();
    // Basis geometry cache is independent of attributes. Extend only this test fixture's
    // continuous attribute truth to RGB888; original oracle source stays unchanged.
    if let Some(s) = &mut r.source {
        for (attributes, color) in s.attributes.iter_mut().zip(rgb) {
            for (attribute, code) in attributes[2..5].iter_mut().zip(color) {
                *attribute = f64::from(code) / 255.0;
            }
        }
        s.constant_channels = std::array::from_fn(|k| {
            (s.attributes[0][k] == s.attributes[1][k] && s.attributes[0][k] == s.attributes[2][k])
                .then_some(s.attributes[0][k])
        });
    }
    (r, albedo, mode)
}
pub fn lod(uv: [[f64; 2]; 4], extent: f64) -> f64 {
    let uv = uv.map(|p| p.map(|v| (v * 65536.0).round_ties_even()));
    let mut rho = 0.0f64;
    for (a, b) in [(0, 1), (2, 3), (0, 2), (1, 3)] {
        for (&u, &v) in uv[a].iter().zip(&uv[b]) {
            rho = rho.max((u - v).abs());
        }
    }
    (rho * extent / 65536.0).log2().max(0.0)
}
