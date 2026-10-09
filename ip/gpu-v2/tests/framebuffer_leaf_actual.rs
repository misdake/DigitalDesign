//! Focused numerical qualification of the registered ROP arithmetic leaf.
//!
//! Every expected value here comes from an independent golden that uses true
//! integer `/255` division and explicit bit-replication expand. It never calls
//! `div255`, never consults the audited model and never reads a captured answer.
//! The emulator under test is the real eight-stage register pipeline, not a
//! latency queue.

use gpu_v2::framebuffer::arithmetic::{
    div255, expand, pixel, quantize, LeafInput, LeafTick, Pipeline, Pixel, ResultPixel,
    DIV255_SAFE_MAX, LEAF_LATENCY,
};
use gpu_v2::framebuffer::ports::{Blend, Context, DepthFunc, Fragment};

/// Independent expand: explicit bit replication, no shared helper.
fn golden_expand(code: u16) -> [u8; 3] {
    let r = ((code >> 11) & 0x1f) as u8;
    let g = ((code >> 5) & 0x3f) as u8;
    let b = (code & 0x1f) as u8;
    [
        (r << 3) | (r >> 2),
        (g << 2) | (g >> 4),
        (b << 3) | (b >> 2),
    ]
}

/// Independent SRC_OVER channel using true integer `/255`.
fn golden_over(a: u8, s: u8, d: u8) -> u8 {
    let a = u32::from(a);
    let s = u32::from(s);
    let d = u32::from(d);
    ((a * s + (255 - a) * d + 127) / 255) as u8
}

/// Independent quantize channel using true integer `/255`.
fn golden_quantize_channel(value: u8, steps: u32) -> u16 {
    ((u32::from(value) * steps + 127) / 255) as u16
}

fn golden_quantize(rgb: [u8; 3]) -> u16 {
    (golden_quantize_channel(rgb[0], 31) << 11)
        | (golden_quantize_channel(rgb[1], 63) << 5)
        | golden_quantize_channel(rgb[2], 31)
}

fn golden_depth_pass(func: DepthFunc, source: u16, old: u16) -> bool {
    match func {
        DepthFunc::Never => false,
        DepthFunc::Less => source < old,
        DepthFunc::Equal => source == old,
        DepthFunc::LessEqual => source <= old,
        DepthFunc::Greater => source > old,
        DepthFunc::NotEqual => source != old,
        DepthFunc::GreaterEqual => source >= old,
        DepthFunc::Always => true,
    }
}

/// Fully independent semantic golden for one covered fragment.
fn golden(old: Pixel, source: Fragment, covered: bool, context: Context) -> ResultPixel {
    let pass = covered && golden_depth_pass(context.depth, source.depth, old.depth);
    if !pass {
        return ResultPixel {
            pixel: old,
            color_written: false,
            depth_written: false,
        };
    }
    let dest = golden_expand(old.color);
    let rgb = std::array::from_fn(|i| match context.blend {
        Blend::Replace => source.rgba[i],
        Blend::SrcOver => golden_over(source.rgba[3], source.rgba[i], dest[i]),
    });
    ResultPixel {
        pixel: Pixel {
            color: golden_quantize(rgb),
            depth: if context.depth_write {
                source.depth
            } else {
                old.depth
            },
        },
        color_written: true,
        depth_written: context.depth_write,
    }
}

/// Push one lane through the real register stages and return its result.
fn emulate(input: LeafInput) -> ResultPixel {
    let mut pipe = Pipeline::new(64).unwrap();
    let mut offered = Some(input);
    for _ in 0..=u32::from(LEAF_LATENCY) {
        let step = pipe
            .tick(LeafTick {
                ce: true,
                input: offered.take(),
            })
            .unwrap();
        if let Some(output) = step.output {
            return output.result;
        }
    }
    panic!("leaf did not publish a result");
}

fn deterministic(seed: &mut u64) -> u32 {
    *seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    (*seed >> 32) as u32
}

fn random_input(seed: &mut u64) -> LeafInput {
    let funcs = [
        DepthFunc::Never,
        DepthFunc::Less,
        DepthFunc::Equal,
        DepthFunc::LessEqual,
        DepthFunc::Greater,
        DepthFunc::NotEqual,
        DepthFunc::GreaterEqual,
        DepthFunc::Always,
    ];
    LeafInput {
        key: (deterministic(seed) % 4) as u8,
        blend: if deterministic(seed) & 1 == 0 {
            Blend::Replace
        } else {
            Blend::SrcOver
        },
        depth: funcs[(deterministic(seed) % 8) as usize],
        depth_write: deterministic(seed) & 1 == 0,
        covered: deterministic(seed) & 1 == 0,
        old: Pixel {
            color: deterministic(seed) as u16,
            depth: deterministic(seed) as u16,
        },
        source: Fragment {
            rgba: [
                deterministic(seed) as u8,
                deterministic(seed) as u8,
                deterministic(seed) as u8,
                deterministic(seed) as u8,
            ],
            depth: deterministic(seed) as u16,
        },
    }
}

#[test]
fn div255_is_exact_for_every_legal_numerator() {
    for n in 0..=DIV255_SAFE_MAX {
        assert_eq!(div255(n), n / 255, "n={n}");
    }
}

