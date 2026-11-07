//! Source-matched bounded actual-cycle replay against the frozen calendar and numerical DAG.
use gpu_v2::lighting::{
    calendars::UnifiedCalendar, dsp_sharing, emu::LightingEmu, ports::*, rtl, sim::counted,
    LightingProfile, LightingQuantization,
};
use std::{collections::BTreeMap, fmt::Write, fs, path::PathBuf, process::Command};
fn idle() -> LightingTick {
    LightingTick {
        reset: false,
        ce: true,
        context: None,
        input: None,
        output_ready: true,
    }
}

#[test]
fn selected_h9_factory_uses_reviewed_b_and_retains_frozen_reference() {
    let q = LightingQuantization::CompensatedFloor;
    let selected = UnifiedCalendar::SmoothH9;
    let options = selected.options(q);
    let plans = selected.plans(q).unwrap();
    let core = rtl::generate_with_schedule_plans(LightingProfile::Fast, options, &plans).unwrap();
    let (reviewed_options, reviewed_plans) =
        dsp_sharing::shared_scalar_pairs(false, false).unwrap();
    let reviewed =
        rtl::generate_with_schedule_plans(LightingProfile::Fast, reviewed_options, &reviewed_plans)
            .unwrap();
    assert_eq!(core.source, reviewed.source);
    assert_eq!(
        (
            core.latency,
            core.specular_ii,
            core.large_multipliers,
            core.small_multipliers,
            core.pair_macros
        ),
        (34, 2, 7, 9, 1)
    );
    let emu =
        LightingEmu::with_schedule_plans(LightingProfile::Fast, options, &plans, 50000).unwrap();
    assert_eq!((emu.latency(), emu.initiation_interval()), (34, 2));
    let consumer =
        gpu_v2::system::pixel::lighting_consumer::verilog_with_calendar(selected).unwrap();
    assert!(consumer.ends_with(&core.source));
    let reference = UnifiedCalendar::SmoothH9Reference;
    let reference_core = rtl::generate_with_schedule_plans(
        LightingProfile::Fast,
        reference.options(q),
        &reference.plans(q).unwrap(),
    )
    .unwrap();
    assert_eq!(
        (
            reference_core.latency,
            reference_core.specular_ii,
            reference_core.large_multipliers,
            reference_core.small_multipliers,
            reference_core.pair_macros
        ),
        (34, 2, 9, 9, 2)
    );
    assert_eq!(UnifiedCalendar::selected(q), UnifiedCalendar::Free);
    assert_eq!(
        UnifiedCalendar::selected(LightingQuantization::NearestEven),
        UnifiedCalendar::TwoEdge
    );
    assert!(selected.plans(LightingQuantization::NearestEven).is_err());
}

