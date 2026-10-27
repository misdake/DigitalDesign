//! Selected LOD versus independent integer normalization and analytic log table.
//! Every slope18 code is visited; directed powers cover every material size,
//! filter, mip policy and bias extreme. HDL and child processes are bounded.
use gpu_v2::texture::rtl::lod;
use std::fmt::Write;
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
                "{name}: {} {}",
                std::fs::read_to_string(dir.join(format!("{name}.out"))).unwrap(),
                std::fs::read_to_string(dir.join(format!("{name}.err"))).unwrap()
            );
            break;
        }
        if start.elapsed() > Duration::from_secs(120) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{name} watchdog");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
#[ignore = "requires Icarus Verilog"]
fn selected_lod_all_slope_codes_and_header_extremes() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/sampling-list-20261009/lod-exhaustive");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("dut.v"), lod::verilog()).unwrap();
    let mut table = String::new();
    for i in 0..64 {
        let value = ((1.0 + i as f64 / 64.0).log2() * 256.0).round_ties_even() as u32;
        writeln!(table, "    logs[{i}]={value};").unwrap();
    }
    std::fs::write(dir.join("tb.v"), TB.replace("// GOLDEN_TABLE", &table)).unwrap();
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
    assert!(out.contains("PASS slopes=262144 directed=3564"), "{out}");
}

const TB: &str = r#"
module tb;
  reg clk=0,reset=1,ce=0,in_valid=0;
  reg [17:0] slope;
  reg [3:0] max_n,quad,mask,slot;
  reg has_mip;
  reg [1:0] filter;
  reg signed [15:0] bias;
  wire ready,fault,out_valid;
  wire signed [17:0] shift0;
  wire nearest,halve,last_fine;
  wire signed [11:0] side0,side1;
  wire [8:0] parent0,parent1;
  wire [3:0] n0,n1,out_quad,out_mask,out_slot;
  wire [82:0] outputs={out_slot,out_mask,out_quad,last_fine,n1,n0,parent1,parent0,side1,side0,halve,nearest,shift0};
  reg [82:0] expected,held;
  integer logs[0:63];
  integer clocks=0,inputs=0,pauses=0,i,h,n,m,f,b,j;
  gpu_v2_texture_serial_lod dut(.clk(clk),.reset(reset),.ce(ce),.in_valid(in_valid),
    .in_ready(ready),.fault(fault),.out_valid(out_valid),
    .in_slope_0(slope),.in_meta_0(max_n),.in_has_mip_0(has_mip),.in_filter_0(filter),
    .in_bias_0(bias),.in_meta_1(quad),.in_meta_2(mask),.in_meta_3(slot),
    .out_shift0(shift0),.out_nearest(nearest),.out_halve(halve),.out_side0(side0),.out_side1(side1),
    .out_parent0(parent0),.out_parent1(parent1),.out_n0(n0),.out_n1(n1),.out_last_fine(last_fine),
    .out_quad(out_quad),.out_mask(out_mask),.out_slot(out_slot));
  initial begin #10000000;$fatal(1,"HDL watchdog");end
  task edge_clock;
    begin #1;clk=1;#1;clk=0;#1;clocks=clocks+1;
      if(clocks>3000000) $fatal(1,"clock budget");end
  endtask
  // Arithmetic reference uses division/powers and a loop, not the DUT's
  // grouped bit-normalizer or its RNE bit/OR network.
  function [82:0] golden;
    input integer s,top,has,policy,bs,identity;
    integer rank,value,norm,tail,q,rem,k,exponent,raw,limit,lod,fine,coarse,lambda,sh,sa,sb;
    begin
      limit=has ? top*256 : 0;
      if(s==0) lod=0;
      else if(s>131072) lod=limit;
      else begin
        rank=0;value=s;
        while(value>1) begin value=value/2;rank=rank+1;end
        norm=s*(2**(19-rank));tail=norm-(2**19);
        q=tail/8192;rem=tail%8192;
        k=q+((rem>4096) || ((rem==4096) && (q%2==1)));
        exponent=top+rank-16+(k==64);
        raw=exponent*256+logs[k%64]+bs;
        if(raw<0) lod=0;else if(raw>limit) lod=limit;else lod=raw;
      end
      fine=top-lod/256;coarse=fine>0 ? fine-1 : 0;
      lambda=policy==2 ? 2*(lod%256)-((lod%256)>128) : 0;
      sh=(fine>0 ? fine : 1)-8;
      sa=2**(fine>0 ? fine : 1);sb=2**(coarse>0 ? coarse : 1);
      golden={identity[11:0],(lambda==0),coarse[3:0],fine[3:0],lambda[8:0],(9'd511-lambda[8:0]),
        sb[11:0],sa[11:0],(fine>1),(policy==0),sh[17:0]};
    end
  endfunction
  task sample;
    input integer s,top,has,policy,bs;
    integer age,id;
    begin
      id=(inputs*173+37)%4096;
      slope=s;max_n=top;has_mip=has;filter=policy;bias=bs;
      quad=id%16;mask=(id/16)%16;slot=id/256;
      expected=golden(s,top,has,policy,bs,id);
      if(ready!==1 || out_valid!==0) $fatal(1,"phase0");
      for(age=0;age<8;age=age+1) begin
        ce=1;in_valid=(age==0);#1;
        if(fault!==0) $fatal(1,"legal offer fault");
        edge_clock;
        if(out_valid!==(age==3)) $fatal(1,"latency age=%0d",age);
        if(age==3 && outputs!==expected)
          $fatal(1,"golden input=%0d slope=%0d max=%0d has=%0d filter=%0d bias=%0d got=%h want=%h",
            inputs,s,top,has,policy,bs,outputs,expected);
        // Live inputs change immediately after capture; the result must use
        // retained operands. A CE pause holds the published result too.
        if(age==0) begin slope=18'h3ffff;max_n=0;bias=-123;filter=0;has_mip=0;end
        if(age==3 && inputs%257==0) begin
          held=outputs;ce=0;in_valid=1;#1;
          if(fault!==0) $fatal(1,"CE0 fault");
          edge_clock;edge_clock;
          if(out_valid!==1 || outputs!==held || ready!==0) $fatal(1,"CE0 hold");
          ce=1;in_valid=0;pauses=pauses+1;
        end
        if(age==6 && inputs%509==0) begin
          in_valid=1;#1;if(fault!==1) $fatal(1,"offphase fault");in_valid=0;
        end
      end
      inputs=inputs+1;
    end
  endtask
  initial begin
// GOLDEN_TABLE
    edge_clock;reset=0;ce=1;
    for(i=0;i<262144;i=i+1)
      sample(i,i%11,(i/11)%2,(i/22)%3,(i*97)%16385-8192);
    for(h=0;h<18;h=h+1) for(n=0;n<11;n=n+1)
      for(m=0;m<2;m=m+1) for(f=0;f<3;f=f+1) for(b=-8192;b<=8192;b=b+8192)
        sample(2**h,n,m,f,b);
    // Reset cancels an occupied ROM/partial age and reuses different data.
    slope=65535;max_n=10;has_mip=1;filter=2;bias=0;in_valid=1;edge_clock;
    in_valid=0;edge_clock;ce=0;reset=1;edge_clock;
    if(out_valid!==0 || ready!==1) $fatal(1,"occupied reset");
    reset=0;ce=1;sample(65537,7,1,2,-77);
    $display("PASS slopes=262144 directed=3564 reset=1 inputs=%0d clocks=%0d pauses=%0d",inputs,clocks,pauses);
    $finish;
  end
endmodule
"#;
