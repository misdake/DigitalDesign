//! Narrow quantizer proof plus actual eight-stage RTL and independent gold.
use gpu_v2::{
    framebuffer::rtl::ROP_LEAF,
    system::pixel::{framebuffer_native, framebuffer_quantization},
};
use std::{collections::BTreeMap, fmt::Write};

#[test]
fn all_byte_quantizations_are_exact() {
    for c in 0..=255_u32 {
        let head = c >> 3;
        let low = c & 7;
        let down = (low == 0 && head >= 19) || (low == 1 && head >= 23) || (low == 2 && head >= 28);
        let up = (low == 5 && head <= 3) || (low == 6 && head <= 8) || (low == 7 && head <= 12);
        assert_eq!(
            head + u32::from(up) - u32::from(down),
            (c * 31 + 127) / 255,
            "q5 c{c}"
        );
        let head = c >> 2;
        let low = c & 3;
        let down = low == 0 && head >= 43;
        let up = low == 3 && head <= 20;
        assert_eq!(
            head + u32::from(up) - u32::from(down),
            (c * 63 + 127) / 255,
            "q6 c{c}"
        );
    }
}

#[derive(Clone, Copy)]
struct Lane {
    key: u8,
    blend: bool,
    func: u8,
    dw: bool,
    covered: bool,
    old_color: u16,
    old_depth: u16,
    rgb: [u8; 3],
    alpha: u8,
    depth: u16,
}
#[derive(Clone, Copy)]
struct Output {
    key: u8,
    color: u16,
    depth: u16,
    cw: bool,
    dw: bool,
}
fn gold(l: Lane) -> Output {
    let pass = l.covered
        && match l.func {
            0 => false,
            1 => l.depth < l.old_depth,
            2 => l.depth == l.old_depth,
            3 => l.depth <= l.old_depth,
            4 => l.depth > l.old_depth,
            5 => l.depth != l.old_depth,
            6 => l.depth >= l.old_depth,
            _ => true,
        };
    let r = (l.old_color >> 11) & 31;
    let g = (l.old_color >> 5) & 63;
    let b = l.old_color & 31;
    let dst = [
        (r << 3) | (r >> 2),
        (g << 2) | (g >> 4),
        (b << 3) | (b >> 2),
    ];
    let rgb: [u32; 3] = std::array::from_fn(|c| {
        if l.blend {
            (u32::from(l.alpha) * u32::from(l.rgb[c])
                + (255 - u32::from(l.alpha)) * u32::from(dst[c])
                + 127)
                / 255
        } else {
            u32::from(l.rgb[c])
        }
    });
    let color = (((rgb[0] * 31 + 127) / 255) << 11)
        | (((rgb[1] * 63 + 127) / 255) << 5)
        | ((rgb[2] * 31 + 127) / 255);
    Output {
        key: l.key,
        color: if pass { color as u16 } else { l.old_color },
        depth: if pass && l.dw { l.depth } else { l.old_depth },
        cw: pass,
        dw: pass && l.dw,
    }
}
fn lanes() -> Vec<Lane> {
    let mut lanes = Vec::new();
    let mut add = |rgb, alpha, blend, func, dw, covered, old_color, relation: i32| {
        lanes.push(Lane {
            key: (lanes.len() % 4) as u8,
            rgb,
            alpha,
            blend,
            func,
            dw,
            covered,
            old_color,
            old_depth: 32768,
            depth: (32768 + relation) as u16,
        });
    };
    for c in 0..=255 {
        add([c, 255 - c, c], 255, false, 7, true, true, 0, 0);
    }
    for a in 0..=255 {
        for func in 0..8 {
            for blend in [false, true] {
                add(
                    [a, 255 - a, a.wrapping_mul(53)],
                    a,
                    blend,
                    func,
                    a % 3 != 0,
                    a % 7 != 0,
                    if a % 2 == 0 { 0xffff } else { 0x52aa },
                    i32::from((u16::from(a) + u16::from(func) + u16::from(blend)) % 3) - 1,
                );
            }
        }
    }
    for func in 0..8 {
        for relation in [-1, 0, 1] {
            for covered in [false, true] {
                for dw in [false, true] {
                    for blend in [false, true] {
                        for alpha in [0, 1, 127, 128, 254, 255] {
                            add(
                                [0, 128, 255],
                                alpha,
                                blend,
                                func,
                                dw,
                                covered,
                                0xf81f,
                                relation,
                            );
                        }
                    }
                }
            }
        }
    }
    lanes
}
fn fixture() -> (String, usize, usize) {
    let inputs = lanes();
    let mut ptr = 0;
    let mut enabled = 0;
    let mut due: BTreeMap<usize, Output> = BTreeMap::new();
    let mut tb = TB.to_string();
    let mut body = String::new();
    let mut outputs = 0;
    let mut accepted = 0;
    let mut discarded = 0;
    let mut complete = false;
    let mut total = 0;
    for wall in 0..18000 {
        let reset = wall == 37;
        let ce = wall % 17 != 13 && wall % 17 != 14 && wall % 23 != 5 && wall % 23 != 6;
        let valid = ptr < inputs.len() && wall % 11 != 7;
        let l = inputs[ptr.min(inputs.len() - 1)];
        writeln!(body,"reset={};ce={};in_valid={};in_key=2'd{};blend={};depth_func=3'd{};depth_write={};covered={};old_color=16'd{};old_depth=16'd{};src_depth=16'd{};src_r=8'd{};src_g=8'd{};src_b=8'd{};src_a=8'd{};#1;",u8::from(reset),u8::from(ce),u8::from(valid),l.key,u8::from(l.blend),l.func,u8::from(l.dw),u8::from(l.covered),l.old_color,l.old_depth,l.depth,l.rgb[0],l.rgb[1],l.rgb[2],l.alpha).unwrap();
        writeln!(body,"if(in_ready!==1'b{}||ref_ready!==in_ready||out_valid!==ref_valid)$fatal(1,\"control edge{wall}\");",u8::from(ce)).unwrap();
        let expected = due.get(&enabled).copied();
        writeln!(
            body,
            "if(out_valid!==1'b{})$fatal(1,\"eight-stage valid edge{wall}\");",
            u8::from(expected.is_some())
        )
        .unwrap();
        if let Some(o) = expected {
            writeln!(body,"if({{out_key,new_color,new_depth,color_written,depth_written}}!=={{ref_key,ref_color,ref_depth,ref_cw,ref_dw}})$fatal(1,\"matched payload edge{wall}\");").unwrap();
            writeln!(body,"if(out_key!==2'd{}||new_color!==16'd{}||new_depth!==16'd{}||color_written!==1'b{}||depth_written!==1'b{})$fatal(1,\"gold edge{wall} color%h depth%h\",new_color,new_depth);",o.key,o.color,o.depth,u8::from(o.cw),u8::from(o.dw)).unwrap();
            if ce && !reset {
                outputs += 1;
            }
        }
        body.push_str("clk=1;#1;clk=0;#1;\n");
        if reset {
            discarded += due.range(enabled..).count();
            due.clear();
            enabled = 0;
            ptr = 0;
        } else if ce {
            if valid {
                due.insert(enabled + 8, gold(l));
                ptr += 1;
                accepted += 1;
            }
            enabled += 1;
        }
        total = wall + 1;
        if ptr == inputs.len() && due.range(enabled..).count() == 0 {
            complete = true;
            break;
        }
    }
    assert!(complete, "finite ROP drain");
    assert_eq!(accepted, outputs + discarded);
    writeln!(body,"$display(\"PASS narrow ROP edges{total} inputs{accepted} outputs{outputs} resetdiscard{discarded}\");$finish;").unwrap();
    tb = tb.replace("// EDGES", &body);
    (tb, total, inputs.len())
}
const TB: &str = r#"module tb;
reg clk=0,reset=1,ce=0,in_valid=0,blend=0,depth_write=0,covered=0;
reg[1:0]in_key=0;reg[2:0]depth_func=0;reg[15:0]old_color=0,old_depth=0,src_depth=0;
reg[7:0]src_r=0,src_g=0,src_b=0,src_a=0;
wire in_ready,out_valid,color_written,depth_written;wire[1:0]out_key;wire[15:0]new_color,new_depth;
rop_leaf dut(.*);
wire ref_ready,ref_valid,ref_cw,ref_dw;wire[1:0]ref_key;wire[15:0]ref_color,ref_depth;
rop_leaf_reference reference(.clk(clk),.reset(reset),.ce(ce),.in_valid(in_valid),.in_key(in_key),.blend(blend),.depth_func(depth_func),.depth_write(depth_write),.covered(covered),.old_color(old_color),.old_depth(old_depth),.src_r(src_r),.src_g(src_g),.src_b(src_b),.src_a(src_a),.src_depth(src_depth),.in_ready(ref_ready),.out_valid(ref_valid),.out_key(ref_key),.new_color(ref_color),.new_depth(ref_depth),.color_written(ref_cw),.depth_written(ref_dw));
`ifdef NATIVE
GSR GSR(.GSRI(1'b1));
`endif
initial begin #100000;$fatal(1,"time watchdog");end
initial begin #1;clk=1;#1;clk=0;#1;reset=0;
// EDGES
end endmodule
"#;

