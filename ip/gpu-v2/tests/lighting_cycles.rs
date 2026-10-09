mod support;
use gpu_v2::lighting::{
    emu::LightingEmu,
    ports::*,
    sim::{counted, oracle},
    LightingProfile,
};

fn idle() -> LightingTick {
    LightingTick {
        reset: false,
        ce: true,
        context: None,
        input: None,
        output_ready: true,
    }
}
fn retiming(profile: LightingProfile) -> gpu_v2::lighting::LightingRetiming {
    let delta = |name| {
        std::env::var(name)
            .ok()
            .map(|v| v.parse().unwrap())
            .unwrap_or(0)
    };
    if std::env::var_os("LIGHTING_RETIMED_RESOURCE").is_some() {
        let mut r = gpu_v2::lighting::LightingRetiming::resource_candidate(profile);
        r.extra_large_multiply += delta("LIGHTING_EXTRA_LARGE");
        r.extra_small_multiply += delta("LIGHTING_EXTRA_SMALL");
        if profile == LightingProfile::Compact
            && std::env::var_os("LIGHTING_REMOVE_SPARE_SMALL9").is_some()
        {
            r.extra_small_multiply = 0;
        }
        return r;
    }
    gpu_v2::lighting::LightingRetiming {
        measured_functions: std::env::var_os("LIGHTING_MEASURED_FUNCTIONS").is_some(),
        extra_large_multiply: delta("LIGHTING_EXTRA_LARGE"),
        extra_small_multiply: delta("LIGHTING_EXTRA_SMALL"),
        extra_normalize_reads: delta("LIGHTING_EXTRA_READS"),
        compact_lifetimes: std::env::var_os("LIGHTING_COMPACT_LIFETIMES").is_some(),
    }
}
fn context(m: Material, l: Light, pr: Projection) -> LightingContext {
    LightingContext {
        material: m,
        light: l,
        projection: pr,
        epoch: 17,
    }
}

