use gpu_v2::{
    lighting::{ports::*, sim::counted, LightingProfile, LightingQuantization},
    system::pixel::lighting_consumer::{self, Consumer, Signals, Source, Tick},
};
use std::{collections::VecDeque, fmt::Write};

fn context(epoch: u16) -> LightingContext {
    LightingContext {
        epoch,
        material: Material {
            specular_color: if epoch == 11 { [0; 3] } else { [255; 3] },
            shininess_code: if epoch == 13 { 16 } else { 8 },
            ..Material::default()
        },
        light: Light {
            ambient: if epoch == 13 { 256 } else { 32 },
            directional: if epoch == 13 { 0 } else { 224 },
            ..Light::default()
        },
        projection: Projection::default(),
    }
}

fn pixel(i: usize, lane: usize) -> CompactPixelInput {
    let normal = [
        [0, 0, 0],
        [0, 0, 1024],
        [-2048, 2047, 1023],
        [-719, 41, 909],
        [1, -1, -1024],
        [2047, 3, -2048],
    ][(i + lane) % 6];
    let ndc = [[-16384, 16384], [0, 0], [16384, -16384], [719, -4001]][lane];
    CompactPixelInput { normal, ndc }
}

#[derive(Clone, Copy)]
struct Edge {
    input: Tick,
    expected: Signals,
}