#[test]
fn every_rgb565_code_round_trips_through_expand_and_quantize() {
    for code in 0..=u16::MAX {
        assert_eq!(quantize(expand(code)), code, "code={code}");
        assert_eq!(
            golden_quantize(golden_expand(code)),
            code,
            "golden code={code}"
        );
    }
}

#[test]
fn all_depth_functions_masks_depth_write_and_blends_match_golden() {
    let funcs = [
        DepthFunc::Never,
        DepthFunc::Less,
        DepthFunc::Equal,
        DepthFunc::LessEqual,
        DepthFunc::Greater,
        DepthFunc::NotEqual,
        DepthFunc::GreaterEqual,
        DepthFunc::Always,
    ];
    // Distinct backgrounds, plus the leaf's coverage input standing in for the
    // post-mask lane bit.
    let backgrounds = [
        Pixel {
            color: 0x0000,
            depth: 0,
        },
        Pixel {
            color: 0x1234,
            depth: 4321,
        },
        Pixel {
            color: 0xffff,
            depth: 65535,
        },
        Pixel {
            color: 0x8410,
            depth: 1000,
        },
        Pixel {
            color: 0x07e0,
            depth: 32768,
        },
        Pixel {
            color: 0xf81f,
            depth: 12345,
        },
    ];
    for (func_index, func) in funcs.into_iter().enumerate() {
        for blend in [Blend::Replace, Blend::SrcOver] {
            for depth_write in [false, true] {
                for mask in [1u8, 2, 4, 8, 15] {
                    let covered = mask & 1 != 0;
                    for alpha in [0u8, 255] {
                        for old in backgrounds {
                            let context = Context {
                                depth: func,
                                depth_write,
                                blend,
                            };
                            // Probe the comparison boundary with the old depth.
                            for delta in [-1i32, 0, 1] {
                                let depth = (i32::from(old.depth) + delta).clamp(0, 65535) as u16;
                                let source = Fragment {
                                    rgba: [200, 3, 77, alpha],
                                    depth,
                                };
                                let input = LeafInput {
                                    key: (func_index % 4) as u8,
                                    blend,
                                    depth: func,
                                    depth_write,
                                    covered,
                                    old,
                                    source,
                                };
                                let expected = golden(old, source, covered, context);
                                assert_eq!(
                                    emulate(input),
                                    expected,
                                    "func {func_index} blend {blend:?} dw {depth_write} \
                                     mask {mask} alpha {alpha} old {old:?} depth {depth}"
                                );
                                assert_eq!(
                                    pixel(old, source, covered, context),
                                    expected,
                                    "shared pixel path diverged"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn twenty_thousand_randoms_match_the_independent_division_golden() {
    let mut seed = 0x0f1e_2d3c_4b5a_6978u64;
    for index in 0..20_000usize {
        let input = random_input(&mut seed);
        let expected = golden(input.old, input.source, input.covered, input.context());
        assert_eq!(emulate(input), expected, "index {index} input {input:?}");
    }
}

#[test]
fn emulator_is_a_register_pipeline_not_a_delay_queue() {
    let key = 2u8;
    let mut pipe = Pipeline::new(64).unwrap();
    for stage in 1..=u32::from(LEAF_LATENCY) {
        let input = (stage == 1).then(|| {
            let mut seed = 7;
            random_input(&mut seed).with_key(key)
        });
        let step = pipe.tick(LeafTick { ce: true, input }).unwrap();
        let expected: [Option<u8>; LEAF_LATENCY as usize] =
            std::array::from_fn(|i| (i == stage as usize - 1).then_some(key));
        assert_eq!(pipe.stage_keys(), expected, "stage {stage}");
        assert!(step.output.is_none(), "published before stage 8");
    }
    let step = pipe
        .tick(LeafTick {
            ce: true,
            input: None,
        })
        .unwrap();
    assert!(step.returned && step.output.unwrap().key == key);
    assert!(pipe.idle());
}

#[test]
fn ce_pauses_freeze_stages_and_latency_is_measured_in_enabled_edges() {
    let mut pipe = Pipeline::new(128).unwrap();
    let input = random_input(&mut 11);
    let step = pipe
        .tick(LeafTick {
            ce: true,
            input: Some(input),
        })
        .unwrap();
    assert!(step.accepted);
    let frozen = pipe.stage_keys();
    for _ in 0..3 {
        let held = pipe
            .tick(LeafTick {
                ce: false,
                input: None,
            })
            .unwrap();
        assert!(!held.accepted && held.output.is_none() && !held.returned);
        assert_eq!(pipe.stage_keys(), frozen);
    }
    let mut enabled = 1u64;
    let output = loop {
        let step = pipe
            .tick(LeafTick {
                ce: true,
                input: None,
            })
            .unwrap();
        enabled += 1;
        if let Some(output) = step.output {
            break output;
        }
        assert!(enabled < 32, "pipeline watchdog");
    };
    assert_eq!(enabled, u64::from(LEAF_LATENCY) + 1);
    assert_eq!(
        output.result,
        golden(input.old, input.source, input.covered, input.context())
    );
}

trait WithKey {
    fn with_key(self, key: u8) -> Self;
}
impl WithKey for LeafInput {
    fn with_key(mut self, key: u8) -> Self {
        self.key = key;
        self
    }
}
