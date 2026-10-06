//! Offline two-edge stage experiment over the unchanged, atomically bound DAG.
//! Export/check only: these plans do not replace the production emulator or RTL.
use gpu_v2::lighting::{
    calendars::UnifiedCalendar,
    emu::LightingEmu,
    ports::*,
    rtl::{self, DspSteering, LightingRtlOptions},
    sim::{
        counted, oracle,
        workbench::{SchedulePlan, Slot, Workbench},
    },
    LightingProfile, LightingQuantization,
};
use std::{collections::BTreeMap, error::Error, fmt::Write, fs, path::Path};

fn csv(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "\"\""))
}
fn ids(values: &[usize]) -> String {
    values
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(";")
}
fn dsp(name: &str) -> bool {
    matches!(name, "SmallMultiply" | "LargeMultiply" | "PairMultiplyAdd")
}

fn export(w: &Workbench, root: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(root)?;
    let mut resources = String::from("id,name,lanes,latency,initiation\n");
    for (id, r) in w.graph.resources.iter().enumerate() {
        writeln!(
            resources,
            "{id},{},{},{},{}",
            csv(&r.name),
            r.lanes,
            r.latency,
            r.initiation_interval
        )?;
    }
    fs::write(root.join("resources.csv"), resources)?;
    let mut graph = String::from("id,resource,earliest,parents\n");
    for (id, n) in w.graph.nodes.iter().enumerate() {
        writeln!(
            graph,
            "{id},{},{},{}",
            n.resource.map_or_else(String::new, |r| r.to_string()),
            n.earliest,
            csv(&ids(&n.predecessors))
        )?;
    }
    fs::write(root.join("graph.csv"), graph)?;
    let mut nodes = String::from("id,label,resource,bits,stable,parents,physical_lane,recipe\n");
    let mut ports = String::from("node,direction,value,label,description,sources,constant\n");
    for n in &w.nodes {
        writeln!(
            nodes,
            "{},{},{},{},{},{},{},{}",
            n.id,
            csv(&n.label),
            n.resource,
            n.bits,
            n.stable,
            csv(&ids(&n.parents)),
            n.physical_lane.map_or_else(String::new, |l| l.to_string()),
            csv(&n.recipe.join("\n"))
        )?;
        for (direction, values) in [("input", &n.inputs), ("output", &n.outputs)] {
            for p in values {
                writeln!(
                    ports,
                    "{},{},{},{},{},{},{}",
                    n.id,
                    direction,
                    p.value,
                    csv(&p.label),
                    csv(&p.description),
                    csv(&ids(&p.sources)),
                    p.constant
                )?;
            }
        }
    }
    fs::write(root.join("nodes.csv"), nodes)?;
    fs::write(root.join("ports.csv"), ports)?;
    let mut slots = String::from("id,issue,lane\n");
    for s in w.slots(&w.baseline) {
        writeln!(slots, "{},{},{}", s.id, s.issue, s.lane)?;
    }
    fs::write(root.join("baseline.csv"), slots)?;
    let a = w.inspect(
        w.baseline.initiation_interval,
        &w.slots(&w.baseline),
        &w.graph
            .resources
            .iter()
            .map(|r| r.lanes)
            .collect::<Vec<_>>(),
    )?;
    assert!(a.conflicts.is_empty());
    fs::write(
        root.join("baseline.txt"),
        format!(
            "ii={},span={},accept_to_valid={},macros={},tiles={},live_result_bits={:?}\n",
            w.baseline.initiation_interval,
            a.schedule.span,
            w.latency,
            a.dsp.macros,
            a.dsp.tiles,
            a.live_bits_by_phase
        ),
    )?;
    Ok(())
}

