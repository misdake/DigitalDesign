//! Deterministic bounded inputs and original-RNE/ideal references for review.
#[allow(dead_code)]
#[path = "../tests/support/mod.rs"]
mod support;
use gpu_v2::lighting::{ports::*, sim::oracle};
use std::{
    fs::File,
    io::{BufWriter, Write},
};
fn main() {
    let mut cases = Vec::new();
    let mut random = support::Random(0x5be07df56af924d8);
    for _ in 0..20_000 {
        let u = random.direction();
        let scale = 0.5 + f64::from(random.next() % 15001) / 10000.0;
        let normal = u.map(|v| {
            (f64::from(v) * scale)
                .round_ties_even()
                .clamp(-32768.0, 32767.0) as i16
        });
        cases.push((
            0,
            PixelInput {
                normal,
                ndc: std::array::from_fn(|_| (random.next() % 131073) as i32 - 65536),
            },
            Material {
                shininess_code: (random.next() % 17) as u8,
                ..Default::default()
            },
            Light {
                direction: random.direction(),
                ambient: 32,
                directional: 192,
            },
            Projection::default(),
        ));
    }
    let unit = |v: [f64; 3]| {
        let q = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        v.map(|x| (x / q * 16384.0).round_ties_even().clamp(-32768.0, 32767.0) as i16)
    };
    for i in 0..12_000 {
        let l = random.direction();
        let lf = l.map(|v| f64::from(v) / 16384.0);
        let (group, normal, direction, code) = match i % 3 {
            0 => {
                let jitter = f64::from(random.next() % 2001) / 100000.0;
                (
                    1,
                    unit([lf[0] + jitter, lf[1] - jitter, lf[2] + 1.0]),
                    l,
                    (random.next() % 9 + 8) as u8,
                )
            }
            1 => {
                let angle = f64::from(random.next() % 160 + 16) / 16384.0;
                let dir = unit([angle, 0.0, -1.0]);
                let v = dir.map(|x| f64::from(x) / 16384.0);
                (
                    2,
                    unit([v[0], v[1], v[2] + 1.0]),
                    dir,
                    (random.next() % 17) as u8,
                )
            }
            _ => {
                let offset = (i32::try_from(random.next() % 9).unwrap() - 4) as f64 / 16384.0;
                (
                    3,
                    unit([-lf[2] + lf[0] * offset, 0.0, lf[0] + lf[2] * offset]),
                    l,
                    (random.next() % 17) as u8,
                )
            }
        };
        cases.push((
            group,
            PixelInput {
                normal,
                ndc: [0; 2],
            },
            Material {
                shininess_code: code,
                ..Default::default()
            },
            Light {
                direction,
                ambient: 32,
                directional: 192,
            },
            Projection::default(),
        ));
    }
    cases.extend(
        support::representative()
            .into_iter()
            .map(|(p, m, l, pr)| (4, p, m, l, pr)),
    );
    let mut out = BufWriter::new(File::create(std::env::args().nth(1).unwrap()).unwrap());
    for (group, p, m, l, pr) in cases {
        let cfg = oracle::Config {
            scalar_norm: true,
            rounding: oracle::RoundingPolicy {
                power: oracle::Rounding::Floor,
                ..Default::default()
            },
            ..Default::default()
        };
        let g = oracle::evaluate(p, m, l, pr, cfg).unwrap();
        let ideal = oracle::ideal(p, m, l, pr).unwrap();
        writeln!(
            out,
            "{group},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{:.12},{:.12}",
            p.normal[0],
            p.normal[1],
            p.normal[2],
            p.ndc[0],
            p.ndc[1],
            l.direction[0],
            l.direction[1],
            l.direction[2],
            l.ambient,
            l.directional,
            m.shininess_code,
            u8::from(m.unlit),
            u8::from(m.specular_color != [0; 3]),
            pr.k,
            pr.ray_scale[0],
            pr.ray_scale[1],
            g.g,
            g.h,
            ideal[0] * 256.0,
            ideal[1] * 256.0
        )
        .unwrap();
    }
}
