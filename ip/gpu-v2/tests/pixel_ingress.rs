//! Real single-Quad parallel Lighting/UV ingress, excluding full Runtime/join.
use gpu_v2::{
    lighting::{ports::*, sim::counted, LightingProfile, LightingQuantization},
    system::pixel::{
        lighting_consumer::ResultRow, pixel_ingress as combined, quad_ingress as quad,
        quad_sampling_ingress as sample,
    },
    texture::emu::{context_store::Raw, derivative::Header, uv_store::Coordinate},
};
use std::{collections::VecDeque, fmt::Write};
const CONFIG: [u32; 2] = [
    0x0100 | (3 << 16) | (9 << 20) | (1 << 24) | (2 << 25),
    0xff00 | (11 << 16) | (5 << 20) | (1 << 25),
];
fn ctx(bank: bool) -> LightingContext {
    LightingContext {
        epoch: if bank { 202 } else { 101 },
        material: Material {
            shininess_code: if bank { 16 } else { 3 },
            ..Material::default()
        },
        light: Light {
            direction: if bank { [0, 16384, 0] } else { [0, 0, 16384] },
            ..Light::default()
        },
        projection: Projection::default(),
    }
}
fn pix(q: usize, lane: usize) -> CompactPixelInput {
    CompactPixelInput {
        normal: [
            [0, 0, 0],
            [0, 0, 1024],
            [-2048, 2047, 1023],
            [-419, 31, 1001],
        ][(q + lane) % 4],
        ndc: [[-16384, 16384], [0, 0], [16384, -16384], [731, -9001]][lane],
    }
}
fn uv(q: usize) -> [[i64; 2]; 4] {
    let a = [-131072, -65537, -1, 0, 65535, 65536, 131071];
    std::array::from_fn(|lane| [a[(q + lane * 3) % 7], a[(q * 3 + lane + 1) % 7]])
}
fn word(p: [i64; 2]) -> u64 {
    (p[0] as u64 & 0x3ffff) | ((p[1] as u64 & 0x3ffff) << 18)
}
fn beat(q: usize, row: usize) -> quad::Beat {
    quad::Beat {
        begin: quad::Begin {
            mask: if q % 9 == 2 { 0 } else { [1, 6, 15, 8][q % 4] },
            draw: q >= 18,
            unlit: q % 5 == 0,
            untextured: q % 7 == 2,
            force_coarsest: q % 4 == 0,
            origin: (q * 173) as u16,
        },
        basic: if row % 2 == 0 {
            ((q * 119731 + row * 713) & 0xffffff) as u32
        } else {
            (55000 - q * 191 - row * 23) as u32
        },
        lighting: pix(q, row / 2).rows().unwrap()[row % 2],
        uv: word(uv(q)[row / 2]),
    }
}
#[derive(Clone, Copy)]
struct Global {
    slot: u8,
    q: usize,
    light_done: bool,
    sample_done: bool,
}
#[derive(Clone, Copy)]
struct Lease {
    slot: u8,
    q: usize,
}
#[derive(Clone, Copy)]
struct Vector {
    input: combined::Tick,
    expected: combined::Step,
}
fn trace() -> Vec<Vector> {
    let mut dut = combined::Ingress::new(20000).unwrap();
    let mut globals = VecDeque::<Global>::new();
    let mut sample_rows = VecDeque::<(quad::Row, usize)>::new();
    let mut light_rows = VecDeque::<quad::Row>::new();
    let mut gold = VecDeque::<ResultRow>::new();
    let mut leases = [None::<Lease>; 8];
    let mut jobs = VecDeque::<Coordinate>::new();
    let mut remaining = [0usize; 8];
    let mut next_context = 0usize;
    let mut q = 0usize;
    let mut row = 0usize;
    let mut allocated = 0usize;
    let mut phase = 0u8;
    let mut previous_read = false;
    let mut vectors = vec![];
    let mut d_count = 0;
    let mut c_count = 0;
    let mut light_count = 0;
    let mut high_key = false;
    let mut ce0_return = 0;
    let mut mixed_access = 0;
    let mut simultaneous_branches = 0;
    for wall in 0..20000 {
        let ce = wall % 7 != 3 && !previous_read;
        let mut s = sample::Tick {
            quad: quad::Tick {
                reset: wall == 0,
                ce,
                input: (q < 36).then(|| beat(q, row)),
                rop_row7: (ce
                    && globals
                        .front()
                        .is_some_and(|g| g.light_done && g.sample_done))
                .then(|| globals.front().unwrap().slot),
                ..Default::default()
            },
            phase,
            context_credit: wall > 70 && leases[next_context % 8].is_none(),
            context_slot: next_context % 8,
            config: CONFIG[0],
            ..Default::default()
        };
        let preview = dut.signals(s);
        if let Some(head) = preview.quad.sample {
            s.config = CONFIG[usize::from(head.descriptor.draw)];
        }
        if ce {
            s.take_derivative =
                phase == 0 && preview.derivative.is_some() && !(180..260).contains(&wall);
            s.take_coordinate = phase % 2 == 0 && preview.coordinate.is_some() && wall % 11 != 4;
            if phase % 2 != 0 && preview.coordinate_empty && wall > 130 {
                s.coordinate = jobs.front().copied();
            }
        }
        let input = combined::Tick {
            sampling: s,
            lighting_context: ctx(preview.quad.light.is_some_and(|h| h.descriptor.draw)),
            lighting_output_ready: !(140..230).contains(&wall) && wall % 13 != 8,
            retire_bank: None,
        };
        let step = dut.tick(input).unwrap();
        let signals = step.sampling.signals;
        assert!(!signals.quad.fault && !signals.quad.basic_collision);
        if let Some(head) = signals.quad.light {
            assert_eq!(Some(&head), light_rows.front());
        }
        if let Some(head) = signals.quad.sample {
            assert_eq!(Some(&head), sample_rows.front().map(|p| &p.0));
        }
        if step.sampling.quad.accepted {
            if row == 0 && beat(q, 0).begin.mask != 0 {
                allocated += 1;
            }
            if row == 7 {
                let b = beat(q, 0).begin;
                if b.mask != 0 {
                    let slot = ((allocated - 1) % 32) as u8;
                    assert_eq!(step.sampling.quad.published, Some((slot, b)));
                    globals.push_back(Global {
                        slot,
                        q,
                        light_done: b.unlit,
                        sample_done: b.untextured,
                    });
                    let desc = quad::Descriptor {
                        slot,
                        mask: b.mask,
                        draw: b.draw,
                        force_coarsest: false,
                    };
                    if !b.unlit {
                        light_rows.extend((0..8).map(|r| quad::Row {
                            descriptor: desc,
                            row: r,
                            last: r == 7,
                            data: beat(q, usize::from(r)).lighting,
                        }));
                        for lane in 0..4u8 {
                            if b.mask & (1 << lane) != 0 {
                                let c = ctx(b.draw);
                                let result = counted::evaluate_with_config(
                                    pix(q, usize::from(lane)).expanded().unwrap(),
                                    c.material,
                                    c.light,
                                    c.projection,
                                    2048,
                                    counted::Config {
                                        rsqrt_q13: true,
                                        ..counted::Config::lit_queue_resource_profile(
                                            LightingProfile::Fast,
                                            LightingQuantization::CompensatedFloor,
                                        )
                                    },
                                )
                                .unwrap()
                                .output;
                                gold.push_back(ResultRow {
                                    key: slot * 4 + lane,
                                    quad_last: b.mask >> (lane + 1) == 0,
                                    epoch: c.epoch,
                                    g: result.g,
                                    h: result.h,
                                });
                            }
                        }
                    }
                    if !b.untextured {
                        sample_rows.extend((0..4).map(|r| {
                            (
                                quad::Row {
                                    descriptor: quad::Descriptor {
                                        force_coarsest: b.force_coarsest,
                                        ..desc
                                    },
                                    row: r,
                                    last: r == 3,
                                    data: word(uv(q)[usize::from(r)]),
                                },
                                q,
                            )
                        }));
                    }
                }
                q += 1;
                row = 0;
            } else {
                row += 1;
            }
        }
        if step.sampling.quad.light_accepted {
            light_rows.pop_front().unwrap();
        }
        if step.sampling.quad.sample_accepted {
            let (head, idx) = sample_rows.pop_front().unwrap();
            let slot = next_context % 8;
            if head.row == 0 {
                let cfg = CONFIG[usize::from(head.descriptor.draw)];
                let raw = Raw {
                    bias: cfg as u16 as i16,
                    force_coarsest: head.descriptor.force_coarsest,
                    header: Header {
                        quad: head.descriptor.slot & 15,
                        mask: head.descriptor.mask,
                        slot: ((cfg >> 16) & 15) as u8,
                        max_n: ((cfg >> 20) & 15) as u8,
                        has_mip: cfg >> 24 & 1 != 0,
                        filter: ((cfg >> 25) & 3) as u8,
                    },
                }
                .pack()
                .unwrap();
                assert_eq!(step.sampling.uv.raw_write, Some((slot, raw)));
                assert!(leases[slot].is_none());
                leases[slot] = Some(Lease {
                    slot: head.descriptor.slot,
                    q: idx,
                });
            }
            assert_eq!(
                step.sampling.uv.write,
                Some((
                    slot * 4 + usize::from(head.row),
                    word(uv(idx)[usize::from(head.row)])
                ))
            );
            if head.last {
                assert_eq!(step.sampling.uv.committed, Some(slot));
                next_context += 1;
            }
        }
        if s.take_derivative && signals.quad.advance {
            let (slot, data) = signals.derivative.unwrap();
            let lease = leases[slot].unwrap();
            assert_eq!(data, std::array::from_fn(|i| uv(lease.q)[i / 2][i % 2]));
            jobs.extend((0..4).map(|lane| Coordinate { slot, lane }));
            remaining[slot] = 4;
            d_count += 1;
        }
        if s.coordinate.is_some() && signals.quad.advance {
            assert_eq!(s.coordinate, jobs.pop_front());
        }
        if let Some((owner, data)) = signals.coordinate {
            assert_eq!(
                data,
                uv(leases[owner.slot].unwrap().q)[owner.lane].map(|v| v as u32 & 0xffff)
            );
            if s.take_coordinate && signals.quad.advance {
                remaining[owner.slot] -= 1;
                c_count += 1;
                if remaining[owner.slot] == 0 {
                    let lease = leases[owner.slot].take().unwrap();
                    globals
                        .iter_mut()
                        .find(|g| g.slot == lease.slot)
                        .unwrap()
                        .sample_done = true;
                }
            }
        }
        if let Some(out) = step.lighting.output {
            if signals.quad.advance && input.lighting_output_ready {
                assert_eq!(Some(out), gold.pop_front());
                high_key |= out.key >= 64;
                light_count += 1;
                let global = globals.iter_mut().find(|g| g.slot == out.key / 4).unwrap();
                assert_eq!(out.epoch, ctx(beat(global.q, 0).begin.draw).epoch);
                if out.quad_last {
                    assert!(!global.light_done);
                    global.light_done = true;
                }
            }
        }
        if s.quad.rop_row7.is_some() && signals.quad.advance {
            assert_eq!(globals.pop_front().unwrap().slot, s.quad.rop_row7.unwrap());
        }
        ce0_return += usize::from(!signals.quad.advance && step.sampling.uv.returned.is_some());
        mixed_access +=
            usize::from(step.sampling.uv.read.is_some() && step.sampling.uv.write.is_some());
        simultaneous_branches +=
            usize::from(step.lighting.pixel_accepted && step.sampling.quad.sample_accepted);
        previous_read = step.sampling.uv.read.is_some();
        if signals.quad.advance {
            phase = (phase + 1) & 7;
        }
        vectors.push(Vector {
            input,
            expected: step,
        });
        if q == 36
            && globals.is_empty()
            && leases.iter().all(Option::is_none)
            && dut.light_in_flight() == 0
            && light_rows.is_empty()
            && sample_rows.is_empty()
            && gold.is_empty()
        {
            assert!(high_key && ce0_return > 0 && mixed_access > 0 && simultaneous_branches > 0);
            assert_eq!(c_count, d_count * 4);
            println!("joint edges={} D={d_count} C={c_count} Light={light_count} CE0returns={ce0_return} parallel={simultaneous_branches} mixedRAM={mixed_access}",vectors.len());
            return vectors;
        }
    }
    panic!("bounded joint ingress drain");
}
#[test]
fn single_quad_drives_both_actual_branches() {
    trace();
}