fn load_plan(root: &Path) -> Result<SchedulePlan, Box<dyn Error>> {
    let slots: Vec<Slot> = fs::read_to_string(root.join("slots.csv"))?
        .lines()
        .skip(1)
        .map(|line| {
            let f: Vec<_> = line.split(',').collect();
            Ok(Slot {
                id: f[0].parse()?,
                issue: f[1].parse()?,
                lane: f[2].parse()?,
            })
        })
        .collect::<Result<_, Box<dyn Error>>>()?;
    let capacities: Vec<usize> = fs::read_to_string(root.join("capacities.txt"))?
        .split_whitespace()
        .map(str::parse)
        .collect::<Result<_, _>>()?;
    Ok(SchedulePlan {
        ii: 2,
        slots,
        capacities,
        preconnected_modes: true,
    })
}
fn check(w: &Workbench, root: &Path) -> Result<String, Box<dyn Error>> {
    let plan = load_plan(root)?;
    let a = w.inspect(2, &plan.slots, &plan.capacities)?;
    if !a.conflicts.is_empty() {
        return Err(format!("{}: {:?}", root.display(), a.conflicts).into());
    }
    // Recompute stage locality from the assignment rather than trusting a label.
    let mut owners = BTreeMap::<(usize, usize), Vec<u64>>::new();
    for s in &plan.slots {
        let r = w.graph.nodes[s.id].resource.unwrap();
        if dsp(&w.graph.resources[r].name) {
            owners.entry((r, s.lane)).or_default().push(s.issue / 2);
        }
    }
    let local = owners
        .values()
        .filter(|stages| stages.iter().all(|g| g == &stages[0]))
        .count();
    let report = format!("valid=true,ii=2,span={},macros={},tiles={},local_dsp_channels={}/{},live_result_bits={:?}\n", a.schedule.span, a.dsp.macros, a.dsp.tiles, local, owners.len(), a.live_bits_by_phase);
    fs::write(root.join("checked.txt"), &report)?;
    Ok(report)
}

