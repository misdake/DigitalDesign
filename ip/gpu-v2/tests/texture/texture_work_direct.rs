//! Two actual Work implementations and an independent immutable-row scoreboard.
use gpu_v2::texture::rtl::work;

#[test]
#[ignore = "requires Icarus Verilog"]
fn actual_direct_payload_matches_old_transport() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/work-direct-20261010");
    std::fs::create_dir_all(&dir).unwrap();
    let old =
        work::compact_verilog().replace("module gpu_v2_texture_work", "module reference_work");
    std::fs::write(
        dir.join("dut.v"),
        old + "\n" + &work::compact_direct_verilog(),
    )
    .unwrap();
    std::fs::write(dir.join("tb.v"), TB).unwrap();
    let compiled =
        std::process::Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or("iverilog".into()))
            .current_dir(&dir)
            .args(["-g2012", "-s", "tb", "-o", "sim.vvp", "dut.v", "tb.v"])
            .output()
            .unwrap();
    std::fs::write(dir.join("compile.err"), &compiled.stderr).unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let run = std::process::Command::new(std::env::var_os("VVP_EXE").unwrap_or("vvp".into()))
        .current_dir(&dir)
        .arg("sim.vvp")
        .output()
        .unwrap();
    std::fs::write(dir.join("run.out"), &run.stdout).unwrap();
    assert!(
        run.status.success(),
        "{} {}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    println!("{}", String::from_utf8_lossy(&run.stdout));
}

const TB: &str = r#"module tb;
reg clk=0,reset=1,ce=0,in_valid=0,out_ready=0;reg[75:0]in_member=0;
wire ar,av,af,b_r,b_v,b_f,a_read,a_write,a_capture,a_ack,b_read,b_write,b_capture,b_ack;
wire[75:0]a_member,b_member;wire[1:0]a_tap,b_tap,a_loaded,b_loaded;
wire[3:0]a_rr,a_wr,b_rr,b_wr;wire[4:0]a_count,b_count;
reference_work a(.clk(clk),.reset(reset),.ce(ce),.in_valid(in_valid),.in_member(in_member),.in_ready(ar),
.out_ready(out_ready),.out_valid(av),.out_member(a_member),.out_tap(a_tap),.fault(af),
.read_fire(a_read),.write_fire(a_write),.capture_fire(a_capture),.ack(a_ack),.read_row(a_rr),.write_row(a_wr),.materialized(a_count),.loaded(a_loaded));
gpu_v2_texture_work b(.clk(clk),.reset(reset),.ce(ce),.in_valid(in_valid),.in_member(in_member),.in_ready(b_r),
.out_ready(out_ready),.out_valid(b_v),.out_member(b_member),.out_tap(b_tap),.fault(b_f),
.read_fire(b_read),.write_fire(b_write),.capture_fire(b_capture),.ack(b_ack),.read_row(b_rr),.write_row(b_wr),.materialized(b_count),.loaded(b_loaded));
wire[21:0]a_control={af,ar,av,a_read,a_write,a_capture,a_ack,a_rr,a_wr,a_count,a_loaded};
wire[21:0]b_control={b_f,b_r,b_v,b_read,b_write,b_capture,b_ack,b_rr,b_wr,b_count,b_loaded};
reg[75:0]gold[0:15];reg[3:0]remaining[0:15];
integer gc=0,gh=0,gt=0,wall=0,edges=0,sent=0,old_count=0,i;
integer full=0,ack_put=0,fetch_ack=0,ce0_valid=0,held=0,wraps=0,outputs=0,acks=0;
function[75:0]member;input integer n;reg[75:0]value;begin
 value[31:0]=32'hbc793ab5^(n*32'd93481);
 value[63:32]=32'h7a315def^(n*32'd721973);
 value[75:64]=n*13;value[39:36]=(n%15)+1;member=value;
end endfunction
function[1:0]first;input[3:0]mask;begin
 if(mask[0])first=0;else if(mask[1])first=1;else if(mask[2])first=2;else first=3;
