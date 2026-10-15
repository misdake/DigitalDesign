//! Finite matched emu/RTL qualification for the explicit smooth-h9 program.
use gpu_v2::lighting::{
    calendars::UnifiedCalendar, emu::LightingEmu, ports::*, rtl, sim::counted, LightingProfile,
    LightingQuantization,
};
use std::{collections::BTreeMap, fmt::Write, fs, path::Path};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/h9-probe".into());
    let root = Path::new(&arg);
    fs::create_dir_all(root)?;
    let q = LightingQuantization::CompensatedFloor;
    let cal = UnifiedCalendar::SmoothH9;
    let opts = cal.options(q);
    let plans = cal.plans(q)?;
    let core = rtl::generate_with_schedule_plans(LightingProfile::Fast, opts, &plans)?;
    assert_eq!(
        (
            core.latency,
            core.diffuse_latency,
            core.specular_ii,
            core.diffuse_ii
        ),
        (34, 34, 2, 2)
    );
    fs::write(root.join("behavioral.v"), &core.source)?;
    fs::write(
        root.join("native.v"),
        format!("`define GPU_V2_GOWIN_DSP\n{}", core.source),
    )?;
    fs::write(root.join("storage.csv"), &core.storage_csv)?;
    fs::write(root.join("dsp-inputs.csv"), &core.dsp_input_csv)?;
    let kernel = counted::Config {
        smooth_h9: true,
        h9_tail_dsp: true,
        rsqrt_q13: true,
        ..counted::Config::lit_queue_resource_profile(LightingProfile::Fast, q)
    };
    let mut emu = LightingEmu::with_schedule_plans(LightingProfile::Fast, opts, &plans, 50000)?;
    let idle = LightingTick {
        reset: false,
        ce: true,
        context: None,
        input: None,
        output_ready: true,
    };
    emu.tick(LightingTick {
        reset: true,
        ..idle
    })?;
    let mut tb=String::from("module tb;reg clk=0,reset=1,ce=1,context_valid=0,in_valid=0,out_ready=1;reg[15:0]context_epoch=0;reg[1:0]context_mode=0;reg[4:0]context_code=0;reg signed[15:0]light_x=0,light_y=0,light_z=16384,ray_x=-8192,ray_y=-8192,ray_k=8192;reg[8:0]ambient=64,directional=192;reg[31:0]in_id=0;reg[35:0]in_row0=0,in_row1=0,in_row2=0;wire context_ready,in_ready,out_valid;wire[31:0]out_id;wire[15:0]out_epoch;wire[8:0]out_g,out_h;gpu_v2_lighting dut(.*);\n`ifdef GPU_V2_GOWIN_DSP\nGSR GSR(.GSRI(1'b1));\n`endif\ninitial begin #1000000;$fatal(1,\"bounded timeout\");end\ninitial begin #2;clk=1;#2;clk=0;reset=0;\n");
    let mut cycles = 0;
    let mut outputs = 0;
    for case in 0..19_u8 {
        let context = LightingContext {
            material: Material {
                shininess_code: case.min(16),
                specular_color: if case < 17 { [255; 3] } else { [0; 3] },
                ..Material::default()
            },
            light: Light {
                ambient: 64,
                directional: if case == 18 { 0 } else { 192 },
                direction: if case & 1 == 0 {
                    [0, 0, 16384]
                } else {
                    [-8192, 8192, 11585]
                },
            },
            projection: Projection::default(),
            epoch: u16::from(case) + 0x120,
        };
        let mut sent = 0;
        let mut loaded = false;
        let mut pending = BTreeMap::new();
        let mut done = false;
        for step in 0..4096 {
            let request = (loaded && sent < 32).then(|| {
                let k = sent as i32;
                let special = [
                    [0, 0, 0],
                    [0, 0, 1024],
                    [0, 0, -1024],
                    [1, 0, 0],
                    [-1, 0, 0],
                    [2047, -2048, 2047],
                ];
                let n = special.get(sent).copied().unwrap_or([
                    (k * 311) % 4096 - 2048,
                    (k * 197 + 13) % 4096 - 2048,
                    (k * 83 + 41) % 4096 - 2048,
                ]);
                LightingRequest {
                    id: 0xf1230000_u32
                        .wrapping_add(u32::from(case) * 0x101)
                        .wrapping_add(sent as u32),
                    pixel: PixelInput {
                        normal: n.map(|v| (v * 16) as i16),
                        ndc: [(k * 503) % 32769 - 16384, (k * 1097 + 7) % 32769 - 16384],
                    },
                }
            });
            let tick = LightingTick {
                ce: step % 7 != 2 && step % 19 != 4,
                context: (!loaded).then_some(context),
                input: request,
                output_ready: step % 11 > 2,
                ..idle
            };
            let mut stages = emu.stage_values();
            stages.extend(emu.boundary_values());
            let signals = emu.tick(tick)?;
            writeln!(tb,"ce={};context_valid={};in_valid={};out_ready={};context_epoch={};context_mode={};context_code={};light_x={};light_y={};light_z={};ambient={};directional={};",u8::from(tick.ce),u8::from(tick.context.is_some()),u8::from(request.is_some()),u8::from(tick.output_ready),context.epoch,context.mode(),context.material.shininess_code,context.light.direction[0],context.light.direction[1],context.light.direction[2],context.light.ambient,context.light.directional)?;
            if let Some(req) = request {
                let rows = PixelRows::encode(req.pixel)
                    .map_err(|e| format!("{e:?}"))?
                    .0;
                writeln!(
                    tb,
                    "in_id={};in_row0=36'h{:x};in_row1=36'h{:x};in_row2=36'h{:x};",
                    req.id, rows[0], rows[1], rows[2]
                )?;
                if signals.input_ready {
                    let report = counted::evaluate_with_config(
                        req.pixel,
                        context.material,
                        context.light,
                        context.projection,
                        4096,
                        kernel,
                    )
                    .map_err(|e| format!("{e:?}"))?;
                    pending.insert(req.id, [report.output.g, report.output.h]);
                    sent += 1;
                }
            }
            writeln!(tb,"#1;if(context_ready!==1'b{}||in_ready!==1'b{}||out_valid!==1'b{})$fatal(1,\"handshake {cycles}\");",u8::from(signals.context_ready),u8::from(signals.input_ready),u8::from(signals.output.is_some()))?;
            for (id, full, name, raw) in stages {
                let probe = core
                    .stages
                    .iter()
                    .chain(core.boundaries.iter())
                    .find(|s| s.name == name && s.full == full)
                    .unwrap();
                let expression = if probe.signal.contains('\'') {
                    probe.signal.clone()
                } else {
                    format!("dut.{}", probe.signal)
                };
                let encoded = raw & ((1_i128 << probe.bits) - 1);
                writeln!(tb,"if({expression} !== {}'h{encoded:x})$fatal(1,\"stage {name} tick {cycles} id {id} got=%h expected=%h\",{expression},{}'h{encoded:x});",probe.bits,probe.bits)?;
            }
            if signals.context_ready && tick.context.is_some() {
                loaded = true;
            }
            if let Some(result) = signals.output {
                assert_eq!(pending[&result.id], [result.output.g, result.output.h]);
                writeln!(tb,"if(out_id!==32'd{}||out_epoch!==16'd{}||out_g!==9'd{}||out_h!==9'd{})$fatal(1,\"payload {cycles} id%0d g%0d h%0d\",out_id,out_g,out_h);",result.id,result.epoch,result.output.g,result.output.h)?;
                if tick.ce && tick.output_ready {
                    pending.remove(&result.id);
                    outputs += 1;
                }
            }
            writeln!(tb, "clk=1;#2;clk=0;#2;")?;
            cycles += 1;
            if loaded && sent == 32 && pending.is_empty() {
                done = true;
                break;
            }
        }
        assert!(done, "context failed finite drain");
    }
    writeln!(tb,"$display(\"PASS h9 matched cycles={cycles} outputs={outputs} contexts=19 max_wall=50000\");$finish;end endmodule")?;
    fs::write(root.join("tb.v"), tb)?;
    fs::write(root.join("structure.txt"),format!("latency=34 II2 cycles={cycles} outputs={outputs} emu=actual_Program counted=independent_static_frame\n"))?;
    println!("H9 emu and counted PASS cycles={cycles} outputs={outputs}");
    Ok(())
}
