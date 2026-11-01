"""Actual command transport diagnostics, ownership and generic checked controls."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

TB = r'''
`timescale 1ns/1ps
module tb;
reg clk=0,reset=1;
reg geometry_request_valid=0,control_request_valid=0,texture_request_valid=0;
reg[21:0]geometry_request_address='h1000,control_request_address='h2000;
reg[1:0]geometry_request_count=0,control_request_count=0;
reg[31:0]texture_request_address='h10000;
wire geometry_request_ready,geometry_response_valid,geometry_response_last,geometry_response_error,geometry_response_bad;
wire[63:0]geometry_response_data,control_response_data,texture_read_data;
wire control_request_ready,control_response_valid,control_response_last,control_response_error;
wire texture_request_ready,texture_read_valid,texture_complete,texture_ok;wire[3:0]texture_read_index;
reg framebuffer_request_valid=0,framebuffer_request_write=0,framebuffer_write_valid=1;
reg[31:0]framebuffer_request_address='h100000;reg[63:0]framebuffer_write_data=64'h12345678;
wire framebuffer_request_ready,framebuffer_write_ready,framebuffer_read_valid,framebuffer_complete,framebuffer_ok;
wire[63:0]framebuffer_read_data;
wire ro_request_valid;reg ro_request_ready=1;wire[21:0]ro_address;wire[1:0]ro_count;
reg ro_response_valid=0,ro_response_last=0,ro_response_error=0;reg[63:0]ro_data=0;
wire fb_r_request_valid,fb_w_request_valid;wire[21:0]fb_r_address,fb_w_address;wire[1:0]fb_r_count,fb_w_count;wire[63:0]fb_w_data;
reg fb_r_request_ready=1,fb_w_request_ready=1,fb_w_data_ready=0;
reg fb_r_response_valid=0,fb_r_response_last=0,fb_r_response_error=0;
reg fb_w_response_valid=0,fb_w_response_last=0,fb_w_response_error=0;reg[63:0]fb_r_data=64'hf00d;
wire busy,fault;
`ifndef TEST_CONTROL
 `define TEST_CONTROL 0
`endif
gpu_vliw_memory_router #(.CONTROL(`TEST_CONTROL)) router(.*);
reg ce=0,stop=0,work_valid=0,dresponse_valid=0,dresponse_last=0,dresponse_error=0,dresponse_bad=0;
wire work_ready,didle,dfault,dma_we,work_done,drequest_valid;wire[8:0]dma_addr;wire[63:0]dma_data;
gpu_command_dma dma(.clk(clk),.reset(reset),.ce(ce),.stop(stop),
 .cache_valid(1'b0),.cache_source(22'd0),.cache_bank(1'b0),
 .work_valid(work_valid),.work_ready(work_ready),.work_mesh(1'b0),.work_source(22'h1000),
 .work_destination(7'd66),.work_size(2'd0),.work_blocks(3'd0),.work_slot(1'b0),.work_done(work_done),
 .request_valid(drequest_valid),.request_ready(1'b1),.response_valid(dresponse_valid),
 .response_last(dresponse_last),.response_error(dresponse_error),.response_bad(dresponse_bad),.response_data(64'hcafe),
 .dma_we(dma_we),.dma_addr(dma_addr),.dma_data(dma_data),.idle(didle),.fault(dfault));
integer kind=0,cycle=0,i,saved,mode=0;
integer trace_fd=0;reg[2047:0]trace_path;
initial if($value$plusargs("TRACE=%s",trace_path))trace_fd=$fopen(trace_path,"w");
always @(posedge clk)if(!reset&&trace_fd)$fdisplay(trace_fd,"%h %h %h %h %h %h %h %h %h %h %h",
 {geometry_request_valid,geometry_request_ready,texture_request_valid,texture_request_ready,control_request_valid,control_request_ready},
 {ro_request_valid,ro_request_ready,ro_response_valid,ro_response_last,ro_response_error,ro_address,ro_count},ro_data,
 {geometry_response_valid,geometry_response_last,geometry_response_bad,texture_read_valid,texture_read_index,texture_complete,texture_ok,control_response_valid,control_response_last},
 {router.present,router.offer_owner,router.rput,router.rhead,router.rb,router.ro_bad},
 {router.r_owner[0],router.r_owner[1],router.r_count[0],router.r_count[1],router.r_error[0],router.r_error[1]},
 {framebuffer_request_valid,framebuffer_request_ready,framebuffer_request_write,framebuffer_complete,framebuffer_ok},
 {fb_r_response_valid,fb_r_response_last,fb_r_response_error,fb_w_data_ready,fb_w_response_valid,fb_w_response_last,fb_w_response_error},
 {router.fused,router.f_error,router.fdata_done,router.fbeat,router.fb_bad,busy,fault},
 {ce,stop,work_valid,drequest_valid,dresponse_valid,dresponse_last,dresponse_error,dresponse_bad,dma_we,work_done,dfault,didle},
 {dma.put,dma.issued,dma.retired,dma.rp,dma.rh,dma.cursor});
task tick;begin
 #2;clk=1;#2;clk=0;#1;cycle=cycle+1;
 if(cycle>250)$fatal(1,"transport profile watchdog");
end endtask
task ro_request;
 input integer owner;
 begin
  case(owner)0:geometry_request_valid=1;1:texture_request_valid=1;2:control_request_valid=1;endcase
  tick;
  case(owner)
   0:if(!geometry_request_ready)$fatal(1,"geometry request not presented");
   1:if(!texture_request_ready)$fatal(1,"texture request not presented");
   2:if(!control_request_ready)$fatal(1,"control request not presented");
  endcase
  tick;geometry_request_valid=0;texture_request_valid=0;control_request_valid=0;
 end
endtask
task fb_request;input wr;begin
 framebuffer_request_write=wr;framebuffer_request_valid=1;#1;
 if(!framebuffer_request_ready)$fatal(1,"FB missing request credit");
 tick;framebuffer_request_valid=0;
end endtask
task ro_beat;input last_bit,error_bit;begin
 ro_response_valid=1;ro_response_last=last_bit;ro_response_error=error_bit;ro_data=cycle;
 tick;ro_response_valid=0;ro_response_error=0;ro_response_last=0;
end endtask
task fb_read;input last_bit,error_bit;begin
 fb_r_response_valid=1;fb_r_response_last=last_bit;fb_r_response_error=error_bit;
 tick;fb_r_response_valid=0;fb_r_response_error=0;
end endtask
initial begin
 if($value$plusargs("KIND=%d",kind))begin end
 if($value$plusargs("COMMAND=%d",mode))begin end
 tick;tick;reset=0;tick;
 case(kind)
 0,1:begin
  ro_request(0);
  if(kind==1)repeat(3)ro_beat(0,0);
  ro_beat(kind==1,1);
  if(mode)begin if(fault||router.rused!=(kind==1?0:1))$fatal(1,"command peer error invented terminal/fault");end
  else if(!fault||router.rused!=0)$fatal(1,"generic RO error did not terminate/fault");
 end
 2:begin
  fb_request(0);fb_read(0,1);
  if(mode)begin if(fault||router.fused!=1)$fatal(1,"command FB error invented terminal");end
  else if(!fault||router.fused!=0)$fatal(1,"generic FB read error contract");
 end
 3:begin
  fb_request(1);fb_w_response_valid=1;fb_w_response_error=1;fb_w_response_last=0;tick;fb_w_response_valid=0;
  if(mode)begin if(fault||router.fused!=1)$fatal(1,"command write error invented terminal");end
  else if(!fault||router.fused!=0)$fatal(1,"generic FB write error contract");
 end
 4:begin ro_beat(0,0);if(router.rused!=0||fault!==!mode)$fatal(1,"orphan RO contract");end
 5:begin fb_read(0,0);if(router.fused!=0||fault!==!mode)$fatal(1,"orphan FB contract");end
 6:begin fb_request(1);fb_read(0,0);if(router.fused!=1||fault!==!mode)$fatal(1,"wrong direction owner changed");end
 7:begin ro_request(0);ro_beat(1,0);if(router.rused!=0||fault!==!mode)$fatal(1,"early LAST contract");end
 8,9:begin
  ro_request(0);repeat(4)ro_beat(0,0);
  repeat(3)begin tick;if(router.rused!=1||!busy||fault!==!mode)$fatal(1,"missing LAST invented progress");end
  if(kind==9)begin ro_beat(1,0);if(router.rused!=0)$fatal(1,"late LAST did not retire real owner");end
 end
 10:begin fb_request(0);fb_read(1,0);if(router.fused!=0||fault!==!mode)$fatal(1,"FB early LAST contract");end
 11:begin
  fb_request(1);fb_w_response_valid=1;fb_w_response_last=1;tick;fb_w_response_valid=0;
  if(router.fused!=0||fault!==!mode)$fatal(1,"premature ACK contract");
 end
 12:begin
  fb_request(1);framebuffer_write_valid=0;fb_w_data_ready=1;tick;fb_w_data_ready=0;
  if(router.fused!=1||router.fbeat!=1||fault!==!mode)$fatal(1,"underprovided write contract");
 end
 13:begin
  framebuffer_request_address='h100001;framebuffer_request_valid=1;tick;framebuffer_request_valid=0;
  if(mode)begin if(router.f_error!=0||fault)$fatal(1,"command local FB reject appeared");end
  else if(!fault||!router.f_error[0])$fatal(1,"generic invalid FB was not rejected");
 end
 14,15:begin
  if(kind==14)geometry_request_count=2;else texture_request_address='h10001;
  ro_request(kind==14?0:1);
  if(mode)begin if(fault||router.r_error[0]!=0)$fatal(1,"command local RO reject appeared");end
  else if(!fault||!router.r_error[0])$fatal(1,"generic invalid RO was not rejected");
 end
 16,17,18:begin
  ce=1;work_valid=1;tick;work_valid=0;ce=0;repeat(3)tick;
  if(dma.returns!=1)$fatal(1,"DMA setup did not own response");
  if(kind==18)repeat(3)begin dresponse_valid=1;tick;dresponse_valid=0;end
  dresponse_valid=1;dresponse_error=kind==16;dresponse_bad=kind!=16;dresponse_last=kind==18;
  #1;if(dma_we!==mode[0])$fatal(1,"DMA error write policy");
  tick;dresponse_valid=0;
  if(dfault!==!mode||dma.returns!=((kind==18||(!mode&&kind==16))?0:1))$fatal(1,"DMA error/terminal contract");
  if(kind==18&&work_done!==mode[0])$fatal(1,"DMA terminal publication contract");
 end
 19:begin
  ro_request(1);repeat(16)ro_beat(0,0);
  ro_response_valid=1;#1;
  if(texture_read_valid!==mode[0])$fatal(1,"texture diagnostic read-window gate");
  tick;ro_response_valid=0;
  if(router.rused!=1||fault!==!mode)$fatal(1,"overrun owner/fault contract");
 end
 20:begin
  ro_request(2);repeat(3)begin if(!control_response_valid&&ro_response_valid)$fatal(1,"control owner");ro_beat(0,0);end
  ro_response_valid=1;ro_response_last=1;#1;
  if(!control_response_valid||!control_response_last||geometry_response_valid||texture_read_valid)$fatal(1,"control response ownership");
  tick;ro_response_valid=0;if(busy||fault)$fatal(1,"control did not drain");
 end
 22:begin
  fb_request(0);repeat(15)fb_read(0,0);fb_read(1,1);
  if(router.fused!=0||fault!==!mode)$fatal(1,"read error at real LAST contract");
 end
 23:begin
  fb_request(1);fb_w_data_ready=1;repeat(15)tick;
  fb_w_response_valid=1;fb_w_response_last=1;fb_w_response_error=1;tick;
  fb_w_response_valid=0;fb_w_data_ready=0;
  if(router.fused!=0||fault!==!mode)$fatal(1,"write error at real LAST contract");
 end
 24:begin
  ce=1;work_valid=1;tick;work_valid=0;ce=0;repeat(3)tick;
  dresponse_valid=1;repeat(3)tick;
  dresponse_last=1;dresponse_error=1;tick;dresponse_valid=0;
  if(dma.returns!=0||dfault!==!mode||work_done!==mode[0])$fatal(1,"DMA error at real LAST contract");
 end
 21:begin
  ro_request(0);ro_request(1);
  geometry_request_count=1;geometry_request_address='h1080;geometry_request_valid=1;
  repeat(3)begin tick;if(router.rused!=2||geometry_request_ready||ro_request_valid)$fatal(1,"full credit borrowed");end
  repeat(3)ro_beat(0,0);
  ro_response_valid=1;ro_response_last=1;#1;
  if(geometry_request_ready)$fatal(1,"full credit borrowed same-edge pop");tick;ro_response_valid=0;ro_response_last=0;
  ro_request_ready=0;tick;
  if(!ro_request_valid||router.owner!=1)$fatal(1,"held third offer/texture owner");
  for(i=0;i<16;i=i+1)begin
   ro_response_valid=1;ro_response_last=i==15;ro_data=64'hff0000+i;ro_request_ready=i==15;#1;
   if(!texture_read_valid||texture_read_index!=i||texture_read_data!=ro_data||geometry_response_valid)$fatal(1,"texture owner/data");
   if(i==15&&!geometry_request_ready)$fatal(1,"accept plus terminal not exercised");
   tick;
  end
  ro_response_valid=0;ro_response_last=0;geometry_request_valid=0;
  if(router.rused!=1||router.owner!=0||router.rb!=0)$fatal(1,"simultaneous accept/pop ownership");
  repeat(7)ro_beat(0,0);ro_beat(1,0);
  if(busy||fault)$fatal(1,"mixed RO did not drain");
 end
 endcase
 if(mode&&(fault||dfault))$fatal(1,"command diagnostic fed back");
 $display("PASS transport kind=%0d command=%0d cycles=%0d",kind,mode,cycle);$finish;
end
endmodule
'''


def main():
    p = argparse.ArgumentParser(__doc__)
    p.add_argument('--export', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--generic', action='store_true')
    p.add_argument('--dma', type=Path, help='Generic DMA source, required with --generic')
    args = p.parse_args()
    out = args.output.resolve(); out.mkdir(parents=True, exist_ok=False)
    sources = [args.export.resolve()]
    if args.generic:
        assert args.dma
        sources.append(args.dma.resolve())
    (out/'tb.v').write_text(TB)
    receipt = []
    for monitors in (True, False):
        for control in ((0, 1) if args.generic else (0,)):
            name = f'{"on" if monitors else "off"}-control{control}'
            sim = out/f'{name}.vvp'
            command = [os.environ['IVERILOG_EXE'], '-g2012', '-s', 'tb', f'-DTEST_CONTROL={control}',
                       *([] if monitors else ['-DSYNTHESIS']), '-o', str(sim), *map(str, sources), str(out/'tb.v')]
            compile_result = subprocess.run(command, capture_output=True, timeout=30)
            (out/f'{name}-compile.log').write_bytes(compile_result.stdout+compile_result.stderr)
            assert compile_result.returncode == 0, compile_result.stderr.decode(errors='replace')[-2000:]
            for kind in list(range(20))+([20] if control else [])+[21,22,23,24]:
                trace = out/f'{name}-kind{kind}.trace'
                z = subprocess.run([os.environ['VVP_EXE'], str(sim), f'+KIND={kind}',
                                    f'+COMMAND={int(not args.generic)}', f'+TRACE={trace}'], capture_output=True, timeout=20)
                log = (z.stdout+z.stderr).decode(errors='replace')
                (out/f'{name}-kind{kind}.log').write_text(log)
                negative = not args.generic and monitors and kind not in (20, 21)
                if negative:
                    assert z.returncode != 0 and 'GPU contract:' in log, (name, kind, log[-2000:])
                else:
                    assert z.returncode == 0 and 'PASS transport' in log, (name, kind, log[-2000:])
                receipt.append(dict(profile=name, kind=kind, expected_monitor=negative,
                                    trace_sha256=hashlib.sha256(trace.read_bytes()).hexdigest()))
    result = dict(status='pass', generic=args.generic,
                  sources={str(s): hashlib.sha256(s.read_bytes()).hexdigest() for s in sources},
                  test_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), cases=receipt)
    (out/'receipt.json').write_text(json.dumps(result, indent=2))
    print(json.dumps(dict(status='pass', generic=args.generic, cases=len(receipt))))


if __name__ == '__main__':
    main()

