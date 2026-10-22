"""Two-credit framebuffer router: overlapping supply and terminal edges."""
from pathlib import Path
import hashlib,json,os,subprocess,argparse
R=Path(__file__).resolve().parents[3]
p=argparse.ArgumentParser(__doc__);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
o=a.output.resolve();o.mkdir(parents=True,exist_ok=False)
s=R/'ip/gpu-v2/src/system/vliw_memory.v';(o/'dut.v').write_text(s.read_text().split('// Standalone factory:')[0])
(o/'tb.v').write_text(r'''`timescale 1ns/1ps
module tb;
reg clk=0,reset=1,framebuffer_request_valid=0,framebuffer_request_write=1;
reg[31:0]framebuffer_request_address='h100000;
reg framebuffer_write_valid=1;reg[63:0]framebuffer_write_data=0;
wire framebuffer_request_ready,framebuffer_write_ready,framebuffer_read_valid,framebuffer_complete,framebuffer_ok,busy,fault;
wire[63:0]framebuffer_read_data;
reg fb_w_request_ready=1,fb_w_data_ready=0,fb_w_response_valid=0,fb_w_response_last=1,fb_w_response_error=0;
reg fb_r_request_ready=1,fb_r_response_valid=0,fb_r_response_last=0,fb_r_response_error=0;
reg[63:0]fb_r_data=0;
wire fb_w_request_valid,fb_r_request_valid;wire[21:0]fb_w_address,fb_r_address;wire[1:0]fb_w_count,fb_r_count;wire[63:0]fb_w_data;
gpu_vliw_memory_router dut(.clk(clk),.reset(reset),.framebuffer_request_valid(framebuffer_request_valid),.framebuffer_request_write(framebuffer_request_write),.framebuffer_request_address(framebuffer_request_address),.framebuffer_request_ready(framebuffer_request_ready),.framebuffer_write_valid(framebuffer_write_valid),.framebuffer_write_data(framebuffer_write_data),.framebuffer_write_ready(framebuffer_write_ready),.framebuffer_read_valid(framebuffer_read_valid),.framebuffer_read_data(framebuffer_read_data),.framebuffer_complete(framebuffer_complete),.framebuffer_ok(framebuffer_ok),.fb_w_request_valid(fb_w_request_valid),.fb_w_request_ready(fb_w_request_ready),.fb_w_address(fb_w_address),.fb_w_count(fb_w_count),.fb_w_data(fb_w_data),.fb_w_data_ready(fb_w_data_ready),.fb_w_response_valid(fb_w_response_valid),.fb_w_response_last(fb_w_response_last),.fb_w_response_error(fb_w_response_error),.fb_r_request_valid(fb_r_request_valid),.fb_r_request_ready(fb_r_request_ready),.fb_r_address(fb_r_address),.fb_r_count(fb_r_count),.fb_r_response_valid(fb_r_response_valid),.fb_r_response_last(fb_r_response_last),.fb_r_response_error(fb_r_response_error),.fb_r_data(fb_r_data),.busy(busy),.fault(fault),
 .geometry_request_valid(1'b0),.geometry_request_address(22'd0),.geometry_request_count(2'd0),
 .control_request_valid(1'b0),.control_request_address(22'd0),.control_request_count(2'd0),
 .texture_request_valid(1'b0),.texture_request_address(32'd0),
 .ro_request_ready(1'b0),.ro_response_valid(1'b0),.ro_response_last(1'b0),.ro_response_error(1'b0),.ro_data(64'd0));
integer cycle=0,requests=0,terms=0,words=0,i;
task tick;begin
 #2;
 if(!reset)begin
  if(framebuffer_request_valid&&framebuffer_request_ready)requests=requests+1;
  if(framebuffer_write_ready||(framebuffer_read_valid&&!fb_r_response_error))words=words+1;
  if(framebuffer_complete)begin if(!framebuffer_ok)$fatal(1,"bad legitimate terminal");terms=terms+1;end
  if(fault)$fatal(1,"router fault");
 end
 clk=1;#2;clk=0;#1;cycle=cycle+1;
 if(cycle>150)$fatal(1,"router watchdog");
end endtask
task request;begin
 framebuffer_request_valid=1;#1;
 if(!framebuffer_request_ready)$fatal(1,"missing request credit");
 tick;framebuffer_request_valid=0;framebuffer_request_address=framebuffer_request_address+128;
end endtask
initial begin
 tick;reset=0;request;request;
 if(framebuffer_request_ready)$fatal(1,"more than two credits");
 fb_w_data_ready=1;
 for(i=0;i<32;i=i+1)begin
  framebuffer_write_data=i;fb_w_response_valid=(i==15||i==31);tick;
 end
 fb_w_data_ready=0;fb_w_response_valid=0;
 if(busy||requests!=2||terms!=2||words!=32)$fatal(1,"write conservation");
 request;
 fb_w_data_ready=1;
 for(i=0;i<16;i=i+1)tick;
 fb_w_data_ready=0;repeat(3)tick;fb_w_response_valid=1;tick;fb_w_response_valid=0;
 if(busy)$fatal(1,"delayed terminal did not drain");
 framebuffer_request_write=0;request;request;
 fb_r_response_valid=1;
 for(i=0;i<32;i=i+1)begin
  fb_r_response_last=i==15||i==31;fb_r_data=64'h1234567800000000+i;
  #1;if(!framebuffer_read_valid||framebuffer_read_data!==fb_r_data)$fatal(1,"read stream");tick;
 end
 fb_r_response_valid=0;tick;
 if(fault||busy||requests!=5||terms!=5||words!=80)$fatal(1,"router conservation");
 $display("PASS router: two credits, same-edge and delayed write terminal, continuous reads, phase drain");$finish;
end
endmodule
''')
logs=[]
for n,cmd in [('compile',[os.environ['IVERILOG_EXE'],'-g2012','-s','tb','-o',o/'sim.vvp',o/'dut.v',o/'tb.v']),('run',[os.environ['VVP_EXE'],o/'sim.vvp'])]:
 z=subprocess.run(list(map(str,cmd)),cwd=o,capture_output=True,timeout=30);(o/(n+'.log')).write_bytes(z.stdout+z.stderr)
 assert z.returncode==0,(z.stdout+z.stderr).decode(errors='replace')[-1500:]
 logs+=[l for l in z.stdout.decode().splitlines() if l.startswith('PASS')]
receipt=dict(status='pass',cases=logs,source_sha256=hashlib.sha256(s.read_bytes()).hexdigest(),test_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest())
(o/'receipt.json').write_text(json.dumps(receipt,indent=2));print(json.dumps(receipt))