#[test]
#[ignore = "requires native and portable Icarus replay"]
fn candidate_matches_frozen_calendar_numerical_dag_and_both_rtl_backends() {
    let variant = std::env::var("LIGHTING_SHARING_VARIANT").unwrap_or("selected".into());
    let q = LightingQuantization::CompensatedFloor;
    let base_options = UnifiedCalendar::SmoothH9Reference.options(q);
    let base_plans = UnifiedCalendar::SmoothH9Reference.plans(q).unwrap();
    let (options, plans) = match variant.as_str() {
        "selected" => (
            UnifiedCalendar::SmoothH9.options(q),
            UnifiedCalendar::SmoothH9.plans(q).unwrap(),
        ),
        "baseline" => (base_options, base_plans.clone()),
        "A" => dsp_sharing::shared_dots().unwrap(),
        "B" => dsp_sharing::shared_scalar_pairs(false, false).unwrap(),
        "Bsmall" => dsp_sharing::shared_scalar_pairs(true, false).unwrap(),
        "C" => dsp_sharing::shared_scalar_pairs(false, true).unwrap(),
        "D" => dsp_sharing::shared_scalar_pairs(true, true).unwrap(),
        _ => panic!("unknown candidate"),
    };
    let rtl = rtl::generate_with_schedule_plans(LightingProfile::Fast, options, &plans).unwrap();
    let dir = PathBuf::from(std::env::var("LIGHTING_SHARING_OUTPUT").expect("output directory"));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("lighting.v"), &rtl.source).unwrap();
    let mut emu =
        LightingEmu::with_schedule_plans(LightingProfile::Fast, options, &plans, 50000).unwrap();
    let mut frozen =
        LightingEmu::with_schedule_plans(LightingProfile::Fast, base_options, &base_plans, 50000)
            .unwrap();
    assert_eq!((emu.latency(), emu.initiation_interval()), (34, 2));
    let mut tb = String::from(
        r#"module tb;
reg clk=0,reset=0,ce=0,context_valid=0,in_valid=0,out_ready=0;
wire context_ready,in_ready,out_valid;
reg[15:0]context_epoch=0;reg[1:0]context_mode=0;reg[4:0]context_code=0;
reg signed[15:0]light_x=0,light_y=0,light_z=0,ray_x=0,ray_y=0,ray_k=0;
reg[8:0]ambient=0,directional=0;reg[31:0]in_id=0;reg[35:0]in_row0=0,in_row1=0,in_row2=0;
wire[31:0]out_id;wire[15:0]out_epoch;wire[8:0]out_g,out_h;gpu_v2_lighting dut(.*);
`ifdef GPU_V2_GOWIN_DSP
GSR GSR(.GSRI(1'b1));
`endif
initial begin #200000;$fatal(1,"bounded simulation watchdog");end
initial begin
"#,
    );
    let mut cycles = 0usize;
    let mut checks = 0usize;
    let mut results = 0usize;
    let mut accepted = 0usize;
    let mut ce0 = 0usize;
    let mut blocked = 0usize;
    let mut contexts = 0usize;
    let mut expected = BTreeMap::new();
    let mut active_context: Option<LightingContext> = None;
    let mut edge = |tick: LightingTick| {
        let signal = emu.signals(tick);
        let ref_signal = frozen.signals(tick);
        assert_eq!(signal, ref_signal, "frozen signals at edge {cycles}");
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
        if let Some(c) = tick.context {
            writeln!(tb,"context_epoch={};context_mode={};context_code={};ambient={};directional={};light_x=16'h{:x};light_y=16'h{:x};light_z=16'h{:x};ray_x=16'h{:x};ray_y=16'h{:x};ray_k=16'h{:x};",c.epoch,c.mode(),c.material.shininess_code,c.light.ambient,c.light.directional,c.light.direction[0] as u16,c.light.direction[1] as u16,c.light.direction[2] as u16,c.projection.ray_scale[0] as u16,c.projection.ray_scale[1] as u16,c.projection.k as u16).unwrap();
        }
        if let Some(i) = tick.input {
            let rows = PixelRows::encode(i.pixel).unwrap();
            writeln!(
                tb,
                "in_id=32'h{:x};in_row0=36'h{:x};in_row1=36'h{:x};in_row2=36'h{:x};",
                i.id, rows.0[0], rows.0[1], rows.0[2]
            )
            .unwrap();
        }
        tb.push_str("#1;\n");
        if cycles > 0 {
            writeln!(tb,"if(context_ready!==1'b{} || in_ready!==1'b{} || out_valid!==1'b{}) $fatal(1,\"handshake edge {cycles}\");",u8::from(signal.context_ready),u8::from(signal.input_ready),u8::from(signal.output.is_some())).unwrap();
        }
        if let Some(o) = signal.output {
            assert_eq!(
                o.output,
                *expected
                    .get(&o.id)
                    .expect("independent accepted-pixel numerical golden")
            );
            writeln!(tb,"if(out_id!==32'h{:x} || out_epoch!==16'h{:x} || out_g!==9'd{} || out_h!==9'd{}) $fatal(1,\"payload edge {cycles}\");",o.id,o.epoch,o.output.g,o.output.h).unwrap();
            results += 1;
        }
        for (_, full, name, raw) in
            emu.stage_values()
                .into_iter()
                .chain(emu.boundary_values().into_iter().filter(|(_, _, name, _)| {
                    [291, 773, 516, 236, 288, 770, 120, 633, 251, 266]
                        .iter()
                        .any(|id| name == &format!("event:{id}"))
                }))
        {
            let probe = rtl
                .stages
                .iter()
                .chain(&rtl.boundaries)
                .find(|s| s.full == full && s.name == name)
                .unwrap();
            let expr = if probe.signal.contains('\'') {
                probe.signal.clone()
            } else {
                format!("dut.{}", probe.signal)
            };
            let encoded = raw & ((1i128 << probe.bits) - 1);
            writeln!(
                tb,
                "if({expr}!=={}'h{encoded:x}) $fatal(1,\"stage {name} edge {cycles}\");",
                probe.bits
            )
            .unwrap();
            checks += 1;
        }
        if tick.reset {
            expected.clear();
        } else if signal.input_ready && tick.input.is_some() {
            let i = tick.input.unwrap();
            let c = active_context.expect("loaded context");
            let report = counted::evaluate_with_config(
                i.pixel,
                c.material,
                c.light,
                c.projection,
                4096,
                counted::Config {
                    smooth_h9: true,
                    h9_tail_dsp: true,
                    rsqrt_q13: true,
                    ..counted::Config::lit_queue_resource_profile(LightingProfile::Fast, q)
                },
            )
            .unwrap();
            expected.insert(i.id, report.output);
            accepted += 1;
        }
        if tick.context.is_some() && signal.context_ready {
            active_context = tick.context;
            contexts += 1;
        }
        if tick.reset {
            active_context = None;
        }
        ce0 += usize::from(!tick.ce);
        blocked += usize::from(signal.output.is_some() && !tick.output_ready);
        assert_eq!(emu.tick(tick).unwrap(), signal);
        assert_eq!(frozen.tick(tick).unwrap(), ref_signal);
        tb.push_str("clk=1;#1;clk=0;#1;\n");
        cycles += 1;
        signal
    };
    edge(LightingTick {
        reset: true,
        ce: false,
        ..idle()
    });
    for batch in 0..51usize {
        let mode = 1 + batch % 3;
        let code = (batch / 3) as u8;
        let c = LightingContext {
            material: Material {
                shininess_code: code,
                specular_color: if mode == 2 { [0; 3] } else { [255; 3] },
                unlit: false,
            },
            light: Light {
                direction: [
                    [0, 0, 16384],
                    [0, 0, -16384],
                    [11585, -11585, 0],
                    [0, 16384, 0],
                ][batch % 4],
                ambient: [0, 1, 255, 256][batch % 4],
                directional: if mode == 1 {
                    0
                } else {
                    [1, 127, 255, 256][batch % 4]
                },
            },
            projection: Projection {
                ray_scale: [12288, -8192],
                k: [8192, 10240, 12288][batch % 3],
            },
            epoch: 0xffff - (batch as u16),
        };
        let mut loaded = false;
        let n = if batch == 48 { 128 } else { 12 };
        let mut sent = 0usize;
        for wall in 0..2000usize {
            let ce = wall < 8 || wall % 11 != 3 && wall % 11 != 4;
            let ready = wall < 10 || wall % 29 < 21;
            let request = (loaded && sent < n).then(|| LightingRequest {
                pixel: PixelInput {
                    normal: [
                        [0; 3],
                        [32767, -32768, 1],
                        [1, 0, -1],
                        [0, 0, 16384],
                        [0, 0, -16384],
                        [377 * 16, -286 * 16, 939 * 16],
                        [-16384, 16384, 0],
                        [i16::MIN; 3],
                    ][sent % 8],
                    ndc: [
                        [0, 0],
                        [-16384, 16384],
                        [16384, -16384],
                        [
                            (sent as i32 * 7919 % 32769) - 16384,
                            (sent as i32 * 3571 % 32769) - 16384,
                        ],
                    ][sent % 4],
                },
                id: 0x80000000 | ((batch as u32) << 16) | sent as u32,
            });
            let s = edge(LightingTick {
                ce,
                context: (!loaded).then_some(c),
                input: request,
                output_ready: ready,
                ..idle()
            });
            if s.context_ready && !loaded {
                loaded = true;
            }
            if s.input_ready && request.is_some() {
                sent += 1;
            }
            if loaded && sent == n && !s.output.is_some() && wall > n * 2 + 120 {
                break;
            }
            assert!(wall < 1999, "bounded batch timeout");
        }
    }
    assert_eq!(contexts, 51);
    assert!(accepted >= 700 && ce0 > 200 && blocked > 100 && checks > 10000);
    writeln!(tb,"$display(\"PASS cycles={cycles} accepted={accepted} contexts={contexts} CE0={ce0} blocked={blocked} output_checks={results} stages={checks}\");$finish;end endmodule").unwrap();
    fs::write(dir.join("tb.v"), tb).unwrap();
    for native in [false, true] {
        let mut cmd = Command::new(std::env::var_os("IVERILOG_EXE").unwrap());
        cmd.current_dir(&dir).args([
            "-g2012",
            "-s",
            "tb",
            "-o",
            if native { "native.vvp" } else { "portable.vvp" },
        ]);
        if native {
            cmd.arg("-DGPU_V2_GOWIN_DSP").arg(
                PathBuf::from(std::env::var_os("GOWIN_HOME").unwrap())
                    .join("IDE/simlib/gw2a/prim_sim.v"),
            );
        }
        let compiled = cmd.args(["lighting.v", "tb.v"]).output().unwrap();
        fs::write(
            dir.join(if native {
                "native-compile.log"
            } else {
                "portable-compile.log"
            }),
            &compiled.stderr,
        )
        .unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let run = Command::new(std::env::var_os("VVP_EXE").unwrap())
            .current_dir(&dir)
            .arg(if native { "native.vvp" } else { "portable.vvp" })
            .output()
            .unwrap();
        let output = String::from_utf8_lossy(&run.stdout);
        fs::write(
            dir.join(if native {
                "native-replay.log"
            } else {
                "portable-replay.log"
            }),
            &run.stdout,
        )
        .unwrap();
        assert!(
            run.status.success() && output.contains("PASS"),
            "native={native} {output} {}",
            String::from_utf8_lossy(&run.stderr)
        );
        println!("native={native} {output}");
    }
}
