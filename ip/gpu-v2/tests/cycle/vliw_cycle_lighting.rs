//! Actual II4 tuple calendar, independent stage integers and whole production RTL.
#[allow(dead_code)]
#[path = "../support/vliw_pixel_source.rs"]
mod actual;
#[path = "../support/vliw_lighting.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "../support/vliw_cycle/mod.rs"]
mod transport_support;
#[allow(dead_code)]
#[path = "../support/vliw_vertex/mod.rs"]
mod vertex_support;
use gpu_v2::{
    lighting::{
        dsp_sharing,
        ports::{Light, Material, PixelInput, Projection},
        sim::{counted, oracle},
    },
    system::vliw_cycle::{lighting::*, vertex::Bits},
};
use std::collections::VecDeque;

#[derive(Clone, Copy)]
struct Edge {
    i: Input,
    o: Output,
}
fn globals() -> Globals {
    Globals {
        epoch: 0x8192,
        mode: 3,
        code: 12,
        light: [-9830, 0, 13107],
        ambient: 49,
        directional: 173,
        ray: [-8192, -7816, 8192],
    }
}
fn config() -> oracle::Config {
    let (o, _) = dsp_sharing::four_cycle_shared_roms(9).unwrap();
    oracle::Config::from_counted(counted::Config {
        smooth_h9: o.smooth_h9,
        h9_tail_dsp: o.h9_tail_dsp,
        rsqrt_q13: o.rsqrt_q13,
        lit_queue: o.lit_queue,
        quantization: o.quantization,
        scalar_norm: o.scalar_norm,
        weighted_view: o.weighted_view,
        scalar_normal: o.scalar_normal,
        exact_normal_gate: o.exact_normal_gate,
        block_prescale: o.block_prescale,
        direct_square: o.direct_square,
        direct_all_squares: o.direct_all_squares,
        square9: o.square9,
        dedicated_dots: o.dedicated_dots,
        ..counted::Config::architecture()
    })
}
fn h9(cosine: i128, code: usize, positive: bool) -> u16 {
    let exponent: i32 = [
        4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 32, 40, 48, 56, 64,
    ][code];
    let mut shift = 0;
    while (1 << shift) * exponent < 6144 {
        shift += 1;
    }
    let distance = 32768 - cosine;
    let bin = distance >> shift;
    if !positive || bin >= 32 {
        return 0;
    }
    let width = 1_i128 << shift;
    let endpoint = |i: i128| {
        (256.0 * (1.0 - (i * width) as f64 / 32768.0).max(0.0).powi(exponent)).round_ties_even()
            as u16
    };
    let base = endpoint(bin);
    let delta = base - endpoint(bin + 1);
    let fraction = ((distance << 6) >> shift) as u16 & 63;
    base - ((delta * fraction + 32) >> 6)
}
fn golden(p: PixelInput, g: Globals, c: oracle::Config) -> [u16; 2] {
    let o = oracle::evaluate(
        p,
        Material {
            unlit: false,
            specular_color: [255; 3],
            shininess_code: g.code,
        },
        Light {
            direction: g.light,
            ambient: g.ambient,
            directional: g.directional,
        },
        Projection {
            ray_scale: [g.ray[0], g.ray[1]],
            k: g.ray[2],
        },
        c,
    )
    .unwrap();
    let cosine = o.stages.iter().find(|(n, _)| n == "x").unwrap().1;
    let positive = p
        .normal
        .iter()
        .zip(g.light)
        .map(|(&n, l)| i64::from(n) * i64::from(l))
        .sum::<i64>()
        > 0;
    let product = u32::from(g.directional) * u32::from(h9(cosine, g.code as usize, positive));
    let q = product >> 8;
    let r = product & 255;
    [
        o.g as u16,
        (q + u32::from(r > 128 || r == 128 && q & 1 != 0)) as u16,
    ]
}
fn pixel(n: usize) -> PixelInput {
    let normals = [
        [0, 0, 1024],
        [0, 0, -1024],
        [0, 0, 0],
        [-513, 97, 889],
        [371, -711, 922],
        [2047, -2048, 1],
        [-1024, 0, 1024],
    ];
    PixelInput {
        normal: normals[n % normals.len()].map(|x| x * 16),
        ndc: [
            ((n * 719) % 32001) as i32 - 16000,
            ((n * 1237) % 32001) as i32 - 16000,
        ],
    }
}
fn source(i: &mut Input, n: usize) {
    let p = pixel(n);
    i.source_slot = (n / 4 % 32) as u8;
    i.source_lane = n as u8 & 3;
    i.source_mask = [15, 9, 6, 2, 8][n / 4 % 5];
    i.normal = Bits::new(
        p.normal.iter().enumerate().fold(0, |v, (c, &x)| {
            v | (u64::from((x >> 4) as u16 & 4095) << (12 * c))
        }),
        36,
    );
    i.position = Bits::new(
        u64::from(p.ndc[0] as u16) | (u64::from(p.ndc[1] as u16) << 16),
        32,
    );
}
fn run() -> Vec<Edge> {
    let mut dut = Lighting::new().unwrap();
    let config = config();
    let mut edges = Vec::new();
    let mut expected = VecDeque::new();
    let mut enabled = 0;
    let mut accepted = 0;
    let mut results = 0;
    let mut nonzero_h = 0;
    let mut ce0 = 0;
    let mut i = Input {
        reset: true,
        globals: globals(),
        ..Default::default()
    };
    let mut n = 0;
    let mut prepare: usize = 4;
    for wall in 0..2500 {
        i.reset = wall == 0 || wall == 630;
        i.ce = wall % 13 != 5;
        i.compute_tick = i.ce && wall % 7 != 2;
        i.out_ready = wall % 3 != 1; // Live core advance is compute_tick only.
        i.globals.epoch = (0x8192 + wall * 19) as u16;
        // The ambient wire changes after the first admission, before its
        // scheduled G operation. A context snapshot at admission is wrong.
        i.globals.ambient = if accepted == 0 { 49 } else { 91 };
        i.globals.mode = if wall % 79 < 9 {
            1
        } else if wall % 61 < 12 {
            2
        } else {
            3
        };
        source(&mut i, n);
        i.source_valid = prepare == 0 && n < 192;
        let o = dut.outputs(i);
        if !i.reset && o.result.valid {
            let (at, key, last, gold): (usize, u8, bool, [u16; 2]) = *expected.front().unwrap();
            assert_eq!(enabled - at, 35, "latency at wall{wall}");
            assert_eq!(o.result.key, key);
            assert_eq!(o.result.last, last);
            let gold = match i.globals.mode {
                1 => [i.globals.ambient, 0],
                2 => [gold[0], 0],
                _ => gold,
            };
            assert!(o.result.g.is_known(9) && o.result.h.is_known(9));
            assert_eq!(
                [o.result.g.value as u16, o.result.h.value as u16],
                gold,
                "wall{wall} key{key}"
            );
            if i.compute_tick {
                nonzero_h += usize::from(gold[1] > 0);
                results += 1;
                expected.pop_front();
            }
        }
        if o.pixel_accepted {
            let p = pixel(n);
            expected.push_back((
                enabled,
                (n % 128) as u8,
                i.source_mask >> (i.source_lane + 1) == 0,
                golden(
                    p,
                    Globals {
                        ambient: 91,
                        ..i.globals
                    },
                    config,
                ),
            ));
            accepted += 1;
        }
        edges.push(Edge { i, o });
        dut.clock(i).unwrap_or_else(|e| panic!("wall{wall}: {e}"));
        if i.reset {
            expected.clear();
            enabled = 0;
            prepare = 4;
        } else if i.compute_tick {
            enabled += 1;
            prepare = prepare.saturating_sub(1);
        } else {
            ce0 += 1;
        }
        if o.source_ready {
            n += 1;
            prepare = 2;
        }
        if n == 192 && dut.idle() {
            break;
        }
    }
    assert!(n == 192 && dut.idle());
    assert!(accepted > 90 && results > 80 && nonzero_h > 3 && ce0 > 50);
    println!("II4_LIGHTING edges={} accepted={accepted} results={results} nonzero_h={nonzero_h} ce0={ce0}", edges.len());
    edges
}
#[test]
fn scheduled_live_lighting_matches_independent_stage_integers() {
    run();
}
#[test]
#[ignore = "two bounded actual II4 tuple RTL profiles, with vendor hard DSP model"]
fn selected_lighting_tuple_matches_each_enabled_edge() {
    let rows = run();
    let root = root("lighting");
    for native in [false, true] {
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
    println!("LIGHTING_ROOT {}", root.display());
}
fn root(label: &str) -> std::path::PathBuf {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-draw")
        .join(format!(
            "{label}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[test]
#[ignore = "three bounded compiled actual Lighting wiring/calendar mutations"]
fn live_context_calendar_and_h9_mutations_are_detected() {
    let rows = run();
    let root = root("lighting-negative");
    let p = fixture::fields(&rows[0]);
    let source = fixture::production(&p, false);
    for (name, old, new) in [
        (
            "live_epoch",
            "assign out_epoch = ctx_epoch;",
            "assign out_epoch = 16'd0;",
        ),
        (
            "ready_is_not_ce",
            "wire advance = ce;",
            "wire advance = ce && out_ready;",
        ),
        (
            "h9_round_bit",
            "out_h=v814_d1;",
            "out_h={v814_d1[8:1],1'b0};",
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
    println!("LIGHTING_NEGATIVE_ROOT {}", root.display());
}