fn trial(
    root: &Path,
    policy: &str,
    quantization: LightingQuantization,
) -> Result<(), Box<dyn Error>> {
    let reviewed = match std::env::var("LIGHTING_REVIEWED_CALENDAR").as_deref() {
        Ok("free") => Some(UnifiedCalendar::Free),
        Ok("two-edge") => Some(UnifiedCalendar::TwoEdge),
        Ok("selected") => Some(UnifiedCalendar::selected(quantization)),
        Ok(_) => return Err("unknown reviewed calendar".into()),
        Err(_) => None,
    };
    let unified = reviewed.is_some() || std::env::var_os("LIGHTING_UNIFIED_LIT").is_some();
    let legacy = std::env::var_os("LIGHTING_LEGACY_RSQRT").is_some();
    let mut plans = if let Some(calendar) = reviewed {
        if legacy {
            calendar.plans_legacy(quantization)?
        } else {
            calendar.plans(quantization)?
        }
    } else {
        [
            load_plan(&root.join(format!("{policy}-full")))?,
            load_plan(&root.join(format!("{policy}-diffuse")))?,
        ]
    };
    if unified {
        plans[1] = plans[0].clone();
    }
    let steering = match std::env::var("LIGHTING_TRIAL_STEERING").as_deref() {
        Ok("local") => DspSteering::Local,
        Ok("joint") => DspSteering::Joint,
        Ok("none") => DspSteering::None,
        _ => DspSteering::Orient,
    };
    let options = reviewed.map_or_else(
        || LightingRtlOptions {
            unified_lit: unified,
            dsp_steering: steering,
            id_ring: std::env::var_os("LIGHTING_TRIAL_ID_RING").is_some(),
            ram_retained: std::env::var_os("LIGHTING_TRIAL_RAM_RETAINED").is_some(),
            one_hot_dsp: std::env::var_os("LIGHTING_TRIAL_ONE_HOT").is_some(),
            stationary_logic: std::env::var_os("LIGHTING_STAGE_STATIONARY").is_some(),
            shared_prefix: std::env::var_os("LIGHTING_SHARED_PREFIX").is_some(),
            ..LightingRtlOptions::lit_queue_resource_profile(LightingProfile::Fast, quantization)
        },
        |calendar| {
            if legacy {
                calendar.options_legacy(quantization)
            } else {
                calendar.options(quantization)
            }
        },
    );
    let rtl = rtl::generate_with_schedule_plans(LightingProfile::Fast, options, &plans)?;
    let dir = root.join(format!(
        "{policy}-rtl{}",
        if unified {
            "-unified"
        } else if options.shared_prefix {
            "-prefix"
        } else if options.stationary_logic {
            "-stationary"
        } else {
            ""
        }
    ));
    fs::create_dir_all(&dir)?;
    let mut physical = String::from("full,node,lane,kind\n");
    for i in rtl::physical_calendar_with_schedule_plans(LightingProfile::Fast, options, &plans)? {
        writeln!(
            physical,
            "{},{},{},{}",
            i.full,
            i.event,
            i.lane.map_or_else(String::new, |l| l.to_string()),
            csv(&i.kind)
        )?;
    }
    fs::write(dir.join("physical.csv"), physical)?;
    fs::write(dir.join("behavioral.v"), &rtl.source)?;
    fs::write(
        dir.join("lighting.v"),
        format!("`define GPU_V2_GOWIN_DSP\n{}", rtl.source),
    )?;
    fs::write(dir.join("storage.csv"), &rtl.storage_csv)?;
    fs::write(dir.join("dsp-inputs.csv"), &rtl.dsp_input_csv)?;
    fs::write(
        dir.join("probe.v"),
        include_str!("../src/lighting/rtl/probe.v"),
    )?;
    fs::write(
        dir.join("build.tcl"),
        include_str!("../src/lighting/rtl/probe.tcl")
            .replace("run all", "set_option -place_option 0\nrun all"),
    )?;
    fs::write(dir.join("lighting.sdc"), "create_clock -name clk -period 15.151515 [get_ports {clk}]\nreport_timing -setup -max_paths 20\n")?;
    let structure = format!("full_latency={},diffuse_latency={},full_ii={},diffuse_ii={},register_bits={},mul9={},mul18={},mac={},rom={}\n", rtl.latency, rtl.diffuse_latency, rtl.specular_ii, rtl.diffuse_ii, rtl.register_bits, rtl.small_multipliers, rtl.large_multipliers, rtl.pair_macros, rtl.normalization_roms);
    fs::write(dir.join("structure.txt"), &structure)?;
    print!("{policy}: {structure}");
    let kernel = counted::Config {
        rsqrt_q13: options.rsqrt_q13,
        ..counted::Config::lit_queue_resource_profile(LightingProfile::Fast, quantization)
    };
    let golden = oracle::Config::from_counted(kernel);
    let mut emu = LightingEmu::with_schedule_plans(LightingProfile::Fast, options, &plans, 50000)?;
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
    let mut tb = String::from("module tb;reg clk=0,reset=1,ce=1,context_valid=0,in_valid=0,out_ready=1;reg [15:0] context_epoch=0;reg [1:0] context_mode=0;reg [4:0] context_code=0;reg signed [15:0] light_x=0,light_y=0,light_z=16384,ray_x=8192,ray_y=8192,ray_k=8192;reg [8:0] ambient=64,directional=192;reg [31:0] in_id=0;reg [35:0] in_row0=0,in_row1=0,in_row2=0;wire context_ready,in_ready,out_valid;wire [31:0] out_id;wire [15:0] out_epoch;wire [8:0] out_g,out_h;gpu_v2_lighting dut(.*);initial begin #2;clk=1;#2;clk=0;reset=0;\n");
    let mut tick_count = 0usize;
    let mut checked_outputs = 0usize;
    for mode in 1..=3u8 {
        let context = LightingContext {
            material: Material {
                shininess_code: if mode == 3 { 16 } else { 8 },
                specular_color: if mode == 3 { [255; 3] } else { [0; 3] },
                ..Material::default()
            },
            light: Light {
                ambient: u16::from(mode) * 32,
                directional: if mode == 1 { 0 } else { 192 },
                direction: match mode {
                    2 => [0, 16384, 0],
                    3 => [-16384, 0, 0],
                    _ => [0, 0, 16384],
                },
            },
            projection: Projection::default(),
            epoch: u16::from(mode) + 100,
        };
        let mut loaded = false;
        let mut sent = 0usize;
        let mut pending = BTreeMap::new();
        let mut complete = false;
        for step in 0..4096 {
            let request = (loaded && sent < 96).then(|| {
                let k = sent as i32;
                let boundary_normals = [
                    [0, 0, 0],
                    [-2048, 2047, 0],
                    [2047, -2048, 2047],
                    [1, 0, 0],
                    [-1, 0, 0],
                    [0, 0, 1024],
                    [0, 0, -1024],
                ];
                let codes = boundary_normals.get(sent).copied().unwrap_or([
                    (k * 311) % 4096 - 2048,
                    (k * 197 + 13) % 4096 - 2048,
                    (k * 83 + 41) % 4096 - 2048,
                ]);
                LightingRequest {
                    id: u32::MAX
                        .wrapping_sub(u32::from(mode) * 0x10000001)
                        .wrapping_sub((sent as u32).wrapping_mul(0x23456789)),
                    pixel: PixelInput {
                        normal: codes.map(|v| (v * 16) as i16),
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
            writeln!(
                tb,
                "light_x={};light_y={};light_z={};ray_x={};ray_y={};ray_k={};",
                context.light.direction[0],
                context.light.direction[1],
                context.light.direction[2],
                context.projection.ray_scale[0],
                context.projection.ray_scale[1],
                context.projection.k
            )?;
            writeln!(tb,"ce={};context_valid={};in_valid={};out_ready={};context_epoch={};context_mode={};context_code={};ambient={};directional={};",u8::from(tick.ce),u8::from(tick.context.is_some()),u8::from(tick.input.is_some()),u8::from(tick.output_ready),context.epoch,context.mode(),context.material.shininess_code,context.light.ambient,context.light.directional)?;
            if let Some(r) = request {
                let packed = PixelRows::encode(r.pixel).map_err(|e| format!("{e:?}"))?.0;
                writeln!(
                    tb,
                    "in_id={};in_row0=36'h{:x};in_row1=36'h{:x};in_row2=36'h{:x};",
                    r.id, packed[0], packed[1], packed[2]
                )?;
                if signals.input_ready {
                    let expected = oracle::evaluate(
                        r.pixel,
                        context.material,
                        context.light,
                        context.projection,
                        golden,
                    )
                    .map_err(|e| format!("{e:?}"))?;
                    pending.insert(r.id, [expected.g as u16, expected.h as u16]);
                    sent += 1;
                }
            }
            if signals.context_ready && tick.context.is_some() {
                loaded = true;
            }
            writeln!(tb,"#2;if(context_ready !== 1'b{} || in_ready !== 1'b{} || out_valid !== 1'b{}) $fatal(1,\"handshake tick {tick_count}\");",u8::from(signals.context_ready),u8::from(signals.input_ready),u8::from(signals.output.is_some()))?;
            for (id, full, name, raw) in stages {
                let probe = rtl
                    .stages
                    .iter()
                    .chain(rtl.boundaries.iter())
                    .find(|s| s.name == name && s.full == full)
                    .unwrap();
                let expression = if probe.signal.contains('\'') {
                    probe.signal.clone()
                } else {
                    format!("dut.{}", probe.signal)
                };
                let encoded = raw & ((1_i128 << probe.bits) - 1);
                writeln!(tb,"if({expression} !== {}'h{encoded:x}) $fatal(1,\"stage {name} tick {tick_count} id {id} got=%h expected=%h\",{expression},{}'h{encoded:x});",probe.bits,probe.bits)?;
            }
            if let Some(r) = signals.output {
                assert_eq!(
                    pending[&r.id],
                    [r.output.g, r.output.h],
                    "independent oracle {policy} mode{mode} id{}",
                    r.id
                );
                writeln!(tb,"if(out_id !== 32'd{} || out_epoch !== 16'd{} || out_g !== 9'd{} || out_h !== 9'd{}) $fatal(1,\"result tick {tick_count} got id=%d g=%d h=%d\",out_id,out_g,out_h);",r.id,r.epoch,r.output.g,r.output.h)?;
                if tick.ce && tick.output_ready {
                    pending.remove(&r.id);
                    checked_outputs += 1;
                }
            }
            writeln!(tb, "clk=1;#2;clk=0;#2;")?;
            tick_count += 1;
            if loaded && sent == 96 && pending.is_empty() {
                complete = true;
                break;
            }
        }
        assert!(complete, "bounded candidate stream failed to drain");
    }
    writeln!(
        tb,
        "$display(\"PASS cycles={tick_count} outputs={checked_outputs}\");$finish;end endmodule"
    )?;
    tb = tb.replace(
        "gpu_v2_lighting dut(.*);",
        "gpu_v2_lighting dut(.*);\n`ifdef GPU_V2_GOWIN_DSP\nGSR GSR(.GSRI(1'b1));\n`endif\n",
    );
    fs::write(dir.join("tb.v"), tb)?;
    fs::write(dir.join("numerical.txt"), format!("independent_oracle_outputs={checked_outputs},cycles={tick_count},ce_and_output_stalls=true,modes=1;2;3\n"))?;
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let root = Path::new(
        args.first()
            .map_or("target/lighting-two-cycle-20261006", String::as_str),
    );
    for (policy, quantization) in [
        ("floor", LightingQuantization::CompensatedFloor),
        ("rne", LightingQuantization::NearestEven),
    ] {
        if args.get(1).is_some_and(|s| s == "rtl") {
            trial(root, policy, quantization)?;
            continue;
        }
        for (mode, full) in [("full", true), ("diffuse", false)] {
            let w = Workbench::with_quantization(LightingProfile::Fast, full, quantization)?
                .preconnected_modes();
            let directory = root.join(format!("{policy}-{mode}"));
            if args.get(1).is_some_and(|s| s == "check") {
                print!("{policy}-{mode}: {}", check(&w, &directory)?);
            } else {
                export(&w, &directory)?;
            }
        }
    }
    Ok(())
}
