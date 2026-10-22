"""Actual command DMA plus memory-router framing, cursor and drain checks."""
from pathlib import Path
import argparse, hashlib, json, os, subprocess

ROOT = Path(__file__).resolve().parents[3]
SOURCES = [ROOT / 'ip/gpu-v2/src/system/command_dma.v', ROOT / 'ip/gpu-v2/src/system/vliw_memory.v']
TB = r'''
`timescale 1ns/1ps
module tb;
reg clk=0,reset=1,ce=1,stop=0;
always #5 clk=~clk;
reg cache_valid=0,cache_bank=1,work_valid=0,work_mesh=0,work_slot=0;
reg[21:0]cache_source=22'h3000,work_source=0;
reg[6:0]work_destination=66;reg[1:0]work_size=2;reg[2:0]work_blocks=0;
wire cache_ready,cache_done,cache_done_bank,work_ready,work_busy,work_done,mesh_begin,mesh_done,mesh_slot;
wire mesh_begin_slot,mesh_done_slot;
wire uniform_done,mesh_loading,idle,fault,dma_we;wire[1:0]uniform_bank;
wire[8:0]dma_addr;wire[63:0]dma_data;
wire request_valid,request_ready,response_valid,response_last,response_error,response_bad;
wire[21:0]request_address;wire[1:0]request_count;wire[63:0]response_data;
gpu_command_dma dma(.*);
wire ro_request_valid,router_busy,router_fault;wire[21:0]ro_address;wire[1:0]ro_count;
reg ro_request_ready=0,ro_response_valid=0,ro_response_last=0,ro_response_error=0;
reg[63:0]ro_data=0;
gpu_vliw_memory_router router(.clk(clk),.reset(reset),
 .geometry_request_valid(request_valid),.geometry_request_ready(request_ready),
 .geometry_request_address(request_address),.geometry_request_count(request_count),
 .geometry_response_valid(response_valid),.geometry_response_last(response_last),
 .geometry_response_error(response_error),.geometry_response_bad(response_bad),.geometry_response_data(response_data),
 .control_request_valid(1'b0),.control_request_address(22'd0),.control_request_count(2'd0),
 .texture_request_valid(1'b0),.texture_request_address(32'd0),
 .framebuffer_request_valid(1'b0),.framebuffer_request_write(1'b0),.framebuffer_request_address(32'd0),
 .framebuffer_write_valid(1'b0),.framebuffer_write_data(64'd0),
 .ro_request_valid(ro_request_valid),.ro_request_ready(ro_request_ready),.ro_address(ro_address),.ro_count(ro_count),
 .ro_response_valid(ro_response_valid),.ro_response_last(ro_response_last),.ro_response_error(ro_response_error),.ro_data(ro_data),
 .fb_r_request_ready(1'b0),.fb_r_response_valid(1'b0),.fb_r_response_last(1'b0),.fb_r_response_error(1'b0),.fb_r_data(64'd0),
 .fb_w_request_ready(1'b0),.fb_w_data_ready(1'b0),.fb_w_response_valid(1'b0),.fb_w_response_last(1'b0),.fb_w_response_error(1'b0),
 .busy(router_busy),.fault(router_fault));
reg[63:0]expected[0:511];reg allowed[0:511],seen[0:511];
integer mode=0,cycle=0,pi=0,pn=0,pa=0,missing_hold=0,requests=0,terminals=0,writes=0;
integer cache_events=0,work_events=0,mesh_events=0,uniform_events=0,ce0writes=0,missing_checks=0;
integer job=0,k,n,target,source,expected_writes=0;
reg active=0,held=0,cache_injected=0,cache_accepted=0;
reg[23:0]old_request;
function[63:0]source_qword;
 input integer qword;
 begin source_qword=64'h9e3779b97f4a7c15^(64'h100000001*qword);end
endfunction
task expect_copy;
 input integer destination,source_halfwords,count;
 integer i;
 begin for(i=0;i<count;i=i+1)begin
  if(allowed[destination+i])$fatal(1,"overlapping expected copy");
  allowed[destination+i]=1;expected[destination+i]=source_qword(source_halfwords/4+i);expected_writes=expected_writes+1;
 end end
endtask
always @(negedge clk)begin
 ce=cycle%5!=2;
 ro_request_ready=!active&&cycle%4!=1&&(mode!=5||stop);
 ro_response_valid=active&&cycle%3!=1&&missing_hold==0;
 ro_response_last=pi==pn-1;ro_response_error=0;ro_data=source_qword(pa/4+pi);
 if(mode==1)ro_response_last=pi==2;
 if(mode==2)ro_response_last=pi==pn;
 if(mode==3&&pi==3)begin ro_response_error=1;ro_response_last=0;end
 if(mode==4&&active&&pi==3)stop=1;
 if(mode==5&&ro_request_valid&&!ro_request_ready)stop=1;
 if(cache_injected&&!cache_accepted)cache_valid=1;
 if(cache_accepted)cache_valid=0;
end
always @(posedge clk)begin
 cycle=cycle+1;if(cycle>5000)$fatal(1,"DMA/router cycle watchdog");
 if(!reset)begin
  if(held&&(!ro_request_valid||{ro_count,ro_address}!==old_request))$fatal(1,"held RO request changed");
  held=ro_request_valid&&!ro_request_ready;old_request={ro_count,ro_address};
  if(cache_valid&&cache_ready)cache_accepted=1;
  if(ro_request_valid&&ro_request_ready)begin
   if(active)$fatal(1,"duplicate peer owner");
   active=1;pa=ro_address;pi=0;pn=ro_count==3?16:ro_count==1?8:4;requests=requests+1;
   if(mode==0&&job==1)case(requests)
    1:if(pa!=22'h1000||pn!=16)$fatal(1,"local copy was split");
    2:if(pa!=22'h3000||pn!=16)$fatal(1,"cache did not take burst boundary");
   endcase
  end else if(ro_response_valid)begin
   if(mode==0&&job==1&&pi==1&&!cache_injected)begin cache_injected=1;end
   if(mode==2&&pi==pn-1)missing_hold=4;
   if(ro_response_last||ro_response_error)begin active=0;terminals=terminals+1;end
   else pi=pi+1;
  end
  if(missing_hold>0)begin
   if(!work_busy||idle||!active)$fatal(1,"missing LAST retired early");
   missing_checks=missing_checks+1;missing_hold=missing_hold-1;
  end
  if(dma_we)begin
   if(response_bad||response_error||stop||fault)$fatal(1,"bad/stopped beat wrote RAM");
   if(dma_addr>=512||!allowed[dma_addr]||seen[dma_addr]||dma_data!==expected[dma_addr])
    $fatal(1,"RAM cursor/data mismatch address=%0d got=%h expected=%h",dma_addr,dma_data,expected[dma_addr]);
   seen[dma_addr]=1;writes=writes+1;if(!ce)ce0writes=ce0writes+1;
  end
 end
 #1;
 if(!reset)begin
  if(cache_done)cache_events=cache_events+1;
  if(work_done)work_events=work_events+1;
  if(mesh_done)mesh_events=mesh_events+1;
  if(uniform_done)uniform_events=uniform_events+1;
 end
end
task tick;begin @(negedge clk);#1;end endtask
task launch;
 input integer source_halfwords,destination;
 input mesh_job;input[2:0]payloads_minus1;input[1:0]size;
 begin
  work_source=source_halfwords;work_destination=destination;work_mesh=mesh_job;work_blocks=payloads_minus1;
  work_slot=1;work_size=size;work_valid=1;
  while(!ce||!work_ready)tick;
  @(posedge clk);#2;work_valid=0;
 end
endtask
task drain;
 begin
  for(n=0;n<2000&&(!idle||router_busy||active||cache_valid);n=n+1)tick;
  if(!idle||router_busy||active||cache_valid)$fatal(1,"DMA/router did not drain to real terminal");
 end
endtask
initial begin
 k=$value$plusargs("MODE=%d",mode);
 for(k=0;k<512;k=k+1)begin allowed[k]=0;seen[k]=0;expected[k]=0;end
 tick;tick;reset=0;tick;
 if(mode==0)begin
  job=1;expect_copy(264,'h1000,16);expect_copy(464,'h3000,16);launch('h1000,66,0,0,2);drain;
  if(cache_events!=1||work_events!=1||requests!=2)$fatal(1,"cache insertion conservation");
  job=2;expect_copy(280,'h1020,8);launch('h1020,70,0,0,1);drain;
  job=3;expect_copy(320,'h1000,16);launch('h1000,80,0,0,2);drain;
  job=4;expect_copy(304,'h1010,4);launch('h1010,76,0,0,0);drain;
  job=5;expect_copy(260,'h2000,4);expect_copy(128,'h2040,32);launch('h2000,0,1,1,2);drain;
  if(work_events!=5||mesh_events!=1||uniform_events!=1||cache_events!=1||writes!=expected_writes||ce0writes==0||fault||router_fault)
   $fatal(1,"valid DMA conservation writes%0d expected%0d done%0d mesh%0d uniform%0d",writes,expected_writes,work_events,mesh_events,uniform_events);
  for(k=0;k<512;k=k+1)if(allowed[k]&&!seen[k])$fatal(1,"missing target RAM write %0d",k);
 end else begin
  expect_copy(264,'h1000,mode==1?2:mode==2?15:mode==3||mode==4?3:0);
  launch('h1000,66,0,0,2);drain;
  if(requests!=1||terminals!=1||writes!=expected_writes||cache_events||work_events||mesh_events||uniform_events)
   $fatal(1,"failed/stopped transaction published or escaped req%0d term%0d writes%0d expected%0d",requests,terminals,writes,expected_writes);
  if(mode<=3&&(!fault||!router_fault))$fatal(1,"framing/error did not fault");
  if(mode==2&&missing_checks<3)$fatal(1,"missing LAST hold not exercised");
 end
 $display("PASS DMA_STREAM mode=%0d cycles=%0d requests=%0d terminals=%0d writes=%0d cache=%0d work=%0d mesh=%0d uniform=%0d CE0writes=%0d missing_hold=%0d fault=%0d",
  mode,cycle,requests,terminals,writes,cache_events,work_events,mesh_events,uniform_events,ce0writes,missing_checks,fault);
 $finish;
end
endmodule
'''


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args(); out = args.output.resolve(); out.mkdir(parents=True, exist_ok=False)
    identities = {source.name: hashlib.sha256(source.read_bytes()).hexdigest() for source in SOURCES}
    for source in SOURCES: (out / source.name).write_bytes(source.read_bytes())
    (out / 'tb.v').write_text(TB)
    def run(name, command):
        with (out / (name + '.log')).open('w') as log:
            result = subprocess.run(list(map(str, command)), cwd=out, stdout=log, stderr=subprocess.STDOUT, timeout=20)
        assert result.returncode == 0, (out / (name + '.log')).read_text(errors='replace')[-1800:]
    run('compile', [os.environ['IVERILOG_EXE'], '-g2012', '-s', 'tb', '-o', out / 'sim.vvp',
                    *[out / source.name for source in SOURCES], out / 'tb.v'])
    cases = []
    for mode in range(6):
        run(f'run-{mode}', [os.environ['VVP_EXE'], out / 'sim.vvp', f'+MODE={mode}'])
        cases += [line for line in (out / f'run-{mode}.log').read_text().splitlines() if line.startswith('PASS ')]
    assert all(hashlib.sha256(source.read_bytes()).hexdigest() == identities[source.name] for source in SOURCES), 'live source changed'
    receipt = dict(status='pass', sources=identities, test_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), cases=cases)
    (out / 'receipt.json').write_text(json.dumps(receipt, indent=2)); print(json.dumps(receipt))


if __name__ == '__main__': main()
