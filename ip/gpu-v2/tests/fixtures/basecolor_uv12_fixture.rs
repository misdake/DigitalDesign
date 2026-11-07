//! The original seven scenes stay byte-reconstructible; two moving texture scenes extend them.
#[path = "basecolor_fixture.rs"]
mod base;
pub use base::lod;
use gpu_v2::{
    material_color::{Albedo, ColorSource as Mode},
    triangle::{ports::Input, sim::oracle},
};
pub const SCENES: usize = base::SCENES + 2;
pub fn fixture(scene: usize, phase: f64) -> (oracle::Report, [Albedo; 3], Mode) {
    if scene < 7 {
        return base::fixture(scene, phase);
    }
    let (old, _, _) = base::fixture(0, phase);
    let shift = (phase * std::f64::consts::TAU).sin();
    let (screen, w) = if scene == 7 {
        (
            [
                [115.0 + 25.0 * shift, 35.0],
                [280.0, 60.0 + 12.0 * shift],
                [135.0, 215.0],
            ],
            [0.75, 1.4, 0.9],
        )
    } else {
        (
            [[30.0 + 10.0 * shift, 12.0], [375.0, 20.0], [45.0, 230.0]],
            [1.0, 1.0 + 0.18 * shift, 1.0],
        )
    };
    let uv = [[0, 0], [4095, 0], [0, 4095]];
    let mut input: Input = old.input.clone();
    input.id = scene as u32;
    for i in 0..3 {
        input.vertices[i].clip = [
            ((screen[i][0] / 400.0 * 2.0 - 1.0) * w[i] * 65536.0).round_ties_even() as i32,
            ((1.0 - screen[i][1] / 240.0 * 2.0) * w[i] * 65536.0).round_ties_even() as i32,
            0,
            (w[i] * 65536.0).round_ties_even() as i32,
        ];
        input.vertices[i].uv = uv[i];
    }
    let r = oracle::run(&input, old.config).unwrap();
    (r, uv.map(Albedo::Texture), Mode::Texture)
}