#[test]
#[ignore = "requires Icarus and Gowin primitive library"]
fn actual_narrow_rop_portable_native_matches_reference_and_gold() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/pixel-rop-quantization-20261010");
    std::fs::create_dir_all(&dir).unwrap();
    let (tb, edges, inputs) = fixture();
    std::fs::write(dir.join("tb.v"), tb).unwrap();
    std::fs::write(dir.join("original.v"), ROP_LEAF).unwrap();
    for native in [false, true] {
        let narrowed = framebuffer_quantization::lower(ROP_LEAF.to_string());
        let leaf = if native {
            framebuffer_native::bind_products(narrowed)
        } else {
            narrowed
        };
        let reference = ROP_LEAF.replacen("module rop_leaf (", "module rop_leaf_reference (", 1);
        let name = format!("native-{native}.v");
        std::fs::write(dir.join(&name), format!("{leaf}\n{reference}")).unwrap();
        std::fs::write(dir.join(format!("leaf-native-{native}.v")), &leaf).unwrap();
        let mut compiler = std::process::Command::new(
            std::env::var_os("IVERILOG_EXE").unwrap_or("iverilog".into()),
        );
        compiler
            .current_dir(&dir)
            .args(["-g2012", "-s", "tb", "-o", "sim.vvp"]);
        if native {
            compiler.arg("-DNATIVE").arg(
                std::path::PathBuf::from(std::env::var_os("GOWIN_HOME").expect("GOWIN_HOME"))
                    .join("IDE/simlib/gw2a/prim_sim.v"),
            );
        }
        let compiled = compiler.args([&name, "tb.v"]).output().unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let ran = std::process::Command::new(std::env::var_os("VVP_EXE").unwrap_or("vvp".into()))
            .current_dir(&dir)
            .arg("sim.vvp")
            .output()
            .unwrap();
        assert!(
            ran.status.success(),
            "native{native}: {} {}",
            String::from_utf8_lossy(&ran.stdout),
            String::from_utf8_lossy(&ran.stderr)
        );
        println!(
            "native{native} vectors{inputs} max18000 actualedges{edges}: {}",
            String::from_utf8_lossy(&ran.stdout)
        );
    }
}