end endfunction
task tick;begin clk=1;#2;clk=0;#2;edges=edges+1;end endtask
task check;begin
 if(a_control!==b_control)$fatal(1,"transport edge%0d wall%0d A%h B%h",edges,wall,a_control,b_control);
 if(av&&(a_member!==b_member||a_tap!==b_tap))$fatal(1,"valid payload/tap edge%0d",edges);
 if(!reset&&!af)begin
  if(a_count!==gc[4:0])$fatal(1,"independent old count");
  if(av&&(gc==0||a_member!==gold[gh]||a_tap!==first(remaining[gh])))$fatal(1,"independent member/tap edge%0d head%0d",edges,gh);
  if(av&&!(remaining[gh]&(1<<a_tap)))$fatal(1,"inactive selected tap");
 end
end endtask
task reset_core;begin
 reset=1;ce=0;in_valid=0;out_ready=0;gc=0;gh=0;gt=0;tick;tick;reset=0;#2;check;
end endtask
task scoreboard;begin
 old_count=gc;
 if(old_count==16)begin full=full+1;if(ar||a_write)$fatal(1,"same-edge ACK finances full put");end
 if(a_read&&a_write&&a_rr==a_wr)$fatal(1,"old fetch/write collision");
 if(a_ack&&a_write)ack_put=ack_put+1;
 if(a_read&&a_ack)fetch_ack=fetch_ack+1;
 if(!ce&&av)ce0_valid=ce0_valid+1;
 if(av&&!out_ready)held=held+1;
 if(a_capture)begin
  if(a_ack!==((remaining[gh] & ~(1<<a_tap))==0))$fatal(1,"independent final ACK");
  outputs=outputs+1;remaining[gh]=remaining[gh]&~(1<<a_tap);
  if(a_ack)begin gc=gc-1;gh=(gh+1)%16;acks=acks+1;end
 end
 if(a_write)begin
  if(a_wr!==gt[3:0])$fatal(1,"independent write row");
  gold[gt]=in_member;remaining[gt]=in_member[39:36];gc=gc+1;gt=(gt+1)%16;sent=sent+1;
  if(gt==0)wraps=wraps+1;
 end
end endtask
task run_case;input integer total,base;begin:bounded
 sent=0;
 for(wall=0;wall<12000;wall=wall+1)begin
  ce=(wall%7!=3)&&(wall%17!=10);in_valid=sent<total&&wall%9!=2;
  out_ready=wall>100&&wall%13!=2&&wall%13!=3;
  in_member=member(base+sent);#2;check;scoreboard;tick;check;
  if(sent==total&&gc==0&&a_loaded==0&&!av)disable bounded;
 end
 $fatal(1,"Work wall watchdog");
end endtask
initial begin
 reset_core;run_case(512,0);
 // Reuse the same unreset payload RAM with sixteen active rows, then reset.
 sent=0;out_ready=0;
 for(wall=0;wall<160;wall=wall+1)begin
  ce=wall%5!=2;in_valid=sent<16;in_member=member(10000+sent);
  #2;check;scoreboard;tick;check;
 end
 if(gc!=16||a_loaded!=2)$fatal(1,"active full reset not reached");
 reset_core;run_case(256,20000);
 // Invalid emit retains the checked terminal behavior; failed put does not write.
 ce=1;in_valid=1;out_ready=0;in_member=member(30000);in_member[39:36]=0;#2;check;tick;check;
 if(!af||a_count!=0)$fatal(1,"malformed emit fault");
 in_valid=0;ce=0;repeat(7)begin #2;check;tick;check;end
 reset_core;run_case(64,40000);
 if(full==0||ack_put==0||fetch_ack==0||ce0_valid==0||held==0||wraps<40)$fatal(1,"Work coverage");
 $display("PASS Work edges=%0d outputs=%0d ACKs=%0d full=%0d ACK+put=%0d fetch+ACK=%0d CE0valid=%0d held=%0d wraps=%0d",edges,outputs,acks,full,ack_put,fetch_ack,ce0_valid,held,wraps);$finish;
end
initial begin #400000;$fatal(1,"Work time watchdog");end
endmodule
"#;
