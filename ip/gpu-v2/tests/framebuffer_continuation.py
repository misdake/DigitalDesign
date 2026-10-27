"""Bounded real framebuffer cache/router/native SDRAM continuation qualification.

Six or eight real cache bursts: cold color/depth refill, one shaded quad, and flush.
The full 512-byte color/depth footprint and guards are checked at the pins.
"""
from pathlib import Path
import argparse, hashlib, json, os, re, subprocess

ROOT = Path(__file__).resolve().parents[3]
PIXEL = ROOT/'ip/gpu-v2/src/system/pixel'
VENDOR = ROOT/'hardware/vendor/gowin/src/sdram_memory_controller/rtl'

def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()

def run(cmd, out, name):
    r = subprocess.run(list(map(str, cmd)), cwd=out, capture_output=True, timeout=90)
    (out/(name+'.log')).write_bytes(r.stdout+r.stderr)
    if r.returncode: raise RuntimeError((r.stdout+r.stderr).decode(errors='replace')[-2200:])
    return r.stdout.decode(errors='replace')

def main():
    p=argparse.ArgumentParser(__doc__)
    p.add_argument('--peer',type=Path,required=True)
    p.add_argument('--gpu',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--baseline',type=Path,help='Original cache/router sources; peer selected separately')
    p.add_argument('--pause',action='store_true')
    p.add_argument('--expect-chain',action='store_true')
    p.add_argument('--color-only',action='store_true')
    p.add_argument('--refresh-boundary',action='store_true')
    p.add_argument('--release',action='store_true',help='Use the actual production modules extracted from --gpu')
    a=p.parse_args();out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    peer=a.peer.resolve();sources=[]
    for n in ['bridge.v','combination.v','controller.v','cpu_v3_memory_arbiter.v','pins.v','shared_port.v','ports.vh']:
        d=out/n;d.write_bytes((peer/n).read_bytes());sources.append(d)
    if not a.baseline:
        for name,src in [('bridge.v','stream_bridge.v'),('shared_port.v','stream_sdram.v'),('controller.v','sdram_controller.v')]:
            text=(VENDOR/src).read_text()
            if name=='bridge.v': text=text.replace('.BANK_BIT(BANK_BIT)', '.BANK_BIT(BANK_BIT), .INIT_CYCLES(21600)')
            if name=='shared_port.v':text=text.replace('module SharedSdramPort','module cpu_v3_shared_sdram_port') if 'module cpu_v3_shared_sdram_port' in (out/name).read_text() else text
            (out/name).write_text(text)
        # Preserve the generated arbiter and replace the complete current
        # endpoint/bridge wiring. This catches stale interface connections.
        s=(out/'combination.v').read_text()
        start=s.index('wire request_valid, writing, write_valid')
        connection=(VENDOR/'stream_connections.vh').read_text()
        connection=connection.replace('__EARLY_GRANT__',"1'b1").replace('__CHAIN_GROUP_FOUR__',"1'b0").replace('__SHARED_PORT__','SharedSdramPort')
        (out/'combination.v').write_text(s[:start]+connection+'\nendmodule\n')
    gpu=a.gpu.read_text()
    def module(name):
        i=re.search(r'\bmodule\s+'+name+r'(?=\s*[(#])',gpu).start()
        return gpu[i:gpu.index('endmodule',i)+9]
    for n,m in [('framebuffer_tdp_cache.v','gpu_v2_framebuffer_tdp'),('framebuffer_tdp_bank.v','gpu_v2_framebuffer_tdp_bank'),('framebuffer_tdp_leaf.v','gpu_v2_rop_tdp')]:
        src=(a.baseline/n) if a.baseline and n=='framebuffer_tdp_cache.v' else PIXEL/n
        d=out/n;d.write_text(module(m) if a.release else src.read_text());sources.append(d)
    src=(a.baseline/'vliw_memory.v') if a.baseline else ROOT/'ip/gpu-v2/src/system/vliw_memory.v'
    d=out/'router.v';d.write_text(module('gpu_vliw_memory_router') if a.release else src.read_text().split('// Standalone factory:')[0]);sources.append(d)
    d=out/'shared.v';d.write_text(module('gpu_v2_final_rop_bank'));sources.append(d)
    tb=TB.replace('@PAUSE@',str(int(a.pause))).replace('@DW@',str(int(not a.color_only))).replace('@START@',str(480 if a.refresh_boundary else 0)).replace('@NWRITE@',str(2 if a.color_only else 4))
    if a.release:
        tb=tb.replace("#(.COLOR_BASE('h100000),.DEPTH_BASE('h140000),.PREFETCH_ENABLE(0))", "#(.PREFETCH_ENABLE(0))")
        tb=tb.replace('.preflight_bad(preflight_bad),', ".color_base(14'h800),.depth_base(14'ha00),.row_alpha(8'hff),.preflight_bad(preflight_bad),")
    if 'input ui_active' in (out/'framebuffer_tdp_cache.v').read_text():
        ui_gate=".ui_lower_ready(1'b1)," if 'ui_lower_ready' in (out/'framebuffer_tdp_cache.v').read_text() else ".ui_cmd_half(1'b0),"
        tb=tb.replace('.clk(clk),.reset(reset),.ce(ce),.commit_allow',
            ".ui_active(1'b0),.ui_cmd_valid(1'b0),.ui_cmd_write(1'b0),"+ui_gate+".ui_tile(9'd0),.ui_pixel_valid(1'b0),.ui_pixel_half(1'b0),.ui_pixel_word(5'd0),.ui_pixels(64'd0),.clk(clk),.reset(reset),.ce(ce),.commit_allow",1)
    (out/'tb.v').write_text(tb)
    compile_files=[p for p in sources if p.suffix=='.v']
    run([os.environ['IVERILOG_EXE'],'-g2012','-s','tb','-o',out/'sim.vvp',
         Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v',*compile_files,out/'tb.v'],out,'compile')
    log=run([os.environ['VVP_EXE'],out/'sim.vvp'],out,'run')
    summary=[s for s in log.splitlines() if s.startswith(('PASS','TIMING'))]
    assert any(s.startswith('PASS') for s in summary),log[-1500:]
    if a.expect_chain:
        assert f'read_chain=3 write_chain={1 if a.color_only else 3}' in log,log[-1800:]
        assert 'read_column_min=32 read_column_max=32 write_column_min=32 write_column_max=32' in log,log[-1800:]
    receipt=dict(status='pass',summary=summary,pause=a.pause,color_only=a.color_only,refresh_boundary=a.refresh_boundary,baseline=bool(a.baseline),production_modules=a.release,gpu_source_sha256=sha(a.gpu),
                 sources={p.name:sha(p) for p in sources},test_sha256=sha(Path(__file__)),
                 scope='Real cache, native DPB/shared DSP, router, arbiter, bridge, controller and pin model; six or eight 128B bursts, no CPU/display traffic; not board integration')
    (out/'receipt.json').write_text(json.dumps(receipt,indent=2));print(json.dumps({k:receipt[k] for k in ['status','summary','pause','baseline']}))

TB=r'''`timescale 1ns/1ps
module tb;
reg clk=0,controller_clk=0,phy_clk=0,reset=1;
wire logic_clk=clk,sdram_clk=phy_clk;
always #5.051 controller_clk=~controller_clk;
always @(posedge controller_clk)clk=~clk;
initial begin #13.259;forever begin phy_clk=~phy_clk;#5.051;end end
GSR GSR(.GSRI(1'b1));
wire O_sdram_clk,O_sdram_cke,O_sdram_cs_n,O_sdram_cas_n,O_sdram_ras_n,O_sdram_wen_n;
wire[3:0]O_sdram_dqm;wire[10:0]O_sdram_addr;wire[1:0]O_sdram_ba;wire[31:0]IO_sdram_dq;
wire ro_request_valid,ro_request_ready;wire[21:0]ro_address;wire[1:0]ro_count;
wire ro_response_valid,ro_response_last,ro_response_error;wire[63:0]ro_data;
wire fb_r_request_valid,fb_r_request_ready;wire[21:0]fb_r_address;wire[1:0]fb_r_count;
wire fb_r_response_valid,fb_r_response_last,fb_r_response_error;wire[63:0]fb_r_data;
wire fb_w_request_valid,fb_w_request_ready;wire[21:0]fb_w_address;wire[1:0]fb_w_count;
wire[63:0]fb_w_data;wire fb_w_data_ready,fb_w_response_valid,fb_w_response_last,fb_w_response_error;
`include "ports.vh"
GowinSdramCombination mc(.*);
// Only the initialization wait is shortened; every transport source stays exact.
defparam mc.bridge.u_controller.INIT_CYCLES=40;
LabPinModel #(.PERIOD_NS(10.102),.RETURN_MIN(1),.RETURN_MAX(4)) pin(
 .sclk(O_sdram_clk),.reset(reset),.cke(O_sdram_cke),.cs(O_sdram_cs_n),.ras(O_sdram_ras_n),
 .cas(O_sdram_cas_n),.we(O_sdram_wen_n),.dqm(O_sdram_dqm),.a(O_sdram_addr),.ba(O_sdram_ba),.dq(IO_sdram_dq),.cycle(),.refreshes());
reg ce=1,shade_phase=0;
reg row_valid=0;reg[2:0]row_index=0;reg[31:0]row_data=0;
wire row_accept,row_ready,preflight_bad,commit_valid,cache_fault,idle,flush_complete;
wire[8:0]commit_x;wire[7:0]commit_y;wire[3:0]commit_mask;
reg flush_request=0;
wire[53:0]shared_rop_product;wire shared_rop_offer,shared_rop_ce;wire[26:0]shared_rop_a;wire[7:0]shared_rop_b;
wire mem_req_valid,mem_req_write,mem_req_ready,mem_write_valid,mem_write_ready,mem_read_valid,mem_complete_valid,mem_complete_ok;
wire[31:0]mem_req_addr;wire[63:0]mem_write_data,mem_read_data;wire router_fault,router_busy;
gpu_v2_framebuffer_tdp #(.COLOR_BASE('h100000),.DEPTH_BASE('h140000),.PREFETCH_ENABLE(0)) cache(
 .clk(clk),.reset(reset),.ce(ce),.commit_allow(1'b1),.shade_phase(shade_phase),
 .shared_rop_product(shared_rop_product),.shared_rop_offer(shared_rop_offer),.shared_rop_ce(shared_rop_ce),.shared_rop_a(shared_rop_a),.shared_rop_b(shared_rop_b),
 .preflight_bad(preflight_bad),.row_valid(row_valid),.row_index(row_index),.row_data(row_data),
 .row_tile(9'd0),.row_x(9'd0),.row_y(8'd0),.row_mask(4'hf),.row_accept(row_accept),.row_ready(row_ready),
 .depth_func(3'd7),.depth_write(1'b@DW@),.blend(1'b0),.commit_valid(commit_valid),.commit_x(commit_x),.commit_y(commit_y),.commit_mask(commit_mask),
 .flush_request(flush_request),.flush_complete(flush_complete),.fault(cache_fault),.idle(idle),
 .prefetch_abort(1'b0),.prefetch_valid(1'b0),.prefetch_tile(10'd0),
 .mem_req_valid(mem_req_valid),.mem_req_write(mem_req_write),.mem_req_addr(mem_req_addr),.mem_req_ready(mem_req_ready),
 .mem_read_data(mem_read_data),.mem_read_valid(mem_read_valid),.mem_write_valid(mem_write_valid),.mem_write_data(mem_write_data),
 .mem_write_ready(mem_write_ready),.mem_complete_valid(mem_complete_valid),.mem_complete_ok(mem_complete_ok));
gpu_v2_final_rop_bank shared(.clk(clk),.reset(reset),.rop_ce(shared_rop_ce),.rop_a(shared_rop_a),.rop_b(shared_rop_b),.rop_product(shared_rop_product),.spec_ce(1'b0),.spec_a(9'd0),.spec_b(24'd0),.spec_product());
gpu_vliw_memory_router router(.clk(clk),.reset(reset),
 .geometry_request_valid(1'b0),.geometry_request_address(22'd0),.geometry_request_count(2'd0),
 .control_request_valid(1'b0),.control_request_address(22'd0),.control_request_count(2'd0),
 .texture_request_valid(1'b0),.texture_request_address(32'd0),
 .framebuffer_request_valid(mem_req_valid),.framebuffer_request_write(mem_req_write),.framebuffer_request_address(mem_req_addr),
 .framebuffer_request_ready(mem_req_ready),.framebuffer_write_valid(mem_write_valid),.framebuffer_write_data(mem_write_data),
 .framebuffer_write_ready(mem_write_ready),.framebuffer_read_valid(mem_read_valid),.framebuffer_read_data(mem_read_data),.framebuffer_complete(mem_complete_valid),.framebuffer_ok(mem_complete_ok),
 .ro_request_valid(ro_request_valid),.ro_request_ready(ro_request_ready),.ro_address(ro_address),.ro_count(ro_count),
 .ro_response_valid(ro_response_valid),.ro_response_last(ro_response_last),.ro_response_error(ro_response_error),.ro_data(ro_data),
 .fb_r_request_valid(fb_r_request_valid),.fb_r_request_ready(fb_r_request_ready),.fb_r_address(fb_r_address),.fb_r_count(fb_r_count),
 .fb_r_response_valid(fb_r_response_valid),.fb_r_response_last(fb_r_response_last),.fb_r_response_error(fb_r_response_error),.fb_r_data(fb_r_data),
 .fb_w_request_valid(fb_w_request_valid),.fb_w_request_ready(fb_w_request_ready),.fb_w_address(fb_w_address),.fb_w_count(fb_w_count),
 .fb_w_data(fb_w_data),.fb_w_data_ready(fb_w_data_ready),.fb_w_response_valid(fb_w_response_valid),.fb_w_response_last(fb_w_response_last),.fb_w_response_error(fb_w_response_error),.busy(router_busy),.fault(router_fault));
function[20:0]nw;input integer bytes;integer w;begin w=bytes/4;nw=((w>>5)&3)*524288+(w&31)+((w>>7)<<5);end endfunction
function[15:0]seed;input integer a;begin seed=(a*37+1234)&65535;end endfunction
function[31:0]rgba;input integer lane;begin case(lane)0:rgba=32'hff0000ff;1:rgba=32'hff00ff00;2:rgba=32'hffff0000;default:rgba=32'hff345678;endcase end endfunction
function[15:0]color;input integer lane;begin case(lane)0:color=16'hf800;1:color=16'h07e0;2:color=16'h001f;default:color=16'h7aa6;endcase end endfunction
integer cycle=0,rows=0,commits=0,i,j,a,lane,base,reqs=0,terms=0,flight=0,peak=0,readchain=0,writechain=0;
integer first=0,rend=0,wstart=0,wend=0,lastreadcol=-1,lastwritecol=-1,nativecycle=0;
reg[15:0]expected;reg[31:0]word_expected;
integer pin_cycle=0,read_column_last=-1,write_column_last=-1;
integer read_column_min=100000,read_column_max=0,write_column_min=100000,write_column_max=0,gap;
always @(posedge O_sdram_clk)if(!reset)begin
 pin_cycle=pin_cycle+1;
 if(!O_sdram_cs_n&&O_sdram_ras_n&&!O_sdram_cas_n)begin
  if(O_sdram_wen_n)begin
   if(read_column_last>=0)begin
    gap=pin_cycle-read_column_last;
    if(gap<read_column_min)read_column_min=gap;if(gap>read_column_max)read_column_max=gap;
   end
   read_column_last=pin_cycle;
  end else begin
   if(write_column_last>=0)begin
    gap=pin_cycle-write_column_last;
    if(gap<write_column_min)write_column_min=gap;if(gap>write_column_max)write_column_max=gap;
   end
   write_column_last=pin_cycle;
  end
 end
end
always @(posedge controller_clk)if(!reset)begin
 nativecycle=nativecycle+1;
 if(mc.bridge.u_controller.read_chain_accept)readchain=readchain+1;
 if(mc.bridge.u_controller.chain_accept)writechain=writechain+1;
end
always @(negedge clk)if(!reset)begin
 ce=!@PAUSE@||cycle%17!=4;
 row_index=rows%8;row_data=row_index[0]?32'd12345:rgba(rows/2);
 row_valid=rows<8&&cycle>@START@&&row_index[0]==shade_phase;
 if(commits==1&&idle)flush_request=1;
end
always @(posedge clk)begin
 cycle=cycle+1;if(cycle>40000)$fatal(1,"framebuffer native watchdog rows%0d req%0d terms%0d",rows,reqs,terms);
 if(!reset)begin
  if(ce)shade_phase<=!shade_phase;
  if(row_accept)rows=rows+1;
  if(commit_valid)commits=commits+1;
  if(cache_fault||router_fault||preflight_bad)$fatal(1,"framebuffer native fault at%0d cache%0d router%0d",cycle,cache_fault,router_fault);
  if(mem_req_valid&&mem_req_ready)begin
   a=(reqs%4<2?'h100000:'h140000)+(reqs%2)*128;
   if(mem_req_addr!==a||mem_req_write!==(reqs>=4))$fatal(1,"framebuffer request%0d addr%h wanted%h",reqs,mem_req_addr,a);
   if(reqs==0)first=cycle;if(reqs==4)wstart=cycle;
   reqs=reqs+1;flight=flight+1;if(flight>peak)peak=flight;
  end
  if(mem_complete_valid)begin
   if(!mem_complete_ok)$fatal(1,"framebuffer completion error");
   terms=terms+1;flight=flight-1;if(terms==4)rend=cycle;if(terms==4+@NWRITE@)wend=cycle;
  end
 end
end
initial begin
 for(j=0;j<2;j=j+1)begin
  base=j?'h140000:'h100000;
  for(i=0;i<512;i=i+4)pin.seed_word(nw(base+i),{seed(base+i+2),seed(base+i)});
  pin.seed_word(nw(base-4),32'hdeadbeef);
 end
 repeat(4)@(negedge clk);reset=0;
 while(!flush_complete)@(negedge clk);
 if(reqs!=4+@NWRITE@||terms!=4+@NWRITE@||flight!=0||rows!=8||commits!=1)$fatal(1,"framebuffer native conservation");
 for(j=0;j<2;j=j+1)begin
  base=j?'h140000:'h100000;
  for(i=0;i<512;i=i+2)begin
   expected=seed(base+i);
   if(i==0||i==2||i==32||i==34)begin
    lane=(i/32)*2+(i%32)/2;if(!j||@DW@)expected=j?16'd12345:color(lane);
   end
   word_expected=pin.mem[nw(base+(i&~3))];
   if((i%4==0?word_expected[15:0]:word_expected[31:16])!==expected)$fatal(1,"framebuffer final plane%0d byte%0d got%h want%h",j,i,word_expected,expected);
  end
  if(pin.mem[nw(base-4)]!==32'hdeadbeef)$fatal(1,"framebuffer boundary guard");
 end
 $display("TIMING refill=%0d writeback=%0d work=%0d peak=%0d read_chain=%0d write_chain=%0d",rend-first+1,wend-wstart+1,wend-first+1,peak,readchain,writechain);
 $display("TIMING read_column_min=%0d read_column_max=%0d write_column_min=%0d write_column_max=%0d",read_column_min,read_column_max,write_column_min,write_column_max);
 $display("PASS framebuffer native %0d bursts: native DPB, varying Q, full footprint, guards, pause=%0d",4+@NWRITE@,@PAUSE@);$finish;
end
endmodule
'''

if __name__=='__main__':main()
