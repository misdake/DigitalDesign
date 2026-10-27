"""Bounded prefetch queue, old credit, service recheck and lifecycle qualification."""
from pathlib import Path
import argparse,json,os
import framebuffer_tdp as common
R,S,O=common.R,common.S,common.O

def main():
 ap=argparse.ArgumentParser(__doc__);ap.add_argument('--tag',default='prefetch-lifecycle-v1');a=ap.parse_args();out=O/a.tag;out.mkdir(parents=True,exist_ok=False);reports=[]
 for mode in ['portable','native']:
  d=out/mode;d.mkdir();source='\n'.join((S/n).read_text() for n in ['framebuffer_tdp_cache.v','framebuffer_tdp_leaf.v','framebuffer_tdp_bank.v']);(d/'dut.v').write_text(('`define GPU_VLIW_POOL_PORTABLE\n' if mode=='portable' else '`define NATIVE\n')+source);(d/'tb.v').write_text(TB)
  cmd=[os.environ['IVERILOG_EXE'],'-g2012','-s','tb','-o',d/'sim.vvp']
  if mode=='native':cmd += [Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v']
  common.run(cmd+[d/'dut.v',d/'tb.v'],d,d/'compile.log');log=common.run([os.environ['VVP_EXE'],d/'sim.vvp'],d,d/'run.log')
  import subprocess
  trace=(d/'trace.txt').read_bytes()
  bad=subprocess.run([os.environ['VVP_EXE'],str(d/'sim.vvp'),'+BAD_ACK'],cwd=d,capture_output=True,timeout=30)
  (d/'bad-ack.log').write_bytes(bad.stdout+bad.stderr)
  assert bad.returncode!=0 and b'framebuffer completion before payload' in bad.stdout,(bad.stdout+bad.stderr).decode()
  (d/'trace.txt').write_bytes(trace)
  print(mode,log);reports.append(dict(mode=mode,log=log,trace_sha256=common.SHA(d/'trace.txt'),dut_sha256=common.SHA(d/'dut.v')))
 assert reports[0]['trace_sha256']==reports[1]['trace_sha256'];(out/'receipt.json').write_text(json.dumps(dict(status='pass',reports=reports,sources={str(p.relative_to(R)):common.SHA(p) for p in [S/'framebuffer_tdp_cache.v',S/'framebuffer_tdp_leaf.v',S/'framebuffer_tdp_bank.v',Path(__file__)]}),indent=2))
TB=r'''`timescale 1ns/1ps
module tb;
`ifdef NATIVE
GSR GSR(.GSRI(1'b1));
`endif
reg clk=0,reset=1,ce=0,commit_allow=1,shade_phase=0;
reg prefetch_valid=0,prefetch_abort=0;reg[9:0]prefetch_tile=0;
reg row_valid=0;reg[8:0]row_tile=0,row_x=0;reg[7:0]row_y=0;reg[3:0]row_mask=15;reg[2:0]row_index=0;reg[31:0]row_data=0;
wire row_accept,row_ready,preflight_bad,commit_valid,fault,idle,flush_complete;
wire[8:0]commit_x;wire[7:0]commit_y;wire[3:0]commit_mask;
reg flush_request=0;wire[53:0]shared_rop_product=0;wire shared_rop_offer,shared_rop_ce;wire[26:0]shared_rop_a;wire[7:0]shared_rop_b;
wire mem_req_valid,mem_req_write;wire[31:0]mem_req_addr;reg mem_req_ready=0;reg[63:0]mem_read_data=0;reg mem_read_valid=0;
wire mem_write_valid;wire[63:0]mem_write_data;reg mem_write_ready=0,mem_complete_valid=0,mem_complete_ok=1;
gpu_v2_framebuffer_tdp #(.PREFETCH_ENABLE(1),.PREFETCH_DIRTY(0)) dut(.*,.depth_func(3'd7),.depth_write(1'b1),.blend(1'b0));
reg race_arm=0,race_seen=0;
integer wall=0,requests=0,pending=0,beat=0,fd,n,i;reg enable_req=1,force_complete=0,washeld=0;reg[32:0]held;reg[1:0]calold;
task tick;begin
 mem_req_ready=enable_req&&pending==0;mem_read_valid=pending==1;mem_complete_valid=(pending==2)||force_complete;mem_complete_ok=1;mem_read_data=0;
 if(race_arm&&mem_complete_valid&&dut.mt_active&&!dut.mt_write&&dut.mt_retire==3'd3)begin row_valid=1;ce=1;end
 #2;
 if(race_arm&&dut.row0_fire&&mem_complete_valid&&dut.mt_retire==3'd3)race_seen=1;
 if(washeld&&{mem_req_write,mem_req_addr}!==held)$fatal(1,"presented descriptor changed");
 washeld=mem_req_valid&&!mem_req_ready;held={mem_req_write,mem_req_addr};calold=dut.cal;
 if(mem_req_valid&&mem_req_ready)begin if(mem_req_write)$fatal(1,"clean hint wrote back");pending=1;beat=0;requests=requests+1;end
 else if(mem_read_valid)begin beat=beat+1;if(beat==16)pending=2;end
 if(mem_complete_valid)pending=0;
 $fdisplay(fd,"T %0d %0d %0d %0d %0d %0d %0d %0d",wall,dut.hint_count,dut.mt_active,dut.mt_issue,mem_req_valid,mem_req_ready,mem_read_valid,fault);
 clk=1;#2;clk=0;#1;wall=wall+1;
 if(!reset&&!ce&&dut.cal!==calold)$fatal(1,"CE0 cal changed");
 if(ce)shade_phase=!shade_phase;
 if(wall>2500)$fatal(1,"bounded timeout");
end endtask
task restart;begin
 reset=1;prefetch_valid=0;prefetch_abort=0;row_valid=0;ce=0;shade_phase=0;flush_request=0;enable_req=1;force_complete=0;pending=0;washeld=0;requests=0;tick;reset=0;
 if(dut.hint_count!=0||dut.mt_active||!idle)$fatal(1,"reset state");
end endtask
task hint;input[9:0]v;begin prefetch_valid=1;prefetch_tile=v;tick;prefetch_valid=0;end endtask
initial begin
 fd=$fopen("trace.txt","w");restart;
 // Wall-clock enqueue and full-drop while the first descriptor is held.
 enable_req=0;hint(0);tick;tick;
 if(!mem_req_valid)$fatal(1,"missing held descriptor");
 hint(1);hint(2);hint(3);
 if(dut.hint_count!=2||dut.hint0!=1||dut.hint1!=2)$fatal(1,"old-state lossy capacity");
 prefetch_abort=1;tick;
 if(dut.hint_count!=0||!mem_req_valid)$fatal(1,"abort discarded presented request");
 enable_req=1;
 for(n=0;n<40&&dut.mt_active;n=n+1)tick;
 if(dut.mt_active||requests!=1)$fatal(1,"abort terminal drain");
 for(i=0;i<$size(dut.tvalid);i=i+1)if(dut.tvalid[i])$fatal(1,"partial prefetch published");
 restart;
 // Hint queued simultaneously with demand fill becomes resident before service.
 row_valid=1;ce=1;hint(0);ce=0;
 for(n=0;n<220&&dut.mt_active;n=n+1)tick;
 row_valid=0;tick;tick;
 if(requests!=4||dut.hint_count!=0||!idle)$fatal(1,"stale queued hint relaunched resident tile");
 restart;
 // Flush drops queued hints, rejects new hints, and drains current clean fill.
 hint(0);tick;tick;hint(1);hint(2);flush_request=1;hint(3);
 if(dut.hint_count!=0)$fatal(1,"flush left queued hints");
 for(n=0;n<220&&!flush_complete;n=n+1)tick;
 if(!flush_complete||requests!=4||!idle)$fatal(1,"flush did not drain");
 restart;
 hint(0);tick;tick;tick;hint(1);
 // Reset while a transport is in flight discards every local owner/queue.
 restart;
 // Early success is a sideband protocol assertion. Run the injection separately
 // so a simulation-only stop never supplies release datapath correctness.
 if($test$plusargs("BAD_ACK"))begin
  hint(2);tick;tick;tick;force_complete=1;tick;
  $fatal(1,"missing premature completion monitor");
 end
 // Two real four-transaction fills: consuming the old protected line must
 // not clear the new line's protection on its final depth ACK edge.
 hint(0);
 for(n=0;n<240&&!dut.prefetch_protect_valid;n=n+1)tick;
 if(!dut.prefetch_protect_valid||dut.prefetch_protect_line!=0||requests!=4)$fatal(1,"first race fill did not publish");
 hint(1);race_arm=1;
 for(n=0;n<240&&!race_seen;n=n+1)tick;
 if(!race_seen||requests!=8||dut.mt_active||!dut.tvalid[1]||dut.tag[1]!=1)$fatal(1,"second race publication not reached");
 if(!dut.ctx_valid[0]||dut.ctx_line[0]!=0)$fatal(1,"old protected row0 not actually accepted");
 if(!dut.prefetch_protect_valid||dut.prefetch_protect_line!=1)$fatal(1,"new protection lost to old consumption");
 $display("PASS prefetch lifecycle wall=%0d",wall);$fclose(fd);$finish;
end
initial begin #20000;$fatal(1,"absolute timeout");end
endmodule
'''
if __name__=='__main__':main()
