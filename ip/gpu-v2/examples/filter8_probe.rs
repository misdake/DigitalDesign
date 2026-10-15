use gpu_v2::texture::{
    emu::color::{ColorEmu, Input, Tick},
    ports::{Group4, TileKey},
};
use std::fmt::Write;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(
        args[1], "half",
        "only the retained half-up filter is supported"
    );
    let mut emu = ColorEmu::new_integer_filter(2000).unwrap();
    let mut tb = String::from("module tb;reg clk=0,reset=1,ce=0,in_valid=0,out_ready=0;reg[71:0]in_payload=0;reg[15:0]in_tex0=0,in_tex1=0,in_tex2=0,in_tex3=0;wire in_ready,out_valid,fault;wire[5:0]out_key;wire[7:0]out_r,out_g,out_b;gpu_v2_texture_runtime_color_checked dut(.*,.preflight_bad());initial begin #1;clk=1;#1;clk=0;reset=0;\n");
    let mut groups = vec![];
    for job in 0..40u8 {
        let weights: [u32; 8] = match job % 5 {
            0 => [511, 0, 0, 0, 0, 0, 0, 0],
            1 => [64, 64, 64, 63, 64, 64, 64, 64],
            2 => [1, 1, 2, 3, 4, 5, 6, 489],
            3 => [128, 127, 128, 128, 0, 0, 0, 0],
            _ => [31, 73, 17, 8, 192, 5, 62, 123],
        };
        for plane in 0..2 {
            let g = Group4 {
                key: TileKey {
                    slot: 0,
                    n: 6,
                    x: 0,
                    y: 0,
                },
                top_left_local: [0, 0],
                coefficients: std::array::from_fn(|t| weights[plane * 4 + t]),
                first: plane == 0,
                last: plane == 1,
                quad_id: job % 16,
                lane: job % 4,
            };
            let texels = std::array::from_fn(|t| {
                if job % 7 == 0 {
                    65535
                } else {
                    ((job as u16) * 1177).wrapping_add((t + plane * 4) as u16 * 2113)
                }
            });
            groups.push(Input {
                payload: g.pack72().unwrap() as i128,
                texels,
            });
        }
    }
    let mut sent = 0;
    let mut commits = 0;
    for wall in 0..1500 {
        let ce = wall % 13 != 5 && wall % 13 != 6;
        let ready = wall % 19 < 11;
        let input = groups.get(sent).copied();
        let step = emu
            .tick(Tick {
                ce,
                input,
                output_ready: ready,
            })
            .unwrap();
        let p = input.map_or(0, |i| i.payload);
        let ts = input.map_or([0; 4], |i| i.texels);
        writeln!(tb,"ce={};out_ready={};in_valid={};in_payload=72'h{:018x};in_tex0=16'h{:04x};in_tex1=16'h{:04x};in_tex2=16'h{:04x};in_tex3=16'h{:04x};#1;if(in_ready!=={})$fatal(1,\"ready {wall}\");",u8::from(ce),u8::from(ready),u8::from(input.is_some()),p,ts[0],ts[1],ts[2],ts[3],u8::from(step.input_ready)).unwrap();
        if let Some(o) = step.output {
            writeln!(tb,"if(out_valid!==1||out_key!==6'd{}||{{out_b,out_g,out_r}}!==24'h{:02x}{:02x}{:02x})$fatal(1,\"output {wall}\");",o.key,o.rgb[2],o.rgb[1],o.rgb[0]).unwrap();
            commits += usize::from(ce && ready);
        } else {
            writeln!(tb, "if(out_valid!==0)$fatal(1,\"valid {wall}\");").unwrap();
        }
        writeln!(tb,"clk=1;#1;clk=0;#1;if(fault!==0||dut.f_count!==5'd{}||dut.credits!==5'd{})$fatal(1,\"state {wall}\");",step.snapshot.queued,step.snapshot.result_credits).unwrap();
        sent += usize::from(step.accepted);
        if sent == groups.len() && emu.idle() {
            assert_eq!(commits, 40);
            writeln!(tb,"$display(\"PASS filter8 live two-edge emu/RTL: {sent} groups, {commits} pixels, {} walls\");$finish;end endmodule",wall+1).unwrap();
            std::fs::write(&args[2], tb).unwrap();
            return;
        }
    }
    panic!("filter8 finite watchdog");
}