/// Bounded driver has a held row head and post-hoc independent counted results.
/// Four epochs include two simultaneously live draw banks and same-bank reuse.
fn trace() -> Vec<Edge> {
    let mut dut = Consumer::new(10000).unwrap();
    let mut rows = VecDeque::new();
    let mut golden = VecDeque::new();
    for (batch, quads) in [16usize, 16, 4, 2].into_iter().enumerate() {
        let ctx = context(10 + batch as u16);
        for q in 0..quads {
            let mask = if batch < 2 {
                q as u8
            } else {
                [15, 1, 8, 6][q % 4]
            };
            for lane in 0..4 {
                let pix = pixel(q + batch * 11, lane);
                let packed = pix.rows().unwrap();
                for (part, data) in packed.into_iter().enumerate() {
                    rows.push_back((
                        batch,
                        Source {
                            row: (lane * 2 + part) as u8,
                            data,
                            slot: (16 + q) as u8,
                            mask,
                            draw_bank: if batch == 3 { 0 } else { (batch % 2) as u8 },
                        },
                        ctx,
                    ));
                }
                if mask & (1 << lane) != 0 {
                    let value = counted::evaluate_with_config(
                        pix.expanded().unwrap(),
                        ctx.material,
                        ctx.light,
                        ctx.projection,
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
                    golden.push_back(lighting_consumer::ResultRow {
                        key: (16 + q as u8) * 4 + lane as u8,
                        quad_last: mask >> (lane + 1) == 0,
                        epoch: ctx.epoch,
                        g: value.g,
                        h: value.h,
                    });
                }
            }
        }
    }
    let mut edges = Vec::new();
    let mut returned = [0usize; 4];
    let totals = [32usize, 32, 8, 5];
    let mut retired = [false; 3];
    let mut context_loads = 0;
    let mut simultaneous_retire_load = 0;
    let mut wait_with_return = 0;
    let mut held_output = 0;
    for wall in 0..10000 {
        let ce = wall % 7 != 3 && wall % 31 != 17;
        let reused_bank_available = retired[2];
        let mut retire_bank = None;
        for batch in 0..3 {
            if returned[batch] == totals[batch] && !retired[batch] && ce {
                retire_bank = Some((batch % 2) as u8);
                retired[batch] = true;
                break;
            }
        }
        // Fourth batch deliberately reuses bank zero while no other bank switch
        // would have forced a context reload; wait for third draw retirement.
        let source = rows
            .front()
            .filter(|(b, _, _)| *b != 3 || reused_bank_available);
        let input = Tick {
            reset: wall == 0,
            ce,
            source: source.map(|(_, s, _)| *s),
            context: source.map_or(context(10), |(_, _, c)| *c),
            output_ready: !(120..210).contains(&wall) && wall % 13 != 8,
            retire_bank,
        };
        let result = dut.tick(input).unwrap();
        if result.context_accepted {
            context_loads += 1;
            simultaneous_retire_load += usize::from(retire_bank.is_some());
        }
        if let Some(out) = result.output {
            if ce && input.output_ready {
                assert_eq!(golden.pop_front().unwrap(), out, "wall {wall}");
                returned[usize::from(out.epoch - 10)] += 1;
                wait_with_return += usize::from(result.context_wait);
            } else {
                held_output += 1;
            }
        }
        if result.source_ready {
            rows.pop_front().unwrap();
        }
        edges.push(Edge {
            input,
            expected: result,
        });
        if wall > 0 && rows.is_empty() && golden.is_empty() && dut.in_flight() == 0 {
            assert_eq!(returned, totals);
            assert_eq!(context_loads, 4);
            assert!(wait_with_return > 0 && held_output > 0);
            assert!(simultaneous_retire_load > 0);
            return edges;
        }
    }
    panic!("bounded consumer trace did not drain");
}

#[test]
fn selected_stream_matches_independent_counted_outputs_and_lifetimes() {
    let edges = trace();
    assert!(edges.len() < 10000);
    assert!(edges
        .iter()
        .any(|e| !e.input.ce && e.expected.output.is_some()));
}

#[test]
#[ignore = "requires Icarus and Gowin primitive simulation library"]
fn actual_selected_core_and_consumer_match_each_clock_portable_and_native() {
    let edges = trace();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/pixel-lighting-consumer-20261010");
    std::fs::create_dir_all(&dir).unwrap();
    let mut tb = String::from(
        r#"module tb;
reg clk=0,reset=0,ce=0,source_valid=0,source_bank=0,retire_valid=0,retire_bank=0;
reg [2:0] source_row=0;reg [35:0] source_data=0;
reg [4:0] source_slot=0;reg [3:0] source_mask=0;
reg [15:0] context_epoch=0;reg [1:0] context_mode=0;reg [4:0] context_code=0;
reg signed [15:0] light_x=0,light_y=0,light_z=0,ray_x=0,ray_y=0,ray_k=0;
reg [8:0] ambient=0,directional=0;reg out_ready=0;
wire source_ready,context_wait,context_accepted,pixel_accepted,out_valid,out_quad_last;
wire [6:0] out_key;wire [15:0] out_epoch;wire [8:0] out_g,out_h;
gpu_v2_pixel_lighting_consumer dut(.*);
`ifdef GPU_V2_GOWIN_DSP
GSR GSR(.GSRI(1'b1));
`endif
initial begin #100000;$fatal(1,"bounded timeout");end
initial begin
// Initialize sequential state before comparing old-state signals.
reset=1;#2;clk=1;#2;clk=0;#2;
"#,
    );
    for (i, edge) in edges.iter().enumerate() {
        let t = edge.input;
        let s = t.source.unwrap_or(Source {
            row: 0,
            data: 0,
            slot: 0,
            mask: 0,
            draw_bank: 0,
        });
        let c = t.context;
        writeln!(tb, "reset={};ce={};source_valid={};source_row=3'd{};source_data=36'h{:x};source_slot=5'd{};source_mask=4'd{};source_bank={};retire_valid={};retire_bank={};context_epoch=16'd{};context_mode=2'd{};context_code=5'd{};light_x=16'h{:x};light_y=16'h{:x};light_z=16'h{:x};ambient=9'd{};directional=9'd{};ray_x=16'h{:x};ray_y=16'h{:x};ray_k=16'h{:x};out_ready={};#2;",
            u8::from(t.reset),u8::from(t.ce),u8::from(t.source.is_some()),s.row,s.data,s.slot,s.mask,s.draw_bank,
            u8::from(t.retire_bank.is_some()),t.retire_bank.unwrap_or(0),c.epoch,c.mode(),c.material.shininess_code,
            c.light.direction[0] as u16,c.light.direction[1] as u16,c.light.direction[2] as u16,
            c.light.ambient,c.light.directional,c.projection.ray_scale[0] as u16,c.projection.ray_scale[1] as u16,
            c.projection.k as u16,u8::from(t.output_ready)).unwrap();
        let e = edge.expected;
        writeln!(tb,"if(source_ready!==1'b{} || context_wait!==1'b{} || context_accepted!==1'b{} || pixel_accepted!==1'b{} || out_valid!==1'b{}) $fatal(1,\"handshake edge {i} got %b%b%b%b%b\",source_ready,context_wait,context_accepted,pixel_accepted,out_valid);",
            u8::from(e.source_ready),u8::from(e.context_wait),u8::from(e.context_accepted),u8::from(e.pixel_accepted),u8::from(e.output.is_some())).unwrap();
        if let Some(o) = e.output {
            writeln!(tb,"if(out_key!==7'd{} || out_quad_last!==1'b{} || out_epoch!==16'd{} || out_g!==9'd{} || out_h!==9'd{}) $fatal(1,\"result edge {i}: %d %d %d %d %d\",out_key,out_quad_last,out_epoch,out_g,out_h);",
                o.key,u8::from(o.quad_last),o.epoch,o.g,o.h).unwrap();
        }
        tb.push_str("clk=1;#2;clk=0;#2;\n");
    }
    writeln!(
        tb,
        "$display(\"PASS consumer edges={}\");$finish;end endmodule",
        edges.len()
    )
    .unwrap();
    std::fs::write(
        dir.join("consumer.v"),
        lighting_consumer::verilog().unwrap(),
    )
    .unwrap();
    std::fs::write(dir.join("tb.v"), tb).unwrap();
    for native in [false, true] {
        let mut command = std::process::Command::new(
            std::env::var_os("IVERILOG_EXE").unwrap_or("iverilog".into()),
        );
        command
            .current_dir(&dir)
            .args(["-g2012", "-s", "tb", "-o", "sim.vvp"]);
        if native {
            let library =
                std::path::PathBuf::from(std::env::var_os("GOWIN_HOME").expect("GOWIN_HOME"))
                    .join("IDE/simlib/gw2a/prim_sim.v");
            command.arg("-DGPU_V2_GOWIN_DSP").arg(library);
        }
        let compiled = command.args(["consumer.v", "tb.v"]).output().unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let result =
            std::process::Command::new(std::env::var_os("VVP_EXE").unwrap_or("vvp".into()))
                .current_dir(&dir)
                .arg("sim.vvp")
                .output()
                .unwrap();
        assert!(
            result.status.success(),
            "native={native}: {} {}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        println!(
            "native={native}: {}",
            String::from_utf8_lossy(&result.stdout)
        );
    }
}
