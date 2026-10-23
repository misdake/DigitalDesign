//! Real compiled UI snapshots, native atlas bytes and independent screen reference.
#[allow(dead_code)]
#[path = "gpu_rcc_present_fixture.rs"]
pub mod present;
use std::collections::BTreeMap;
pub fn program() -> String {
    let mut s = present::program().replace(
        "static MATRIX:",
        "static CELLS:Buf<u16,375>=Buf::new([65535;375]);\nstatic MATRIX:",
    );
    s = s.replace(
        "mesh.count=1;uv.count=1;",
        "let mut atlas:GpuUiAtlas=GpuUiAtlas{sector:0x9800,sectors:512};mesh.count=1;uv.count=1;",
    );
    s=s.replace("   r.unlit=0;",r#"   check(gpu_ui_fill(CELLS.as_array(),255));
   let id:u16=if iteration==0{0}else{254};let other:u16=if iteration==0{254}else{0};
   check(gpu_ui_set_cell(CELLS.as_array(),0,0,id));check(gpu_ui_set_cell(CELLS.as_array(),1,0,1));
   check(gpu_ui_set_cell(CELLS.as_array(),49,14,other));
   check(gpu_ui_set_cell(CELLS.as_array(),9,5,id));check(gpu_ui_set_cell(CELLS.as_array(),25,9,other));
   // Diagnostic second-frame placement crosses the real DSEG3 window boundary.
   if iteration!=0{GPU_CB.object_words=16320;}
   check(gpu_pass_draw_ui_grid(pass,view_of(&mut atlas),CELLS.as_array()));
   check(gpu_ui_fill(CELLS.as_array(),255));
   r.unlit=0;"#);
    s = s.replace(
        "   check(gpu_pass_end(pass));",
        r#"   check(gpu_pass_draw_ui_grid(pass,view_of(&mut atlas),CELLS.as_array()));
   check(gpu_ui_fill(CELLS.as_array(),0));
   check(gpu_pass_end(pass));"#,
    );
    s
}
pub fn pixel(id: u16, x: usize, y: usize) -> u16 {
    let alpha = id != 1 && !(x + y + usize::from(id)).is_multiple_of(5);
    let id = usize::from(id);
    ((((id + 3 * x + 7 * y) & 31) << 11)
        | (((id * 3 + x * 5 + y) & 31) << 6)
        | (((id * 7 + x + y * 3) & 31) << 1)
        | usize::from(alpha)) as u16
}
pub fn assets() -> BTreeMap<u32, u8> {
    let mut a = present::draw_fixture::assets();
    for i in 0..65536 {
        a.insert(0x4c0000 + i, 0);
    }
    for id in [0u16, 1, 254] {
        let pixels = std::array::from_fn(|y| std::array::from_fn(|x| pixel(id, x, y)));
        let bytes = gpu_v2::system::vliw_command::ui_char(&pixels);
        for (i, b) in bytes.into_iter().enumerate() {
            a.insert(0x4c0000 + u32::from(id) * 256 + i as u32, b);
        }
    }
    a
}
pub fn surface(frame: u16) -> Vec<u16> {
    let both = present::surface(frame);
    let mut out: Vec<u16> = both
        .iter()
        .map(|v| if *v == 0x07e0 { frame } else { *v })
        .collect();
    let id = if frame == 1 { 0 } else { 254 };
    let other = if frame == 1 { 254 } else { 0 };
    for (cx, cy, id) in [
        (0, 0, id),
        (1, 0, 1),
        (49, 14, other),
        (9, 5, id),
        (25, 9, other),
    ] {
        for y in 0..16 {
            for x in 0..8 {
                let p = pixel(id, x, y);
                if p & 1 != 0 {
                    let r = (p >> 11) & 31;
                    let g = (p >> 6) & 31;
                    let b = (p >> 1) & 31;
                    out[(cy * 16 + y) * 400 + cx * 8 + x] =
                        (r << 11) | (((g << 1) | (g >> 4)) << 5) | b;
                }
            }
        }
    }
    // The second 3D draw follows UI and overwrites its covered pixels.
    for (i, v) in both.iter().enumerate() {
        if *v == 0x07e0 {
            out[i] = *v;
        }
    }
    out
}
pub fn depth() -> Vec<u16> {
    present::surface(1)
        .iter()
        .map(|v| if *v == 1 { 65535 } else { 256 })
        .collect()
}

pub fn fault_program() -> String {
    r#"
mod gpu;static CELLS:Buf<u16,375>=Buf::new([0;375]);
fn check(x:bool){if !x{halt(0xbad);}}
fn main(){unsafe{write_dseg_page(3,19);}check(gpu_init()==GPU_IDLE);let c=gpu_cb_begin();
let p=gpu_pass_begin(c,0,65535);check(p==1);let mut a:GpuUiAtlas=GpuUiAtlas{sector:0x9800,sectors:512};
let words=GPU_CB.object_words;let commands=GPU_CB.command_words;dev_send(7,4,1);
check(!gpu_pass_draw_ui_grid(p,view_of(&mut a),CELLS.as_array()));
check(read_dseg_page(3)==19);check(GPU_CB.object_words==words&&GPU_CB.command_words==commands);
check(gpu_poll(c)==GPU_FAULT);check(gpu_submit_idle(c)==GPU_FAULT);check(gpu_present(c)==GPU_FAULT);
check(!gpu_cb_cancel(c));check(!gpu_discard_ready(c));check(gpu_cb_begin()==0);halt(0x600d);}
"#.into()
}
pub fn fault_testbench(mut tb: String) -> String {
    tb = tb
        .replace(
            "localparam FAULT=0;",
            "localparam FAULT=1;reg armed=0,injected=0;",
        )
        .replace(
            "assign ae=1'b0;",
            "assign ae=ms==5&&write_owner==1&&mb>=('h4a0080/8)&&mb<('h4b0000/8)&&armed&&!injected;",
        )
        .replace("5:if(arr)begin", "5:if(arr)begin if(ae)injected<=1;")
        .replace(
            "for(k=0;k<16;k=k+1)if(k<mn)mem[mb+k]<=write_buffer[k];",
            "if(!ae)for(k=0;k<16;k=k+1)if(k<mn)mem[mb+k]<=write_buffer[k];",
        )
        .replace("if(write_owner==1)begin", "if(write_owner==1&&!ae)begin")
        .replace(
            "if(wen&&di==7&&ch==1)",
            "if(wen&&di==7&&ch==4)armed<=1;\n   if(wen&&di==7&&ch==1)",
        )
        .replace("if(cycle>1500000)", "if(cycle>120000)");
    let begin = tb.find(" if(uis!=4").unwrap();
    let end = tb[begin..].find(" $finish;").unwrap() + begin;
    tb.replace_range(begin..end,r#" if(!armed||!injected||starts!=0||writes!=0||uis!=0||presents!=0||swaps!=0||display.active_base!='h200000)$fatal(1,"UI maintenance fault escaped ownership");
 $display("RCC_UI_FAULT_PASS cycles=%0d starts=%0d writes=%0d swaps=%0d UI=%0d",cycle,starts,writes,swaps,uis);
"#);
    tb
}
