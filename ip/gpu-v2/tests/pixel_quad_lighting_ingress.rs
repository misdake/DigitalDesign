use gpu_v2::{
    lighting::{ports::*, sim::counted, LightingProfile, LightingQuantization},
    system::pixel::{
        lighting_consumer::ResultRow,
        quad_ingress::{self, Beat, Begin, Descriptor, Row},
        quad_lighting_ingress::{self, Ingress, Step, Tick},
    },
};
use std::{collections::VecDeque, fmt::Write};

#[derive(Clone, Copy)]
struct Edge {
    input: Tick,
    expected: Step,
}

fn context(bank: bool) -> LightingContext {
    LightingContext {
        material: Material {
            shininess_code: if bank { 16 } else { 3 },
            ..Material::default()
        },
        light: Light {
            direction: if bank { [0, 16384, 0] } else { [0, 0, 16384] },
            ..Light::default()
        },
        projection: Projection::default(),
        epoch: if bank { 204 } else { 101 },
    }
}
fn begin(q: usize) -> Begin {
    Begin {
        mask: [15, 1, 0, 6, 8, 3, 10][q % 7],
        draw: q >= 22,
        unlit: q % 5 == 0,
        untextured: q % 3 == 0,
        force_coarsest: q % 4 == 0,
        origin: (q * 137) as u16,
    }
}
fn pixel(q: usize, lane: usize) -> CompactPixelInput {
    CompactPixelInput {
        normal: [
            [0, 0, 0],
            [0, 0, 1024],
            [-2048, 2047, 1023],
            [-300, 731, -41],
        ][(q + lane) % 4],
        ndc: [[-16384, 16384], [0, 0], [16384, -16384], [123, -4901]][lane],
    }
}
fn beat(q: usize, row: u8) -> Beat {
    Beat {
        begin: begin(q),
        basic: if row & 1 == 0 {
            ((q as u32 * 123791) ^ (u32::from(row) * 913)) & 0xffffff
        } else {
            (55000 - q as u32 * 191 - u32::from(row) * 23) & 0xffff
        },
        lighting: pixel(q, usize::from(row / 2)).rows().unwrap()[usize::from(row & 1)],
        uv: ((q as u64 * 17311) ^ (u64::from(row) * 3393)) & ((1u64 << 36) - 1),
    }
}

#[derive(Clone, Copy)]
struct Owner {
    q: usize,
    light_done: bool,
    sample_done: bool,
}

