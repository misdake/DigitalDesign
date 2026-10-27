//! Shared immutable D3 fixture programs and reference images.
#[allow(dead_code)]
#[path = "gpu_rcc_d2.rs"]
pub mod draw_fixture;
pub fn program() -> String {
    let full = draw_fixture::program();
    let prefix = full.split(" let mut iteration:u16=0;").next().unwrap();
    let prefix = prefix.replace(
        "static MATRIX:",
        "static FST:Buf<u16,8>=Buf::new([0;8]);\nstatic MATRIX:",
    );
    format!(
        r#"{prefix}
 mesh.count=1;uv.count=1;let mut iteration:u16=0;
 while iteration<2{{
   let cb=gpu_cb_begin();check(cb==1);let pass=gpu_pass_begin(cb,iteration+1,65535);check(pass==1);
   check(gpu_pass_set_global(pass,view_of(&mut g)));r.unlit=1;
   check(gpu_pass_set_render(pass,view_of(&mut texture),view_of(&mut r)));
   gpu_affine_identity(MATRIX.as_array());
   gpu_fstore(MATRIX.as_array(),3,fix32::from_words(32768,65535));
   gpu_fstore(MATRIX.as_array(),7,fix32::from_words(57344,65535));
   gpu_fstore(MATRIX.as_array(),12,fix32::from_int(1));gpu_fstore(MATRIX.as_array(),16,fix32::from_int(1));gpu_fstore(MATRIX.as_array(),20,fix32::from_int(1));
   check(gpu_pass_draw_mesh(pass,view_of(&mut mesh),MATRIX.as_array(),65408,65535));
   r.unlit=0;check(gpu_pass_set_render(pass,view_of(&mut texture),view_of(&mut r)));
   gpu_fstore(MATRIX.as_array(),3,fix32::zero());gpu_fstore(MATRIX.as_array(),7,fix32::from_words(8192,0));
   check(gpu_pass_draw_mesh(pass,view_of(&mut uv),MATRIX.as_array(),65535,65535));
   check(gpu_pass_end(pass));check(gpu_cb_finish(cb));
   // The diagnostic alias is dirty in the outgoing cache when handoff begins.
   let saved=read_dseg_page(3);
   unsafe{{write_dseg_page(3,GPU_CB.color>>8);Ptr::from_addr(0xc000).write(0,0xdead);write_dseg_page(3,saved);}}
   let v=vec4::new(fix32::from_int(1),fix32::from_int(2),fix32::from_int(3),fix32::from_int(4));vec4::export(v,FST.as_ptr());
   check(gpu_submit_idle(cb)==GPU_PENDING);check(gpu_wait(cb,65535)==GPU_COMPLETED);dev_send(7,0,iteration+1);
   check(gpu_present(cb)==GPU_PENDING);check(gpu_present(cb)==GPU_REJECTED);
   check(!gpu_discard_ready(cb));check(!gpu_cb_cancel(cb));
   let result=gpu_present_wait(1);check(result==GPU_TIMEOUT||result==GPU_COMPLETED);
   if result==GPU_TIMEOUT{{check(gpu_present_wait(65535)==GPU_COMPLETED);}}
   dev_send(7,2,iteration+1);iteration+=1;
 }}
 let frame=dev_recv(3,0);let mut left:u16=65535;
 while dev_recv(3,0)==frame&&left!=0{{left-=1;}}check(left!=0);
 check(read_dseg_page(3)==19);halt(0x600d);
}}"#
    )
}
pub fn surface(frame: u16) -> Vec<u16> {
    let mut result = vec![frame; 96000];
    for (tx, ty, color) in [(-32768i64, -8192i64, 0x8000u16), (0, 8192, 0x07e0)] {
        for y in 0..240i64 {
            for x in 0..400i64 {
                let a = (2 * x - 399) * 65536 - 512 * tx;
                let b = (2 * y - 239) * 65536 - 512 * ty;
                if a >= 0 && b >= 0 && a + b < 512 * 4096 {
                    result[(y * 400 + x) as usize] = color;
                }
            }
        }
    }
    result
}
pub fn fault_program() -> String {
    r#"
 mod gpu;
 fn check(x:bool){if !x{halt(0xbad);}}
 fn main(){
  unsafe{write_dseg_page(3,19);}check(gpu_init()==GPU_IDLE);let cb=gpu_cb_begin();check(cb==1);
  check(!gpu_pass_clear(cb,0,65535));check(read_dseg_page(3)==19);
  check(gpu_poll(cb)==GPU_FAULT);check(gpu_present(cb)==GPU_FAULT);check(gpu_cb_begin()==0);
  check(!gpu_discard_ready(cb));check(!gpu_cb_cancel(cb));halt(0x600d);
 }
 "#
    .into()
}
pub fn fault_testbench(mut tb: String) -> String {
    tb = tb
        .replace("localparam FAULT=0;", "localparam FAULT=1;reg injected=0;")
        .replace(
            "assign ae=1'b0;",
            "assign ae=ms==5&&write_owner==1&&mb>=('h4a0000/8)&&!injected;",
        )
        .replace("5:if(arr)begin", "5:if(arr)begin if(ae)injected<=1;")
        .replace(
            "for(k=0;k<16;k=k+1)if(k<mn)mem[mb+k]<=write_buffer[k];",
            "if(!ae)for(k=0;k<16;k=k+1)if(k<mn)mem[mb+k]<=write_buffer[k];",
        )
        .replace("if(write_owner==1)begin", "if(write_owner==1&&!ae)begin")
        .replace("if(cycle>1500000)", "if(cycle>120000)");
    let begin = tb.find(" if(starts!=2||completions").unwrap();
    let finish = tb[begin..].find(" $finish;").unwrap() + begin;
    tb.replace_range(begin..finish,r#" if(!injected||starts!=0||writes!=0||presents!=0||swaps!=0||display.active_base!='h200000)$fatal(1,"Maintenance error escaped ownership");
 $display("RCC_PRESENT_FAULT_PASS cycles=%0d starts=%0d writes=%0d swaps=%0d",cycle,starts,writes,swaps);
"#);

    tb
}

// Read the compiler's static location instead of maintaining a second address.
pub fn fst_address(program: &cpu_v3::rcc_backend::CpuV3Program) -> u16 {
    program
        .debug
        .globals
        .iter()
        .find_map(|v| {
            if v.name == "FST" {
                match v.loc {
                    rcc::VarLoc::Global(address) => Some(address),
                    _ => None,
                }
            } else {
                None
            }
        })
        .unwrap_or(u16::MAX)
}
