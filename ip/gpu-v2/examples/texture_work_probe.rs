//! Structural opportunity study over bounded perspective helper quads.
//! These are synthetic geometry cases, not hardware work counters or timings.
#[path = "support/texture_work.rs"]
mod texture_work;
use gpu_v2::texture::{ports::*, sim::oracle::*};
use std::path::PathBuf;
use texture_work::{check_strength_reductions, WorkStudy};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    check_strength_reductions();
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/gpu-v2-texture-work".into()),
    );
    std::fs::create_dir_all(&root)?;
    let slot = Slot {
        base_address: 0x1000,
        has_full_mip: true,
        max_size_log2: 9,
        valid: true,
    };
    let config = Config {
        uv_fraction: Some(18),
        coefficient_fraction: 9,
        coefficient_encoding: CoefficientEncoding::Unorm,
        mip_selection: MipSelection::Floor,
        max_quads: 32768,
        ..Config::default()
    };
    let mut work = WorkStudy::default();
    for (name, perspective) in [("perspective_mild", 0.02), ("perspective_strong", 0.3)] {
        let mut state = 0x319f_6514_u64;
        for case in 0..16384 {
            let mut next = || {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                (state >> 32) as f64 / f64::from(u32::MAX)
            };
            let center = [next() * 1.2 - 0.1, next() * 1.2 - 0.1];
            let slope = 2.0_f64.powf(-1.0 + next() * 9.75) / 512.0;
            let w = 1.0 + next() * 2.0;
            let dw = [
                w * perspective * (next() - 0.5),
                w * perspective * (next() - 0.5),
            ];
            let uv = std::array::from_fn(|lane| {
                let x = (lane % 2) as f64;
                let y = (lane / 2) as f64;
                let divisor = w + x * dw[0] + y * dw[1];
                [
                    (center[0] * w + w * slope * (0.93 * x - 0.31 * y)) / divisor,
                    (center[1] * w + w * slope * (0.19 * x + 0.77 * y)) / divisor,
                ]
            });
            let mask = [15, 15, 15, 5, 3, 1][case % 6];
            let q = QuadInput {
                quad_id: 0,
                mask,
                uv,
                slot: 0,
                material_size_log2: 9,
                filter: Filter::Trilinear,
                lod_bias: 0.0,
            };
            work.observe(name, &prepare(&q, &[slot], config)?);
        }
    }
    work.write(&root.join("geometry.csv"))?;
    println!("32768 bounded perspective quads; exact /511 normalization, lambda, parent and signed mip-coordinate strength reductions verified");
    Ok(())
}
