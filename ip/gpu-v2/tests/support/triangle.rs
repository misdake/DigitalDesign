#![allow(dead_code)]
use gpu_v2::{triangle::ports::*, vertex::ports::Transformed};

pub const MAX_SAMPLES: usize = 40000;
pub fn config() -> Config {
    Config {
        width: 64,
        height: 40,
        max_samples: MAX_SAMPLES,
        ..Config::default()
    }
}
pub fn vertex(screen: [f64; 2], w: f64, index: usize, c: Config) -> Transformed {
    Transformed {
        clip: [
            ((2.0 * screen[0] / f64::from(c.width) - 1.0) * w * 65536.0).round_ties_even() as i32,
            ((1.0 - 2.0 * screen[1] / f64::from(c.height)) * w * 65536.0).round_ties_even() as i32,
            (index as i32 + 1) * 12345,
            (w * 65536.0).round_ties_even() as i32,
        ],
        normal: [[16384, 0, 16384], [-8192, 12288, 4096], [0, -16384, 8192]][index % 3],
        uv: [[0, 0], [4095, 0], [0, 4095]][index % 3],
        rgb565: [0xf800, 0x07e0, 0x001f][index % 3],
    }
}
pub fn input(points: [[f64; 2]; 3], w: [f64; 3], c: Config) -> Input {
    Input {
        id: 17,
        vertices: std::array::from_fn(|i| vertex(points[i], w[i], i, c)),
    }
}
pub fn pressure() -> Vec<(&'static str, Input)> {
    let c = config();
    let mut near = input(
        [[32.0, 4.0], [4.0, 34.0], [60.0, 34.0]],
        [0.10, 1.0, 1.0],
        c,
    );
    near.vertices[0].clip[0] = 0;
    near.vertices[0].clip[1] = 5243;
    vec![
        (
            "far-wall",
            input(
                [[-64.0, -40.0], [192.0, -40.0], [-64.0, 120.0]],
                [200.0; 3],
                c,
            ),
        ),
        (
            "wall-slide",
            input(
                [[-600.0, -180.0], [300.0, -60.0], [10.0, 140.0]],
                [0.5, 20.0, 2.0],
                c,
            ),
        ),
        (
            "grazing-floor",
            input(
                [[-12.0, 43.0], [76.0, 43.0], [32.0, 2.0]],
                [1.0, 1.0, 200.0],
                c,
            ),
        ),
        (
            "triangle-soup",
            input([[4.5, 4.5], [58.5, 4.5], [58.5, 34.5]], [64.0; 3], c),
        ),
        ("near-crossing", near),
    ]
}