#[test]
fn numerical_executor_matches_every_stage_without_rebuilding_the_graph() {
    for profile in [
        LightingProfile::Fast,
        LightingProfile::Compact,
        LightingProfile::SystemFast,
        LightingProfile::SystemCompact,
    ] {
        let system = matches!(
            profile,
            LightingProfile::SystemFast | LightingProfile::SystemCompact
        );
        for &resource in if system {
            &[true][..]
        } else {
            &[false, true][..]
        } {
            for depth in [0, 8] {
                let kernel = if system {
                    counted::Config::system_profile()
                } else if resource {
                    counted::Config::resource_profile(profile)
                } else {
                    counted::Config::architecture()
                };
                let mut emu = if depth != 0 {
                    LightingEmu::with_kernel_depth(
                        profile,
                        false,
                        kernel,
                        system || resource,
                        depth,
                        300_000,
                    )
                } else if system {
                    LightingEmu::with_system_profile(profile, 300_000)
                } else if resource {
                    LightingEmu::with_resource_profile(profile, 300_000)
                } else {
                    LightingEmu::with_profile(profile, 300_000)
                }
                .unwrap();
                for (id, (p, m, l, pr)) in support::representative().into_iter().enumerate() {
                    let ctx = context(m, l, pr);
                    assert!(
                        emu.tick(LightingTick {
                            context: Some(ctx),
                            ..idle()
                        })
                        .unwrap()
                        .context_ready
                    );
                    let req = LightingRequest {
                        pixel: p,
                        id: id as u32,
                    };
                    assert!(
                        emu.tick(LightingTick {
                            input: Some(req),
                            ..idle()
                        })
                        .unwrap()
                        .input_ready
                    );
                    for _ in 0..emu.latency() {
                        emu.tick(idle()).unwrap();
                    }
                    let result = emu.signals(idle()).output.unwrap();
                    let expected = counted::evaluate_with_config(
                        p,
                        m,
                        l,
                        pr,
                        support::MAX_EVENTS,
                        if system {
                            counted::Config::system_profile()
                        } else if resource {
                            counted::Config::resource_profile(profile)
                        } else {
                            counted::Config::architecture()
                        },
                    )
                    .unwrap();
                    assert_eq!(result.output, expected.output, "{p:?} {m:?} {l:?}");
                    let stages = emu.output_stages().unwrap();
                    for o in expected.frame.outputs {
                        assert_eq!(
                            stages.iter().find(|(n, _)| *n == o.name).unwrap().1,
                            o.raw,
                            "stage {} {p:?}",
                            o.name
                        );
                    }
                    let golden = oracle::evaluate(
                        p,
                        m,
                        l,
                        pr,
                        oracle::Config {
                            scalar_norm: resource,
                            scalar_normal: system,
                            exact_normal_gate: system,
                            direct_all_squares: system,
                            rounding: oracle::RoundingPolicy {
                                power: oracle::Rounding::Floor,
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    )
                    .unwrap();
                    assert_eq!(
                        (i128::from(result.output.g), i128::from(result.output.h)),
                        (golden.g, golden.h)
                    );
                    assert_eq!(result.id, id as u32);
                    assert_eq!(result.epoch, 17);
                    emu.tick(idle()).unwrap();
                }
            }
        }
    }
}

#[test]
fn factor_profile_retains_public_rates_and_matches_independent_goldens() {
    for profile in [LightingProfile::Fast, LightingProfile::Compact] {
        let rtl = gpu_v2::lighting::rtl::generate_with_options(
            profile,
            gpu_v2::lighting::rtl::LightingRtlOptions::factor_profile(),
        )
        .unwrap();
        let mut emu = LightingEmu::with_factor_profile(profile, 80_000).unwrap();
        assert_eq!(
            (rtl.specular_ii, rtl.diffuse_ii),
            if profile == LightingProfile::Fast {
                (2, 1)
            } else {
                (3, 2)
            }
        );
        for (id, (pixel, material, light, projection)) in
            support::representative().into_iter().step_by(7).enumerate()
        {
            let ctx = context(material, light, projection);
            assert!(
                emu.tick(LightingTick {
                    context: Some(ctx),
                    ..idle()
                })
                .unwrap()
                .context_ready
            );
            assert_eq!(
                emu.latency(),
                if ctx.mode() == 3 {
                    rtl.latency
                } else {
                    rtl.diffuse_latency
                }
            );
            assert_eq!(
                emu.initiation_interval(),
                if ctx.mode() == 3 {
                    rtl.specular_ii
                } else {
                    rtl.diffuse_ii
                }
            );
            assert!(
                emu.tick(LightingTick {
                    input: Some(LightingRequest {
                        pixel,
                        id: id as u32
                    }),
                    ..idle()
                })
                .unwrap()
                .input_ready
            );
            for _ in 0..emu.latency() {
                emu.tick(idle()).unwrap();
            }
            let result = emu.signals(idle()).output.unwrap();
            let counted = counted::evaluate_with_config(
                pixel,
                material,
                light,
                projection,
                support::MAX_EVENTS,
                counted::Config::system_candidate(true),
            )
            .unwrap();
            assert_eq!(result.output, counted.output);
            let golden = oracle::evaluate(
                pixel,
                material,
                light,
                projection,
                oracle::Config {
                    scalar_norm: true,
                    scalar_normal: true,
                    direct_all_squares: true,
                    rounding: oracle::RoundingPolicy {
                        power: oracle::Rounding::Floor,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                (i128::from(result.output.g), i128::from(result.output.h)),
                (golden.g, golden.h)
            );
            assert_eq!((result.id, result.epoch), (id as u32, 17));
            emu.tick(idle()).unwrap();
        }
    }
}

fn exercise(
    profile: LightingProfile,
    dedicated: bool,
    mut record: impl FnMut(LightingTick, LightingSignals, Vec<(u32, bool, String, i128)>),
) {
    let scalar = std::env::var_os("LIGHTING_SCALAR_NORM").is_some();
    let block = std::env::var_os("LIGHTING_BLOCK_PRESCALE").is_some();
    let roles = std::env::var_os("LIGHTING_ROLE_SCHEDULE").is_some();
    let direct = std::env::var_os("LIGHTING_DIRECT_SQUARE").is_some();
    let resource = std::env::var_os("LIGHTING_RESOURCE_PROFILE").is_some();
    let system = matches!(
        profile,
        LightingProfile::SystemFast | LightingProfile::SystemCompact
    );
    let factor = std::env::var_os("LIGHTING_FACTOR_KERNEL").is_some();
    let mut kernel = if std::env::var_os("LIGHTING_COMPENSATED_FLOOR").is_some() {
        counted::Config::compensated_resource_profile(profile)
    } else if system || factor {
        counted::Config {
            exact_normal_gate: std::env::var_os("LIGHTING_SCALED_GATE").is_none(),
            ..counted::Config::system_profile()
        }
    } else if resource {
        counted::Config::resource_profile(profile)
    } else {
        counted::Config {
            scalar_norm: scalar,
            block_prescale: block,
            direct_square: direct,
            ..counted::Config::architecture()
        }
    };
    kernel.lit_queue = std::env::var_os("LIGHTING_LIT_QUEUE").is_some();
    let depth = std::env::var("LIGHTING_LOGIC_DEPTH")
        .ok()
        .map(|v| v.parse().unwrap())
        .unwrap_or(0);
    let mut emu = if retiming(profile) != Default::default() {
        LightingEmu::with_retiming(
            profile,
            kernel,
            roles || system || resource || factor,
            depth,
            retiming(profile),
            dedicated,
            40_000,
        )
    } else if depth != 0 {
        LightingEmu::with_kernel_depth(
            profile,
            dedicated,
            kernel,
            roles || system || resource || factor,
            depth,
            40_000,
        )
    } else if system {
        LightingEmu::with_kernel(profile, dedicated, kernel, true, 40_000)
    } else if resource {
        // Preserve the selected quantization and lit-queue switches on both
        // sides of the differential comparison; the convenience constructor
        // would silently restore the default resource kernel.
        LightingEmu::with_kernel(profile, dedicated, kernel, true, 40_000)
    } else {
        LightingEmu::with_kernel(profile, dedicated, kernel, roles, 40_000)
    }
    .unwrap();
    let mut advance = |emu: &mut LightingEmu, tick: LightingTick| {
        let stages = emu.stage_values();
        let signals = emu.tick(tick).unwrap();
        record(tick, signals, stages);
        signals
    };
    advance(
        &mut emu,
        LightingTick {
            reset: true,
            ce: false,
            ..idle()
        },
    );
    let mut random = support::Random(0x37ba829183);
    let mut serial = 0_u32;
    for batch in 0..27 {
        // Unlit is a quad-owner bypass, not a request in the queue contract.
        if kernel.lit_queue && batch == 17 {
            continue;
        }
        let m = Material {
            shininess_code: (batch % 17) as u8,
            unlit: batch == 17,
            specular_color: if batch == 19 { [0; 3] } else { [255; 3] },
        };
        let light = Light {
            direction: match batch {
                0..=16 | 22 => [0, 0, 16384],
                20 => [0, 0, -16384],
                21 => [16384, 0, 0],
                _ => random.direction(),
            },
            ambient: if batch == 18 {
                256
            } else {
                (random.next() % 257) as u16
            },
            directional: if batch == 18 {
                0
            } else if batch == 22 {
                256
            } else {
                (random.next() % 256 + 1) as u16
            },
        };
        let mut ctx = context(
            m,
            light,
            Projection {
                k: 12288,
                ray_scale: [-12288, 12288],
            },
        );
        ctx.epoch = batch;
        assert!(
            advance(
                &mut emu,
                LightingTick {
                    context: Some(ctx),
                    ..idle()
                }
            )
            .context_ready
        );
        let mut pixels = Vec::new();
        for i in 0..112 {
            let normal = match i {
                0 => [0, 0, 0],
                1 => [0, 0, 3],
                2 => [0, 0, 4],
                3 => [-32768; 3],
                4 => [32767; 3],
                5 => [0, 0, 16384],
                6 => [0, 0, -16384],
                7 => [63, -64, 65],
                8..=28 if batch == 20 => [-16384, 0, -1],
                // Every normalization pre-scale bin, on both sides of a power
                // of two; include signs and discarded-bit rounding boundaries.
                29..=58 => {
                    let scale = 1_i32 << ((i - 29) / 2);
                    let a = if i % 2 == 0 { scale - 1 } else { scale + 1 };
                    [a as i16, -(a as i16), (a / 2) as i16]
                }
                59..=88 => {
                    let scale = 1_i32 << ((i - 59) / 2);
                    let a = if i % 2 == 0 { scale } else { scale - 1 };
                    [-(a as i16), (a / 2 + 1) as i16, (a / 4) as i16]
                }
                _ => std::array::from_fn(|_| random.next() as i16),
            };
            let ndc = match i {
                0 => [-16384; 2],
                1 => [16384; 2],
                2 | 5 | 6 => [0; 2],
                8..=28 if batch == 20 => [500 + (i - 8), 0],
                _ => std::array::from_fn(|_| (random.next() % 32769) as i32 - 16384),
            };
            pixels.push(PixelInput { normal, ndc });
        }
        let mut input = 0;
        let mut output = 0;
        let mut expected = std::collections::VecDeque::new();
        let mut ce_cycles: usize = 0;
        let mut last_accept = None;
        for wall in 0..2000 {
            let ce = wall % 13 != 7 && wall % 13 != 8;
            // Hold an output long enough to freeze a completely occupied pipe.
            let ready = !(120..260).contains(&wall) && wall % 17 < 13;
            let tick = LightingTick {
                ce,
                output_ready: ready,
                input: pixels.get(input).map(|&pixel| LightingRequest {
                    pixel,
                    id: serial.wrapping_mul(0x9e3779b9) ^ 0x80004001,
                }),
                // Assert an unaccepted context update while old tokens remain.
                context: if wall == 100 {
                    Some(LightingContext { epoch: 999, ..ctx })
                } else {
                    None
                },
                ..idle()
            };
            let signals = advance(&mut emu, tick);
            if tick.context.is_some() {
                last_accept = None;
            }
            if ce && !(signals.output.is_some() && !ready) {
                ce_cycles += 1;
            }
            if tick.input.is_some() && signals.input_ready {
                let p = pixels[input];
                let g = oracle::evaluate(
                    p,
                    m,
                    light,
                    ctx.projection,
                    oracle::Config::from_counted(kernel),
                )
                .unwrap();
                expected.push_back(LightingResult {
                    output: LightingOutput {
                        g: g.g as u16,
                        h: g.h as u16,
                    },
                    id: serial.wrapping_mul(0x9e3779b9) ^ 0x80004001,
                    epoch: batch,
                });
                if let Some(last) = last_accept {
                    assert_eq!(
                        ce_cycles - last,
                        emu.initiation_interval(),
                        "steady {profile:?} mode {}",
                        ctx.mode()
                    );
                }
                last_accept = Some(ce_cycles);
                input += 1;
                serial += 1;
            }
            if ce && ready {
                if let Some(result) = signals.output {
                    assert_eq!(result, expected.pop_front().unwrap());
                    output += 1;
                }
            }
            if input == pixels.len() && output == pixels.len() {
                break;
            }
            assert!(wall < 1999, "stream watchdog");
        }
        assert!(expected.is_empty());
        assert_eq!(emu.in_flight(), 0);
    }
    // Flush numerical jobs while in flight; CE=0 cannot suppress reset.
    let ctx = context(Material::default(), Light::default(), Projection::default());
    advance(
        &mut emu,
        LightingTick {
            context: Some(ctx),
            ..idle()
        },
    );
    advance(
        &mut emu,
        LightingTick {
            input: Some(LightingRequest {
                pixel: PixelInput {
                    normal: [32767; 3],
                    ndc: [1; 2],
                },
                id: 42,
            }),
            ..idle()
        },
    );
    for _ in 0..30 {
        advance(&mut emu, idle());
    }
    advance(
        &mut emu,
        LightingTick {
            reset: true,
            ce: false,
            ..idle()
        },
    );
    for _ in 0..emu.latency() + 3 {
        assert!(advance(&mut emu, idle()).output.is_none());
    }
    let ctx = LightingContext { epoch: 77, ..ctx };
    advance(
        &mut emu,
        LightingTick {
            context: Some(ctx),
            ..idle()
        },
    );
    advance(
        &mut emu,
        LightingTick {
            input: Some(LightingRequest {
                pixel: PixelInput {
                    normal: [0, 0, 16384],
                    ndc: [0; 2],
                },
                id: 43,
            }),
            ..idle()
        },
    );
    for _ in 0..emu.latency() {
        advance(&mut emu, idle());
    }
    assert_eq!(
        advance(&mut emu, idle()).output,
        Some(LightingResult {
            output: LightingOutput { g: 256, h: 224 },
            id: 43,
            epoch: 77
        })
    );
}

#[test]
fn stream_has_real_payloads_both_profiles_stalls_drain_context_and_reset() {
    for profile in [
        LightingProfile::Fast,
        LightingProfile::Compact,
        LightingProfile::SystemFast,
        LightingProfile::SystemCompact,
    ] {
        exercise(profile, false, |_, _, _| {});
    }
}

#[test]
fn clock_budget_and_accepted_input_validation_are_explicit() {
    assert!(LightingEmu::new(0).is_err());
    let mut emu = LightingEmu::new(2).unwrap();
    assert!(emu
        .tick(LightingTick {
            input: Some(LightingRequest {
                pixel: PixelInput {
                    normal: [0; 3],
                    ndc: [i32::MAX; 2]
                },
                id: 0
            }),
            ..idle()
        })
        .is_ok());
    emu.tick(idle()).unwrap();
    assert!(emu.tick(idle()).unwrap_err().contains("budget"));
    let mut emu = LightingEmu::new(10).unwrap();
    emu.tick(LightingTick {
        context: Some(context(
            Material::default(),
            Light::default(),
            Projection::default(),
        )),
        ..idle()
    })
    .unwrap();
    assert!(emu
        .tick(LightingTick {
            input: Some(LightingRequest {
                pixel: PixelInput {
                    normal: [0; 3],
                    ndc: [16385, 0]
                },
                id: 0
            }),
            ..idle()
        })
        .is_err());
    assert_eq!(emu.in_flight(), 0);
}

#[test]
#[ignore = "requires Icarus and GOWIN_HOME for the vendor primitive comparison"]
fn verilog_matches_cycle_payloads_and_all_published_stages() {
    use std::fmt::Write;
    let dedicated = std::env::var_os("LIGHTING_DEDICATED_DSP").is_some();
    let scalar = std::env::var_os("LIGHTING_SCALAR_NORM").is_some();
    let shared_ids = std::env::var_os("LIGHTING_SHARED_IDS").is_some();
    let block = std::env::var_os("LIGHTING_BLOCK_PRESCALE").is_some();
    let roles = std::env::var_os("LIGHTING_ROLE_SCHEDULE").is_some();
    let direct = std::env::var_os("LIGHTING_DIRECT_SQUARE").is_some();
    let split = std::env::var("LIGHTING_SPLIT_CONES").map_or(
        gpu_v2::lighting::rtl::LightingRtlOptions::default().split_cones,
        |v| v != "0",
    );
    let system = std::env::var_os("LIGHTING_SYSTEM_PROFILE").is_some();
    let profiles = if system {
        [LightingProfile::SystemFast, LightingProfile::SystemCompact]
    } else {
        [LightingProfile::Fast, LightingProfile::Compact]
    };
    for profile in profiles {
        let resource = std::env::var_os("LIGHTING_RESOURCE_PROFILE").is_some();
        let mut options = if system || std::env::var_os("LIGHTING_FACTOR_KERNEL").is_some() {
            gpu_v2::lighting::rtl::LightingRtlOptions::system_profile()
        } else if resource {
            gpu_v2::lighting::rtl::LightingRtlOptions::resource_profile(profile)
        } else {
            gpu_v2::lighting::rtl::LightingRtlOptions {
                free_slots: true,
                split_cones: split,
                dedicated_dsp: dedicated,
                scalar_norm: scalar,
                shared_ids,
                block_prescale: block,
                role_schedule: roles,
                direct_square: direct,
                ..Default::default()
            }
        };
        options.lit_queue = std::env::var_os("LIGHTING_LIT_QUEUE").is_some();
        // Explicit experiments must also override the resource constructor.
        // Otherwise LIGHTING_SPLIT_CONES=0 silently tests the default backend.
        if std::env::var_os("LIGHTING_SPLIT_CONES").is_some() {
            options.split_cones = split;
        }
        options.q_windows = std::env::var_os("LIGHTING_Q_WINDOWS").is_some();
        options.shallow_normal_ff = profile == LightingProfile::Fast
            && std::env::var_os("LIGHTING_SHALLOW_NORMAL_FF").is_some();
        options.cost_cut = std::env::var_os("LIGHTING_COST_CUT").is_some();
        options.stationary_logic = std::env::var_os("LIGHTING_STATIONARY_LOGIC").is_some();
        options.logic_depth = std::env::var("LIGHTING_LOGIC_DEPTH")
            .ok()
            .map(|v| v.parse().unwrap())
            .unwrap_or(0);
        if std::env::var_os("LIGHTING_SCALED_GATE").is_some() {
            options.exact_normal_gate = false;
        }
        options.quantization = if std::env::var_os("LIGHTING_COMPENSATED_FLOOR").is_some() {
            gpu_v2::lighting::LightingQuantization::CompensatedFloor
        } else {
            Default::default()
        };
        options.retiming = retiming(profile);
        options.dsp_steering = match std::env::var("LIGHTING_DSP_STEERING").as_deref() {
            Ok("joint") => gpu_v2::lighting::rtl::DspSteering::Joint,
            Ok("local") => gpu_v2::lighting::rtl::DspSteering::Local,
            Ok("orient") => gpu_v2::lighting::rtl::DspSteering::Orient,
            _ => gpu_v2::lighting::rtl::DspSteering::None,
        };
        options.one_hot_dsp = std::env::var_os("LIGHTING_ONEHOT_DSP").is_some();
        let rtl = gpu_v2::lighting::rtl::generate_with_options(profile, options).unwrap();
        let dir=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../target/gpu-v2-lighting/rtl-cosim-f{}-l{}-s{}-r{}-c{}-{profile:?}-resource-{resource}-dedicated-{dedicated}-scalar-{scalar}-block-{block}-roles-{roles}-direct-{direct}-depth-{}-stationary-{}-cut-{}-window-{}-normalff-{}",u8::from(options.retiming.measured_functions),options.retiming.extra_large_multiply,options.retiming.extra_small_multiply,options.retiming.extra_normalize_reads,u8::from(options.retiming.compact_lifetimes),options.logic_depth,options.stationary_logic,options.cost_cut,options.q_windows,options.shallow_normal_ff));
        let dir =
            if options.quantization == gpu_v2::lighting::LightingQuantization::CompensatedFloor {
                dir.join("compensated-floor")
            } else {
                dir
            };
        let dir = if options.dsp_steering != gpu_v2::lighting::rtl::DspSteering::None
            || options.one_hot_dsp
        {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                "../../target/gpu-v2-lighting/steering-{profile:?}-{:?}-onehot{}-resource{resource}-f{}-l{}-s{}-c{}",
                options.dsp_steering, options.one_hot_dsp,
                options.retiming.measured_functions, options.retiming.extra_large_multiply,
                options.retiming.extra_small_multiply, options.retiming.compact_lifetimes
            ))
        } else {
            dir
        };
        // Keep queue variants and both numerical policies in disjoint evidence
        // directories; DSP steering must not overwrite the quantization suffix.
        let dir = if options.lit_queue {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                "../../target/lighting-lit-queue-20261006/rtl-{profile:?}-{:?}",
                options.quantization
            ))
        } else {
            dir
        };
        let dir = if !options.split_cones {
            dir.join("unsplit-cones")
        } else {
            dir
        };
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("lighting.v"), &rtl.source).unwrap();
        let mut tb = String::from(
            r#"module tb;
reg clk=0,reset=0,ce=0,context_valid=0,in_valid=0,out_ready=0;
wire context_ready,in_ready,out_valid;
reg [15:0] context_epoch=0; reg [1:0] context_mode=0; reg [4:0] context_code=0;
reg signed [15:0] light_x=0,light_y=0,light_z=0,ray_x=0,ray_y=0,ray_k=0;
reg [8:0] ambient=0,directional=0;
reg [31:0] in_id=0; reg [35:0] in_row0=0,in_row1=0,in_row2=0;
wire [31:0] out_id; wire [15:0] out_epoch; wire [8:0] out_g,out_h;
gpu_v2_lighting dut(.*);
`ifdef GPU_V2_GOWIN_DSP
GSR GSR(.GSRI(1'b1));
`endif
initial begin #500000; $fatal(1,"simulation watchdog"); end
initial begin
"#,
        );
        let mut tick_count = 0;
        let mut output_checks = 0;
        let mut stage_checks = 0;
        exercise(profile, dedicated, |tick, signals, stages| {
            writeln!(
                tb,
                "reset={};ce={};context_valid={};in_valid={};out_ready={};",
                u8::from(tick.reset),
                u8::from(tick.ce),
                u8::from(tick.context.is_some()),
                u8::from(tick.input.is_some()),
                u8::from(tick.output_ready)
            )
            .unwrap();
            if options.id_ring {
                writeln!(tb,"if(dut.datapath_ce && in_valid && in_ready && ((dut.full_mode && dut.valid_pipe[{}]) || (!dut.full_mode && dut.valid_pipe[{}])) && dut.id_write==dut.id_read) $fatal(1,\"ID ring read/write collision cycle {tick_count}\");",rtl.latency-1,rtl.diffuse_latency-1).unwrap();
            }
            if let Some(c) = tick.context {
                writeln!(tb,"context_epoch={};context_mode={};context_code={};ambient={};directional={};light_x=16'h{:04x};light_y=16'h{:04x};light_z=16'h{:04x};ray_x=16'h{:04x};ray_y=16'h{:04x};ray_k=16'h{:04x};",
                c.epoch,c.mode(),c.material.shininess_code,c.light.ambient,c.light.directional,
                c.light.direction[0] as u16,c.light.direction[1] as u16,c.light.direction[2] as u16,c.projection.ray_scale[0] as u16,c.projection.ray_scale[1] as u16,c.projection.k as u16).unwrap();
            }
            if let Some(i) = tick.input {
                let rows = PixelRows::encode(i.pixel).unwrap();
                writeln!(
                    tb,
                    "in_id={};in_row0=36'h{:x};in_row1=36'h{:x};in_row2=36'h{:x};",
                    i.id, rows.0[0], rows.0[1], rows.0[2]
                )
                .unwrap();
            }
            writeln!(tb, "#1;").unwrap();
            if tick_count > 0 {
                writeln!(tb,"if(context_ready !== 1'b{} || in_ready !== 1'b{} || out_valid !== 1'b{}) $fatal(1,\"handshake cycle {tick_count}: ctx=%b in=%b out=%b\",context_ready,in_ready,out_valid);",u8::from(signals.context_ready),u8::from(signals.input_ready),u8::from(signals.output.is_some())).unwrap();
            }
            if let Some(o) = signals.output {
                writeln!(tb,"if(out_id !== 32'd{} || out_epoch !== 16'd{} || out_g !== 9'd{} || out_h !== 9'd{}) $fatal(1,\"output cycle {tick_count}: id=%d epoch=%d g=%d h=%d\",out_id,out_epoch,out_g,out_h);",o.id,o.epoch,o.output.g,o.output.h).unwrap();
                output_checks += 1;
            }
            for (id, full, name, raw) in stages {
                let probe = rtl
                    .stages
                    .iter()
                    .find(|s| s.name == name && s.full == full)
                    .unwrap();
                let expr = if probe.signal.contains('\'') {
                    probe.signal.clone()
                } else {
                    format!("dut.{}", probe.signal)
                };
                let encoded = raw & ((1_i128 << probe.bits) - 1);
                let identity = if options.id_ring {
                    String::new()
                } else {
                    format!("dut.id_pipe[{}] !== 32'd{id} || ", probe.age)
                };
                writeln!(tb,"if({identity}{expr} !== {}'h{encoded:x}) $fatal(1,\"stage {name} cycle {tick_count} id {id}: got=%h expected=%h\",{expr},{}'h{encoded:x});",probe.bits,probe.bits).unwrap();
                stage_checks += 1;
            }
            writeln!(tb, "clk=1;#1;clk=0;#1;").unwrap();
            tick_count += 1;
        });
        writeln!(tb,"$display(\"PASS cycles={tick_count} outputs={output_checks} stages={stage_checks}\");$finish;end endmodule").unwrap();
        std::fs::write(dir.join("tb.v"), tb).unwrap();
        let compiler = std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into());
        let runtime = std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into());
        for vendor in [false, true] {
            let mut compile = std::process::Command::new(&compiler);
            compile
                .current_dir(&dir)
                .args(["-g2012", "-s", "tb", "-o", "test.vvp"]);
            if vendor {
                let home = std::env::var_os("GOWIN_HOME")
                    .expect("vendor primitive co-sim needs GOWIN_HOME");
                let library = std::path::Path::new(&home).join("IDE/simlib/gw2a/prim_sim.v");
                compile.arg("-DGPU_V2_GOWIN_DSP");
                compile.arg(library);
            }
            compile.args(["lighting.v", "tb.v"]);
            let compiled = compile.output().unwrap();
            assert!(
                compiled.status.success(),
                "{}",
                String::from_utf8_lossy(&compiled.stderr)
            );
            let ran = std::process::Command::new(&runtime)
                .current_dir(&dir)
                .arg("test.vvp")
                .output()
                .unwrap();
            let output = String::from_utf8_lossy(&ran.stdout);
            assert!(
                ran.status.success() && output.contains("PASS cycles="),
                "vendor={vendor}: {output}\n{}",
                String::from_utf8_lossy(&ran.stderr)
            );
            println!("profile={profile:?} vendor={vendor}: {output}");
        }
    }
}