fn healthy() -> Vec<Edge> {
    let mut dut = Ingress::new(10000).unwrap();
    let mut owners = [None::<Owner>; 32];
    let mut published = VecDeque::<u8>::new();
    let mut expected_light = VecDeque::<Row>::new();
    let mut expected_sample = VecDeque::<Row>::new();
    let mut golden = VecDeque::<ResultRow>::new();
    let mut basic = [0u32; 256];
    let mut paid_basic = None;
    let mut next_slot = 0u8;
    let mut q = 0usize;
    let mut row = 0u8;
    let mut allocated_slot = None;
    let mut retired_quads = 0;
    let mut light_stalls = 0;
    let mut held_outputs = 0;
    let mut paid_ce0 = 0;
    let mut context_return = 0;
    let mut high_key = false;
    let mut edges = Vec::new();
    for wall in 0..10000 {
        let ce = wall % 7 != 3 && wall % 29 != 17;
        let rop = published.front().copied().filter(|s| {
            let o = owners[usize::from(*s)].unwrap();
            ce && o.light_done && o.sample_done && wall % 11 != 4
        });
        let basic_read = published
            .front()
            .copied()
            .filter(|_| ce && wall % 5 == 1)
            .map(|s| (s << 3) | ((wall / 5) as u8 & 7));
        let qt = quad_ingress::Tick {
            reset: wall == 0,
            ce,
            input: (q < 44).then(|| beat(q, row)),
            sample_ready: !(180..350).contains(&wall) && wall % 5 != 2,
            rop_row7: rop,
            basic_read,
            ..Default::default()
        };
        let pre = dut.quad_signals(qt);
        let bank = pre.light.is_some_and(|r| r.descriptor.draw);
        let input = Tick {
            quad: qt,
            context: context(bank),
            light_output_ready: !(150..260).contains(&wall) && wall % 13 != 8,
            retire_bank: None,
        };
        let step = dut.tick(input).unwrap();
        assert!(!step.quad.signals.fault && !step.quad.signals.basic_collision);
        assert_eq!(
            step.quad.signals.basic_return, paid_basic,
            "Basic return wall {wall}"
        );
        paid_ce0 += usize::from(!ce && paid_basic.is_some());
        paid_basic = basic_read
            .filter(|_| pre.advance)
            .map(|a| basic[usize::from(a)]);
        if let Some((address, data)) = step.quad.basic_write {
            basic[usize::from(address)] = data;
        }
        if step.quad.accepted {
            if row == 0 && begin(q).mask != 0 {
                assert!(owners[usize::from(next_slot)].is_none());
                owners[usize::from(next_slot)] = Some(Owner {
                    q,
                    light_done: begin(q).unlit,
                    sample_done: begin(q).untextured,
                });
                allocated_slot = Some(next_slot);
                next_slot = (next_slot + 1) & 31;
            }
            if let Some((slot, b)) = step.quad.published {
                assert_eq!(Some(slot), allocated_slot);
                assert_eq!(b, begin(q));
                published.push_back(slot);
                let descriptor = Descriptor {
                    slot,
                    mask: b.mask,
                    draw: b.draw,
                    force_coarsest: false,
                };
                if !b.unlit {
                    for r in 0..8u8 {
                        expected_light.push_back(Row {
                            descriptor,
                            row: r,
                            last: r == 7,
                            data: beat(q, r).lighting,
                        });
                    }
                    for lane in 0..4u8 {
                        if b.mask & (1 << lane) == 0 {
                            continue;
                        }
                        let c = context(b.draw);
                        let value = counted::evaluate_with_config(
                            pixel(q, usize::from(lane)).expanded().unwrap(),
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
                        golden.push_back(ResultRow {
                            key: slot * 4 + lane,
                            quad_last: b.mask >> (lane + 1) == 0,
                            epoch: c.epoch,
                            g: value.g,
                            h: value.h,
                        });
                    }
                }
                if !b.untextured {
                    for r in 0..4u8 {
                        expected_sample.push_back(Row {
                            descriptor: Descriptor {
                                force_coarsest: b.force_coarsest,
                                ..descriptor
                            },
                            row: r,
                            last: r == 3,
                            data: beat(q, r * 2 + 1).uv,
                        });
                    }
                }
            }
            if row == 7 {
                q += 1;
                row = 0;
                allocated_slot = None;
            } else {
                row += 1;
            }
        }
        if step.quad.light_accepted {
            assert_eq!(
                step.quad.signals.light.unwrap(),
                expected_light.pop_front().unwrap()
            );
        }
        if step.quad.sample_accepted {
            let got = step.quad.signals.sample.unwrap();
            assert_eq!(got, expected_sample.pop_front().unwrap());
            if got.last {
                owners[usize::from(got.descriptor.slot)]
                    .as_mut()
                    .unwrap()
                    .sample_done = true;
            }
        }
        if let Some(out) = step.lighting.output {
            if pre.advance && input.light_output_ready {
                assert_eq!(
                    golden.pop_front().unwrap(),
                    out,
                    "Lighting result wall {wall}"
                );
                high_key |= out.key >= 64;
                context_return += usize::from(step.lighting.context_wait);
                let o = owners[usize::from(out.key / 4)].as_mut().unwrap();
                assert_eq!(out.epoch, context(begin(o.q).draw).epoch);
                if out.quad_last {
                    assert!(!o.light_done);
                    o.light_done = true;
                }
            } else {
                held_outputs += 1;
            }
        }
        if let Some(s) = rop {
            assert_eq!(published.pop_front(), Some(s));
            owners[usize::from(s)] = None;
            retired_quads += 1;
        }
        light_stalls += usize::from(ce && q < 44 && !pre.input_ready && pre.light_used == 2);
        edges.push(Edge {
            input,
            expected: step,
        });
        if q == 44
            && expected_light.is_empty()
            && expected_sample.is_empty()
            && golden.is_empty()
            && published.is_empty()
            && dut.light_in_flight() == 0
        {
            assert!(
                high_key
                    && light_stalls > 0
                    && held_outputs > 0
                    && paid_ce0 > 0
                    && context_return > 0
            );
            assert_eq!(retired_quads, 38);
            println!("healthy edges={} live_quads={retired_quads} light_full_stalls={light_stalls} held_output={held_outputs} paid_ce0={paid_ce0} switch_returns={context_return}",edges.len());
            return edges;
        }
    }
    panic!("bounded Quad/Lighting drain");
}

fn fault() -> Vec<Edge> {
    let mut dut = Ingress::new(10000).unwrap();
    let mut row = 0u8;
    let mut submitted = false;
    let mut faulted = false;
    let mut held = None;
    let mut tail = 0;
    let mut edges = Vec::new();
    for wall in 0..1000 {
        let inject = held.is_some() && !faulted;
        let qt = quad_ingress::Tick {
            reset: wall == 0,
            ce: true,
            input: (!submitted).then(|| Beat {
                begin: Begin {
                    mask: 15,
                    ..Default::default()
                },
                ..beat(1, row)
            }),
            sample_ready: true,
            rop_row7: inject.then_some(31),
            ..Default::default()
        };
        let input = Tick {
            quad: qt,
            context: context(false),
            light_output_ready: inject || faulted,
            retire_bank: None,
        };
        let step = dut.tick(input).unwrap();
        if step.quad.accepted {
            if row == 7 {
                submitted = true;
            } else {
                row += 1;
            }
        }
        if !faulted {
            held = step.lighting.output;
        }
        if inject {
            faulted = true;
            assert!(!step.quad.signals.advance);
            assert_eq!(step.lighting.output, held);
        }
        if faulted {
            assert!(
                !step.lighting.pixel_accepted
                    && !step.quad.light_accepted
                    && !step.quad.sample_accepted
            );
            assert_eq!(step.lighting.output, held);
            tail += 1;
        }
        edges.push(Edge {
            input,
            expected: step,
        });
        if tail == 8 {
            return edges;
        }
    }
    panic!("bounded malformed-edge reproduction");
}

#[test]
fn real_quad_rows_feed_lighting_without_descriptor_or_pixel_shadow() {
    healthy();
    fault();
}

fn testbench(edges: &[Edge]) -> String {
    let mut tb = String::from(
        r#"module tb;
reg clk=0,reset=0,ce=0,in_valid=0,sample_ready=0,basic_read=0,rop_row7_accepted=0,retire_valid=0,retire_bank=0,out_ready=0;
reg [22:0] in_begin=0;reg [31:0] in_basic=0;reg [35:0] in_light=0,in_uv=0;
reg [7:0] basic_read_address=0;reg [4:0] rop_slot=0;
reg [15:0] context_epoch=0;reg [1:0] context_mode=0;reg [4:0] context_code=0;
reg signed [15:0] light_x=0,light_y=0,light_z=0,ray_x=0,ray_y=0,ray_k=0;
reg [8:0] ambient=0,directional=0;
wire in_ready,advance,published,basic_write,basic_return_valid,basic_collision,sample_valid,sample_last,fault,light_bank;
wire [2:0] in_row;wire [4:0] published_slot,retire_slot;wire [22:0] published_begin;
wire [7:0] basic_write_address;wire [23:0] basic_write_data,basic_return_data;
wire [35:0] sample_data;wire [1:0] sample_row,light_used;wire [10:0] sample_descriptor;
wire [5:0] global_used;wire [3:0] sample_used;
wire context_wait,context_accepted,pixel_accepted,out_valid,out_quad_last;
wire [6:0] out_key;wire [15:0] out_epoch;wire [8:0] out_g,out_h;
gpu_v2_quad_lighting_ingress dut(.*);
`ifdef GPU_V2_GOWIN_DSP
GSR GSR(.GSRI(1'b1));
`endif
initial begin #200000;$fatal(1,"bounded timeout");end
initial begin reset=1;#2;clk=1;#2;clk=0;#2;
"#,
    );
    for (i, e) in edges.iter().enumerate() {
        let t = e.input;
        let q = t.quad;
        let b = q.input.unwrap_or_default();
        let c = t.context;
        writeln!(tb,"reset={};ce={};in_valid={};in_begin=23'h{:x};in_basic=32'h{:x};in_light=36'h{:x};in_uv=36'h{:x};sample_ready={};basic_read={};basic_read_address=8'd{};rop_row7_accepted={};rop_slot=5'd{};retire_valid={};retire_bank={};out_ready={};context_epoch=16'd{};context_mode=2'd{};context_code=5'd{};light_x=16'h{:x};light_y=16'h{:x};light_z=16'h{:x};ambient=9'd{};directional=9'd{};ray_x=16'h{:x};ray_y=16'h{:x};ray_k=16'h{:x};#2;",
            u8::from(q.reset),u8::from(q.ce),u8::from(q.input.is_some()),b.begin.packed(),b.basic,b.lighting,b.uv,u8::from(q.sample_ready),u8::from(q.basic_read.is_some()),q.basic_read.unwrap_or(0),u8::from(q.rop_row7.is_some()),q.rop_row7.unwrap_or(0),u8::from(t.retire_bank.is_some()),t.retire_bank.unwrap_or(0),u8::from(t.light_output_ready),c.epoch,c.mode(),c.material.shininess_code,c.light.direction[0]as u16,c.light.direction[1]as u16,c.light.direction[2]as u16,c.light.ambient,c.light.directional,c.projection.ray_scale[0]as u16,c.projection.ray_scale[1]as u16,c.projection.k as u16).unwrap();
        let s = e.expected.quad.signals;
        let l = e.expected.lighting;
        writeln!(tb,"if(in_ready!==1'b{}||advance!==1'b{}||in_row!==3'd{}||global_used!==6'd{}||light_used!==2'd{}||sample_used!==4'd{}||retire_slot!==5'd{}||fault!==1'b{}||basic_collision!==1'b{}) $fatal(1,\"Quad control edge {i}\");",u8::from(s.input_ready),u8::from(s.advance),s.input_row,s.global_used,s.light_used,s.sample_used,s.retire_slot,u8::from(s.fault),u8::from(s.basic_collision)).unwrap();
        writeln!(tb,"if(published!==1'b{}||basic_write!==1'b{}||basic_return_valid!==1'b{}||sample_valid!==1'b{}||dut.light_valid!==1'b{}||context_wait!==1'b{}||context_accepted!==1'b{}||pixel_accepted!==1'b{}||out_valid!==1'b{}) $fatal(1,\"Transfer edge {i}\");",u8::from(e.expected.quad.published.is_some()),u8::from(e.expected.quad.basic_write.is_some()),u8::from(s.basic_return.is_some()),u8::from(s.sample.is_some()),u8::from(s.light.is_some()),u8::from(l.context_wait),u8::from(l.context_accepted),u8::from(l.pixel_accepted),u8::from(l.output.is_some())).unwrap();
        if let Some((slot, b)) = e.expected.quad.published {
            writeln!(tb,"if(published_slot!==5'd{slot}||published_begin!==23'h{:x})$fatal(1,\"Publish edge {i}\");",b.packed()).unwrap();
        }
        if let Some((a, d)) = e.expected.quad.basic_write {
            writeln!(tb,"if(basic_write_address!==8'd{a}||basic_write_data!==24'h{d:x})$fatal(1,\"Basic write {i}\");").unwrap();
        }
        if let Some(d) = s.basic_return {
            writeln!(
                tb,
                "if(basic_return_data!==24'h{d:x})$fatal(1,\"Basic return {i}\");"
            )
            .unwrap();
        }
        if let Some(r) = s.sample {
            writeln!(tb,"if(sample_data!==36'h{:x}||sample_row!==2'd{}||sample_last!==1'b{}||sample_descriptor!==11'h{:x})$fatal(1,\"Sample edge {i}\");",r.data,r.row,u8::from(r.last),r.descriptor.packed()).unwrap();
        }
        if let Some(r) = s.light {
            writeln!(tb,"if(dut.light_data!==36'h{:x}||dut.light_row!==3'd{}||dut.light_descriptor!==10'h{:x})$fatal(1,\"Light edge {i}\");",r.data,r.row,r.descriptor.packed()).unwrap();
        }
        if let Some(o) = l.output {
            writeln!(tb,"if(out_key!==7'd{}||out_quad_last!==1'b{}||out_epoch!==16'd{}||out_g!==9'd{}||out_h!==9'd{})$fatal(1,\"Lighting output edge {i}\");",o.key,u8::from(o.quad_last),o.epoch,o.g,o.h).unwrap();
        }
        tb.push_str("clk=1;#2;clk=0;#2;\n");
    }
    writeln!(
        tb,
        "$display(\"PASS Quad Lighting edges={}\");$finish;end endmodule",
        edges.len()
    )
    .unwrap();
    tb
}

#[test]
#[ignore = "requires Icarus and actual Gowin DSP/RAM primitives"]
fn actual_composed_rtl_matches_real_queue_and_numeric_lifetimes() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/pixel-quad-lighting-ingress-20261010");
    std::fs::create_dir_all(&dir).unwrap();
    for (profile, edges) in [("healthy", healthy()), ("fault", fault())] {
        std::fs::write(dir.join("tb.v"), testbench(&edges)).unwrap();
        for native in [false, true] {
            let backend = if native {
                quad_ingress::rtl::Backend::Native
            } else {
                quad_ingress::rtl::Backend::Portable
            };
            std::fs::write(
                dir.join("dut.v"),
                quad_lighting_ingress::verilog(backend).unwrap(),
            )
            .unwrap();
            let mut compile = std::process::Command::new(
                std::env::var_os("IVERILOG_EXE").unwrap_or("iverilog".into()),
            );
            compile
                .current_dir(&dir)
                .args(["-g2012", "-s", "tb", "-o", "sim.vvp"]);
            if native {
                compile.arg("-DGPU_V2_GOWIN_DSP").arg(
                    std::path::PathBuf::from(std::env::var_os("GOWIN_HOME").expect("GOWIN_HOME"))
                        .join("IDE/simlib/gw2a/prim_sim.v"),
                );
            }
            let result = compile.args(["dut.v", "tb.v"]).output().unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            let ran =
                std::process::Command::new(std::env::var_os("VVP_EXE").unwrap_or("vvp".into()))
                    .current_dir(&dir)
                    .arg("sim.vvp")
                    .output()
                    .unwrap();
            assert!(
                ran.status.success(),
                "{profile} native={native}: {} {}",
                String::from_utf8_lossy(&ran.stdout),
                String::from_utf8_lossy(&ran.stderr)
            );
            println!(
                "{profile} native={native}: {}",
                String::from_utf8_lossy(&ran.stdout)
            );
            std::fs::copy(
                dir.join("dut.v"),
                dir.join(format!("{profile}-native-{native}.v")),
            )
            .unwrap();
            std::fs::copy(dir.join("tb.v"), dir.join(format!("{profile}-tb.v"))).unwrap();
        }
    }
}