/// Declare a TB from the real public port header, including multiple same-type
/// names. This copies port declarations, never device state or numerical logic.
fn tb(vectors: &[Vector]) -> String {
    let mut text = String::from("module tb;\n");
    let mut initial = String::new();
    let mut prefix = String::new();
    let header = combined::wrapper_verilog()
        .split_once("module gpu_v2_pixel_ingress(")
        .unwrap()
        .1
        .split_once(");")
        .unwrap()
        .0;
    for field in header.split(',') {
        let field = field.trim();
        let name;
        if let Some(rest) = field.strip_prefix("input wire ") {
            let (kind, n) = rest.rsplit_once(' ').unwrap_or(("", rest));
            prefix = format!("reg {kind}");
            name = n;
            writeln!(initial, "{name}=0;").unwrap();
        } else if let Some(rest) = field.strip_prefix("output wire ") {
            let (kind, n) = rest.rsplit_once(' ').unwrap_or(("", rest));
            prefix = format!("wire {kind}");
            name = n;
        } else {
            name = field;
            if prefix.starts_with("reg ") {
                writeln!(initial, "{name}=0;").unwrap();
            }
        }
        writeln!(text, "{prefix} {name};").unwrap();
    }
    text.push_str("gpu_v2_pixel_ingress dut(.*);\n`ifdef GPU_V2_GOWIN_DSP\nGSR GSR(.GSRI(1'b1));\n`endif\ninitial begin #200000;$fatal(1,\"bounded timeout\");end\ninitial begin\n");
    text.push_str(&initial);
    text.push_str("reset=1;#2;clk=1;#2;clk=0;#2;\n");
    for (i, v) in vectors.iter().enumerate() {
        let t = v.input;
        let s = t.sampling;
        let q = s.quad;
        let b = q.input.unwrap_or_default();
        let c = t.lighting_context;
        writeln!(text,"reset={};ce={};in_valid={};in_begin=23'h{:x};in_basic=32'h{:x};in_light=36'h{:x};in_uv=36'h{:x};basic_read={};basic_read_address=8'd{};rop_row7_accepted={};rop_slot=5'd{};phase=3'd{};context_credit={};context_slot=3'd{};draw_config=27'h{:x};d_take={};c_request={};c_slot=3'd{};c_lane=2'd{};c_take={};retire_valid=0;out_ready={};context_epoch=16'd{};context_mode=2'd{};context_code=5'd{};light_x=16'h{:x};light_y=16'h{:x};light_z=16'h{:x};ambient=9'd{};directional=9'd{};ray_x=16'h{:x};ray_y=16'h{:x};ray_k=16'h{:x};#2;",
            u8::from(q.reset),u8::from(q.ce),u8::from(q.input.is_some()),b.begin.packed(),b.basic,b.lighting,b.uv,u8::from(q.basic_read.is_some()),q.basic_read.unwrap_or(0),u8::from(q.rop_row7.is_some()),q.rop_row7.unwrap_or(0),s.phase,u8::from(s.context_credit),s.context_slot,s.config,u8::from(s.take_derivative),u8::from(s.coordinate.is_some()),s.coordinate.map_or(0,|o|o.slot),s.coordinate.map_or(0,|o|o.lane),u8::from(s.take_coordinate),u8::from(t.lighting_output_ready),c.epoch,c.mode(),c.material.shininess_code,c.light.direction[0]as u16,c.light.direction[1]as u16,c.light.direction[2]as u16,c.light.ambient,c.light.directional,c.projection.ray_scale[0]as u16,c.projection.ray_scale[1]as u16,c.projection.k as u16).unwrap();
        let e = v.expected;
        let qs = e.sampling.quad.signals;
        let u = e.sampling.uv;
        let ls = e.lighting;
        let mut check = |name: &str, bits: usize, value: u64| {
            writeln!(
                text,
                "if({name}!=={bits}'h{value:x})$fatal(1,\"{name} edge {i} got %h\",{name});"
            )
            .unwrap();
        };
        check("in_ready", 1, u64::from(qs.input_ready));
        check("advance", 1, u64::from(qs.advance));
        check("in_row", 3, u64::from(qs.input_row));
        check("global_used", 6, u64::from(qs.global_used));
        check("light_used", 2, u64::from(qs.light_used));
        check("sample_used", 4, u64::from(qs.sample_used));
        check("fault", 1, u64::from(qs.fault));
        check(
            "published",
            1,
            u64::from(e.sampling.quad.published.is_some()),
        );
        if let Some((slot, b)) = e.sampling.quad.published {
            check("published_slot", 5, u64::from(slot));
            check("published_begin", 23, u64::from(b.packed()));
        }
        check(
            "basic_write",
            1,
            u64::from(e.sampling.quad.basic_write.is_some()),
        );
        if let Some((a, d)) = e.sampling.quad.basic_write {
            check("basic_write_address", 8, u64::from(a));
            check("basic_write_data", 24, u64::from(d));
        }
        check("sample_valid", 1, u64::from(qs.sample.is_some()));
        if let Some(h) = qs.sample {
            check("sample_data", 36, h.data);
            check("sample_row", 2, u64::from(h.row));
            check("sample_descriptor", 11, u64::from(h.descriptor.packed()));
        }
        check("context_wait", 1, u64::from(ls.context_wait));
        check("context_accepted", 1, u64::from(ls.context_accepted));
        check("pixel_accepted", 1, u64::from(ls.pixel_accepted));
        check("out_valid", 1, u64::from(ls.output.is_some()));
        if let Some(o) = ls.output {
            check("out_key", 7, u64::from(o.key));
            check("out_quad_last", 1, u64::from(o.quad_last));
            check("out_epoch", 16, u64::from(o.epoch));
            check("out_g", 9, u64::from(o.g));
            check("out_h", 9, u64::from(o.h));
        }
        check("raw_write", 1, u64::from(u.raw_write.is_some()));
        if let Some((slot, data)) = u.raw_write {
            check("raw_slot", 3, slot as u64);
            check("raw_metadata", 36, data);
        }
        check("committed", 1, u64::from(u.committed.is_some()));
        if let Some(slot) = u.committed {
            check("committed_slot", 3, slot as u64);
        }
        check("uv_write", 1, u64::from(u.write.is_some()));
        if let Some((a, d)) = u.write {
            check("uv_write_address", 5, a as u64);
            check("uv_write_data", 36, d);
        }
        check("uv_read", 1, u64::from(u.read.is_some()));
        if let Some(read) = u.read {
            check("uv_read_address", 5, read.address as u64);
        }
        check(
            "d_valid",
            1,
            u64::from(e.sampling.signals.derivative.is_some()),
        );
        if let Some((slot, _)) = e.sampling.signals.derivative {
            check("d_slot", 3, slot as u64);
        }
        check(
            "c_valid",
            1,
            u64::from(e.sampling.signals.coordinate.is_some()),
        );
        if let Some((owner, data)) = e.sampling.signals.coordinate {
            check("c_owner_slot", 3, owner.slot as u64);
            check("c_owner_lane", 2, owner.lane as u64);
            check("c_uv", 32, u64::from(data[0]) | (u64::from(data[1]) << 16));
        }
        if let Some((_, words)) = e.sampling.signals.derivative {
            let packed: Vec<_> = words.chunks_exact(2).map(|w| word([w[0], w[1]])).collect();
            writeln!(
                text,
                "if(d_uv!==144'h{:09x}{:09x}{:09x}{:09x})$fatal(1,\"D payload edge {i}\");",
                packed[3], packed[2], packed[1], packed[0]
            )
            .unwrap();
        }
        text.push_str("clk=1;#2;clk=0;#2;\n");
    }
    writeln!(
        text,
        "$display(\"PASS single Quad parallel ingress edges={}\");$finish;end endmodule",
        vectors.len()
    )
    .unwrap();
    text
}
#[test]
#[ignore = "requires Icarus and actual Gowin DSP/RAM primitives"]
fn actual_single_quad_both_branches_match_each_edge() {
    let vectors = trace();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/pixel-ingress-20261010");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("tb.v"), tb(&vectors)).unwrap();
    for native in [false, true] {
        let backend = if native {
            quad::rtl::Backend::Native
        } else {
            quad::rtl::Backend::Portable
        };
        std::fs::write(dir.join("dut.v"), combined::verilog(backend).unwrap()).unwrap();
        let mut command = std::process::Command::new(
            std::env::var_os("IVERILOG_EXE").unwrap_or("iverilog".into()),
        );
        command
            .current_dir(&dir)
            .args(["-g2012", "-s", "tb", "-o", "sim.vvp"]);
        if native {
            command.arg("-DGPU_V2_GOWIN_DSP").arg(
                std::path::PathBuf::from(std::env::var_os("GOWIN_HOME").expect("GOWIN_HOME"))
                    .join("IDE/simlib/gw2a/prim_sim.v"),
            );
        }
        let compile = command.args(["dut.v", "tb.v"]).output().unwrap();
        assert!(
            compile.status.success(),
            "{}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let ran = std::process::Command::new(std::env::var_os("VVP_EXE").unwrap_or("vvp".into()))
            .current_dir(&dir)
            .arg("sim.vvp")
            .output()
            .unwrap();
        assert!(
            ran.status.success(),
            "native={native}: {} {}",
            String::from_utf8_lossy(&ran.stdout),
            String::from_utf8_lossy(&ran.stderr)
        );
        println!("native={native}: {}", String::from_utf8_lossy(&ran.stdout));
        std::fs::copy(dir.join("dut.v"), dir.join(format!("native-{native}.v"))).unwrap();
    }
}
