//! Independent raw-port fault/drain assertions, in addition to normal per-edge co-sim.
use gpu_v2::texture::{ports::Slot, rtl::cache};
use std::{
    fs,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
fn run(mut command: Command, dir: &std::path::Path, name: &str) {
    let mut child = command
        .stdout(Stdio::from(
            fs::File::create(dir.join(format!("{name}.out"))).unwrap(),
        ))
        .stderr(Stdio::from(
            fs::File::create(dir.join(format!("{name}.err"))).unwrap(),
        ))
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "{} {}",
                fs::read_to_string(dir.join(format!("{name}.out"))).unwrap(),
                fs::read_to_string(dir.join(format!("{name}.err"))).unwrap()
            );
            break;
        }
        if start.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{name} watchdog");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
#[ignore = "requires Icarus"]
fn malformed_ports_and_terminal_drain_are_real_rtl_checks() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-overnight-20261006/cache-fault-rtl");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("dut.v"),
        cache::verilog(&[Slot {
            base_address: 4096,
            has_full_mip: false,
            max_size_log2: 6,
            valid: true,
        }]),
    )
    .unwrap();
    fs::write(dir.join("tb.v"),r#"
module tb;
 reg clk=0; always #5 clk=~clk;
 reg reset=1,ce=1,abort=0,iv=0,orr=1,mready=0,rv=0,ack=0,ok=1;
 reg [71:0] ip=0; reg [3:0] ri=0; reg [63:0] data=64'h07e007e007e007e0;
 wire ir,ov,mv,fault,drained; wire [31:0] ma; wire [71:0] op;
 wire [15:0] t0,t1,t2,t3;
 gpu_v2_texture_cache dut(.clk(clk),.reset(reset),.ce(ce),.abort(abort),
 .in_valid(iv),.in_payload(ip),.in_ready(ir),.out_ready(orr),.out_valid(ov),.out_payload(op),
 .out_tex0(t0),.out_tex1(t1),.out_tex2(t2),.out_tex3(t3),.mem_req_valid(mv),.mem_req_addr(ma),
 .mem_req_ready(mready),.mem_resp_valid(rv),.mem_resp_index(ri),.mem_resp_data(data),
 .mem_resp_complete(ack),.mem_resp_ok(ok),.fault(fault),.drained(drained));
 task step_edge; begin @(posedge clk); #1; @(negedge clk); end endtask
 task fresh; begin reset=1;ce=1;iv=0;mready=0;rv=0;ack=0;abort=0;step_edge;reset=0;step_edge;end endtask
 task malformed(input [71:0] packet); begin fresh;ip=packet;iv=1;step_edge;iv=0;
   if(!fault || ov || !drained || mv) $fatal(1,"invalid packet not rejected");end endtask
 task request; integer bound; begin fresh;ip=72'h60;iv=1;step_edge;iv=0;
   bound=0;while(!mv && bound<20) begin step_edge;bound=bound+1;end
   if(!mv || ma!=4096) $fatal(1,"legal packet did not issue");
   mready=1;rv=1;ri=0;step_edge;mready=0;rv=0;
 end endtask
 integer i,b;
 initial begin
   step_edge;
   malformed(72'h61); // nonexistent slot
   malformed(72'hb0); // n beyond representable numerical contract
   malformed(72'h70); // n beyond immutable maximum
   malformed(72'h50); // partial chain must use base level
   malformed(72'h860); // tile_x >= side8
   malformed(72'h40060); // tile_y >= side8
   fresh;ce=0;ip=72'h61;iv=1;step_edge;if(fault) $fatal(1,"CE0 invalid offer consumed");iv=0;ce=1;
   fresh;rv=1;ri=0;step_edge;if(!fault || !drained) $fatal(1,"unsolicited beat accepted");rv=0;
   fresh;ack=1;step_edge;if(!fault || !drained) $fatal(1,"unsolicited terminal accepted");ack=0;
   request;ack=1;step_edge;ack=0;if(!fault || !drained || ov) $fatal(1,"early terminal not consumed");
   request; // malformed final beat sharing terminal must consume transport now
   for(i=1;i<15;i=i+1) begin rv=1;ri=i;step_edge;end
   ri=14;ack=1;step_edge;rv=0;ack=0;
   if(!fault || !drained || ov) $fatal(1,"bad final beat lost terminal");
   request; // accepted transport must drain during CE0 after an own mid-burst fault
   ce=0;rv=1;ri=2;step_edge;rv=0;
   if(!fault || drained || ov) $fatal(1,"mid-burst fault canceled transport");
   ack=1;ok=0;step_edge;ack=0;ok=1;if(!drained || ov || mv) $fatal(1,"CE0 fault drain failed");
   request; // a malformed held offer is ignored while the occupied head drains
   for(i=1;i<15;i=i+1) begin rv=1;ri=i;step_edge;end
   ip=72'h61;iv=1;ri=15;ack=1;step_edge;iv=0;rv=0;ack=0;
   if(fault || !drained) $fatal(1,"occupied head consumed malformed offer");
   request; // non-vacuous legal request, actual bank data and output
   for(i=1;i<16;i=i+1) begin rv=1;ri=i;ack=(i==15);step_edge;end
   rv=0;ack=0;b=0;while(!ov && b<20)begin step_edge;b=b+1;end
   if(!ov || fault || !drained || {t0,t1,t2,t3}!=64'h07e007e007e007e0)
     $fatal(1,"legal read/output did not work after reset");
   $display("PASS raw port/terminal/CE0 faults and real legal output");$finish;
 end
 initial begin #20000;$fatal(1,"HDL watchdog");end
endmodule
"#).unwrap();
    let mut compile =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
    compile
        .current_dir(&dir)
        .args(["-g2012", "-s", "tb", "-o", "sim.vvp", "dut.v", "tb.v"]);
    run(compile, &dir, "compile");
    let mut simulate = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
    simulate.current_dir(&dir).arg("sim.vvp");
    run(simulate, &dir, "run");
    assert!(fs::read_to_string(dir.join("run.out"))
        .unwrap()
        .contains("PASS raw port"));
}
