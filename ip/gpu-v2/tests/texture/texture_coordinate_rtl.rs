//! Exhaustive checked-domain Coordinate RTL qualification. The HDL golden uses
//! integer multiplication/division and signed modulo, independently of the
//! selected bit-slice shifter/masks. Both axes visit every UV16 code at every
//! physical size and nearest policy; CE and off-phase admission are separate.
use gpu_v2::texture::rtl::coordinate;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn bounded(mut cmd: Command, dir: &std::path::Path, name: &str) {
    let stdout = std::fs::File::create(dir.join(format!("{name}.out"))).unwrap();
    let stderr = std::fs::File::create(dir.join(format!("{name}.err"))).unwrap();
    let mut child = cmd
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "{name} failed: {} {}",
                std::fs::read_to_string(dir.join(format!("{name}.out"))).unwrap(),
                std::fs::read_to_string(dir.join(format!("{name}.err"))).unwrap()
            );
            break;
        }
        if start.elapsed() > Duration::from_secs(120) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{name} wall watchdog");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
#[ignore = "requires Icarus Verilog"]
fn selected_coordinate_all_uv_codes_and_ce_match_integer_goldens() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/sampling-axis-share-20261009/coordinate-exhaustive");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("dut.v"), coordinate::verilog()).unwrap();
    std::fs::write(dir.join("tb.v"), TB).unwrap();
    let mut compile =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
    compile
        .current_dir(&dir)
        .args(["-g2012", "-s", "tb", "-o", "run.vvp", "dut.v", "tb.v"]);
    bounded(compile, &dir, "compile");
    let mut run = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
    run.current_dir(&dir).arg("run.vvp");
    bounded(run, &dir, "run");
    let out = std::fs::read_to_string(dir.join("run.out")).unwrap();
    assert!(out.contains("PASS inputs=1310720 axes=2621440"), "{out}");
}

const TB: &str = r#"
module tb;
  reg clk=0,ce=0,reset=1,in_valid=0;
  reg nearest,halve;
  reg signed [17:0] shift;
  reg signed [11:0] side0,side1;
  reg [15:0] uv0,uv1;
  wire ready,fault,out_valid;
  wire [7:0] f00,f01,f10,f11;
  wire [9:0] t000,t001,t010,t011,t100,t101,t110,t111;
  wire [111:0] outputs={t111,t110,t101,t100,t011,t010,t001,t000,f11,f10,f01,f00};
  integer n,policy,uv,inputs,axes,clocks,p;
  reg [111:0] held;
  reg held_valid,held_ready;
  gpu_v2_texture_serial_coord dut(.clk(clk),.ce(ce),.reset(reset),
    .in_valid(in_valid),.in_ready(ready),.fault(fault),.out_valid(out_valid),
    .in_flags_0(nearest),.in_flags_1(halve),.in_coordinate_shift_0(shift),
    .in_wrapped_uv_0(uv0),.in_wrapped_uv_1(uv1),.in_side_0(side0),.in_side_1(side1),
    .out_f0_0(f00),.out_f0_1(f01),.out_f1_0(f10),.out_f1_1(f11),
    .out_t0_0_0(t000),.out_t0_0_1(t001),.out_t0_1_0(t010),.out_t0_1_1(t011),
    .out_t1_0_0(t100),.out_t1_0_1(t101),.out_t1_1_0(t110),.out_t1_1_1(t111));
  task tick(input c,input v);
    begin
      ce=c;in_valid=v;#1;clk=1;#1;clk=0;in_valid=0;
      clocks=clocks+1;
      if(clocks>3000000) $fatal(1,"clock budget");
    end
  endtask
  task pause;
    begin
      held=outputs;held_valid=out_valid;held_ready=ready;
      for(p=0;p<3;p=p+1) begin
        tick(0,1);
        if(outputs!==held || out_valid!==held_valid || ready!==held_ready || fault!==0)
          $fatal(1,"CE hold n=%0d uv=%0d",n,uv);
      end
    end
  endtask
  task check_axis(input integer code,input [7:0] f0,input [7:0] f1,
    input [9:0] a0,input [9:0] b0,input [9:0] a1,input [9:0] b1);
    integer qf,qc,ifine,icoarse,s0,s1,w0,w1,w2,w3;
    begin
      s0=1<<n;s1=s0/2;if(s1<2) s1=2;
      qf=(code*s0)/256;
      if(policy==0) qf=qf-128;
      if(n>1) qc=(qf-128) >>> 1;
      else qc=qf;
      ifine=qf >>> 8;icoarse=qc >>> 8;
      w0=((ifine%s0)+s0)%s0;w1=(((ifine+1)%s0)+s0)%s0;
      w2=((icoarse%s1)+s1)%s1;w3=(((icoarse+1)%s1)+s1)%s1;
      if(f0!==(qf&255) || f1!==(qc&255) ||
         a0!==w0 || b0!==w1 || a1!==w2 || b1!==w3)
        $fatal(1,"coordinate n=%0d nearest=%0d uv=%0d fractions=%0d/%0d expected=%0d/%0d taps=%0d,%0d,%0d,%0d expected=%0d,%0d,%0d,%0d",
          n,nearest,code,f0,f1,(qf&255),(qc&255),a0,b0,a1,b1,w0,w1,w2,w3);
      axes=axes+1;
    end
  endtask
  initial begin
    clocks=0;inputs=0;axes=0;tick(0,0);reset=0;
    for(n=1;n<=10;n=n+1) for(policy=0;policy<2;policy=policy+1)
      for(uv=0;uv<65536;uv=uv+1) begin
        nearest=policy;halve=n>1;shift=n-8;side0=1<<n;
        if(n>1) side1=side0/2;else side1=2;
        uv0=uv;uv1=65535-uv;
        if(ready!==1) $fatal(1,"II2 phase before issue");
        tick(1,1);
        if(out_valid!==0 || ready!==0) $fatal(1,"age1 contract");
        // Off-phase admission is detected without advancing/fixing the phase.
        ce=1;in_valid=1;#1;
        if(fault!==1) $fatal(1,"off-phase fault");
        in_valid=0;
        if((uv%8192)==0) pause;
        // U has executed. Poison every external operand before V's edge.
        uv0=16'hdead;uv1=16'hbeef;shift=18'sh20000;
        nearest=~policy;halve=n==1;side0=-1;side1=-1;
        tick(1,0);
        if(out_valid!==1 || ready!==1) $fatal(1,"age2 contract");
        check_axis(uv,f00,f10,t000,t001,t100,t101);
        check_axis(65535-uv,f01,f11,t010,t011,t110,t111);
        if((uv%8192)==0) pause;
        inputs=inputs+1;
      end
    // Reset hides held data, then different inputs must produce fresh values.
    reset=1;tick(0,1);reset=0;
    if(out_valid!==0 || ready!==1) $fatal(1,"reset control");
    n=7;policy=0;nearest=0;halve=1;shift=-1;side0=128;side1=64;uv0=13579;uv1=24680;
    tick(1,1);tick(1,0);
    check_axis(uv0,f00,f10,t000,t001,t100,t101);
    check_axis(uv1,f01,f11,t010,t011,t110,t111);
    if(inputs!=1310720 || axes!=2621442) $fatal(1,"coverage");
    $display("PASS inputs=1310720 axes=2621440 reset_axes=2 clocks=%0d",clocks);$finish;
  end
  initial begin #14000000;$fatal(1,"HDL watchdog");end
endmodule
"#;
