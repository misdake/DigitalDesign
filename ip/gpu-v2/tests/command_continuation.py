"""Bounded CP/real-MC continuation tests, with exact scratch writes and pin data."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
import sys
sys.path.insert(0,str(ROOT/'hardware/vendor/gowin/tests'))
from stream_contract import load_contract, stage_peer
sys.path.insert(0, str(ROOT/'ip/gpu-v2/examples/vliw_integration'))
from replay_command import CommandBuffer, target_params
sys.path.insert(0, str(ROOT/'ip/gpu-v2/examples/postquad_ii4'))
from wrapper import wrapper

TB = r'''
`timescale 1ns/1ps
module tb;
@PORTS@
reg[31:0]command_base=0,command_bytes=0;
GSR GSR(.GSRI(1'b1));
reg controller_clk=0,phy_clk=0;
always #4.629629 controller_clk=~controller_clk;
always @(posedge controller_clk)clk=~clk;
initial begin #12.430556;forever begin phy_clk=~phy_clk;#4.629629;end end
wire logic_clk=clk,sdram_clk=phy_clk;
wire O_sdram_clk,O_sdram_cke,O_sdram_cs_n,O_sdram_cas_n,O_sdram_ras_n,O_sdram_wen_n;
wire[3:0]O_sdram_dqm;wire[10:0]O_sdram_addr;wire[1:0]O_sdram_ba;wire[31:0]IO_sdram_dq;
`include "ports.vh"
GowinSdramCombination mc(.*);
LabPinModel #(.PERIOD_NS(9.259259),.CAS_LATENCY(3),.RCD_MIN(3),.RP_MIN(3),.RETURN_MIN(1),.RETURN_MAX(4)) pin(
 .sclk(O_sdram_clk),.reset(reset),.cke(O_sdram_cke),.cs(O_sdram_cs_n),.ras(O_sdram_ras_n),
 .cas(O_sdram_cas_n),.we(O_sdram_wen_n),.dqm(O_sdram_dqm),.a(O_sdram_addr),.ba(O_sdram_ba),.dq(IO_sdram_dq),.cycle(),.refreshes());
gpu_v2_vliw_command dut(.*);
function[20:0]native_word;input integer bytes;integer w;
begin w=bytes/4;native_word=((w>>5)&3)*524288+(w&31)+((w>>7)<<5);end endfunction
integer cycle=0,first=0,last=0,wi=0,requests=0,terms=0,beats=0,inflight=0,peak=0;
integer work_accepts=0,work_completes=0,mesh_done_count=0,mode=0,fd,rc,a,v,j,expected_i;
reg[8:0]want_addr[0:2047];reg[63:0]want_data[0:2047];
reg[72:0]expected[0:2047];
always @(negedge clk)if(!reset)ce=(mode==0||cycle%7!=3);
always @(posedge clk)begin
 cycle=cycle+1;if(cycle>100000)$fatal(1,"continuation watchdog pc%0d state%0d writes%0d",dut.processor.pc,dut.processor.state,wi);
 if(!reset)begin
  if(dut.work_valid&&dut.work_ready&&ce)begin work_accepts=work_accepts+1;if(first==0)first=cycle;end
  if(dut.work_done)work_completes=work_completes+1;
  if(dut.mesh_done)mesh_done_count=mesh_done_count+1;
  if(dut.dma_we&&dut.dma_addr<448)begin
   if(wi>=@NWORDS@||dut.dma_addr!==expected[wi][72:64]||dut.dma_data!==expected[wi][63:0])
    $fatal(1,"scratch write %0d addr%0d/%0d data%h/%h",wi,dut.dma_addr,expected[wi][72:64],dut.dma_data,expected[wi][63:0]);
   wi=wi+1;last=cycle;
  end
  if(@CLEAR@)begin
   if(fb_w_request_valid&&fb_w_request_ready)begin
    if(fb_w_count!=3||fb_w_address!==((requests<1500?32'h100000:32'h140000)/2+(requests%1500)*64))
     $fatal(1,"clear request address %0d %h",requests,fb_w_address);
    if(requests==0)first=cycle;
    requests=requests+1;inflight=inflight+1;if(inflight>peak)peak=inflight;
   end
   if(fb_w_data_ready)begin
    if(fb_w_data!==(beats<24000?64'h5aa55aa55aa55aa5:64'ha55aa55aa55aa55a))$fatal(1,"clear data");
    beats=beats+1;
   end
   if(fb_w_response_valid&&fb_w_response_last)begin terms=terms+1;inflight=inflight-1;last=cycle;end
  end else begin
   if(ro_request_valid&&ro_request_ready)begin requests=requests+1;inflight=inflight+1;if(inflight>peak)peak=inflight;end
   if(ro_response_valid&&ro_response_last)begin terms=terms+1;inflight=inflight-1;end
  end
  if(ro_response_error||fb_w_response_error||fb_r_response_error)$fatal(1,"MC error");
 end
end
initial begin
 if(!$value$plusargs("MODE=%d",mode))mode=0;
 $readmemh("expected.hex",expected,0,@NWORDS@-1);
 fd=$fopen("seed.txt","r");if(!fd)$fatal(1,"seed file");
 while(!$feof(fd))begin rc=$fscanf(fd,"%h %h",a,v);if(rc==2)pin.seed_word(native_word(a),v);end $fclose(fd);
 reset=1;ce=1;command_base=32'h40000;command_bytes=@BYTES@;output_ready=1;
 repeat(4)@(negedge clk);reset=0;
 submit_commands(command_base,command_bytes);
 while(busy)@(negedge clk);
 if(device_read_data[1]||wi!=@NWORDS@||work_accepts!=@JOBS@||work_completes!=@JOBS@||mesh_done_count!=@MESH@||inflight!=0)
  $fatal(1,"result err%0d scratch%0d jobs%0d/%0d mesh%0d flight%0d",device_read_data[1],wi,work_accepts,work_completes,mesh_done_count,inflight);
 if(@CLEAR@)begin
  if(requests!=3000||terms!=3000||beats!=48000)$fatal(1,"clear conservation");
  for(j=0;j<48000;j=j+1)begin
   if(pin.mem[native_word(32'h100000+j*4)]!==32'h5aa55aa5)$fatal(1,"clear color word%0d",j);
   if(pin.mem[native_word(32'h140000+j*4)]!==32'ha55aa55a)$fatal(1,"clear depth word%0d",j);
  end
  if(pin.mem[native_word(32'h0ffffc)]!==32'hcafebabe||pin.mem[native_word(32'h12ee00)]!==32'hcafebabe||
     pin.mem[native_word(32'h13fffc)]!==32'hcafebabe||pin.mem[native_word(32'h16ee00)]!==32'hcafebabe)$fatal(1,"clear boundary guard");
 end
 $display("PASS CONTINUATION clear=%0d mode=%0d total=%0d work=%0d scratch=%0d requests=%0d peak=%0d",@CLEAR@,mode,cycle,last-first+1,wi,requests,peak);
 $finish;
end
endmodule
'''


def assets(case, clear, dma_jobs=64, meshlet_blocks=8, nops=0):
    seed = {}
    expected = []
    def put(address, data):
        for i in range(0,len(data),4):
            seed[address+i] = int.from_bytes(data[i:i+4], 'little')
    def copy(dst, address, data):
        put(address, data)
        expected.extend((dst+i//8, int.from_bytes(data[i:i+8], 'little')) for i in range(0,len(data),8))
    cb = CommandBuffer(); cb.emit('WAIT', a=2); cb.emit('FLUSH')
    if clear:
        copy(272,0x50140,target_params(0x100000,0x140000))
        cb.emit('DMA',272,0x50140,32);cb.emit('WAIT',a=1);cb.emit('SET_TARGET',272)
        cb.emit('CLEAR',0,0x5aa5);cb.emit('CLEAR',1,0xa55a)
        for a in [0xffffc,0x12ee00,0x13fffc,0x16ee00]:seed[a]=0xcafebabe
        jobs, meshes = 1,0
    else:
        # A valid metadata prefix; the payload is opaque until a later DRAW.
        meta=struct.pack('<4Q',0,0,0,0)
        payload=b''.join(struct.pack('<Q',0x7000000000000000+i) for i in range(meshlet_blocks*16))
        put(0x70000,meta+bytes(96)+payload)
        expected.extend((260+i//8,int.from_bytes(meta[i:i+8],'little')) for i in range(0,32,8))
        expected.extend((128+i,0x7000000000000000+i) for i in range(meshlet_blocks*16))
        cb.emit('DMA_MESHLET',1,0x70000,meshlet_blocks*128)
        for _ in range(nops):cb.emit('NOP')
        for i in range(dma_jobs):
            size=[32,64,128][i%3];dst=[272,280,320][i%3];a=0x60000+i*128
            data=b''.join(struct.pack('<I',0xa0000000|(i<<8)|j) for j in range(size//4))
            copy(dst,a,data);cb.emit('DMA',dst,a,size)
        jobs, meshes = dma_jobs+1,1
    cb.emit('WAIT',a=1);cb.emit('WAIT',a=2);cb.emit('FLUSH');cb.emit('END')
    code=cb.encode();put(0x40000,code+bytes((-len(code))%128))
    (case/'seed.txt').write_text(''.join(f'{a:08x} {v:08x}\n' for a,v in sorted(seed.items())))
    (case/'expected.hex').write_text(''.join(f'{(a<<64)|v:019x}\n' for a,v in expected))
    return len(code),len(expected),jobs,meshes


def main():
    ap=argparse.ArgumentParser(__doc__)
    ap.add_argument('--export',type=Path,required=True);ap.add_argument('--peer',type=Path,required=True)
    ap.add_argument('--output',type=Path,required=True);ap.add_argument('--clear',action='store_true')
    ap.add_argument('--modes',type=int,nargs='+',default=[0,1])
    ap.add_argument('--dma-jobs',type=int,default=64)
    ap.add_argument('--meshlet-blocks',type=int,default=8)
    ap.add_argument('--nops',type=int,default=0)
    ap.add_argument('--clear-stop-after',type=int,default=0,help='Abort after a few accepted CLEAR bursts, then verify all held work drains')
    args=ap.parse_args()
    assert 1<=args.dma_jobs<=64 and 1<=args.meshlet_blocks<=8 and 0<=args.nops<=64
    assert not args.clear_stop_after or args.clear and 2<=args.clear_stop_after<=16
    out=args.output.resolve();out.mkdir(parents=True,exist_ok=False)
    source=args.export.resolve().read_bytes();(out/'dut.v').write_bytes(source)
    _, ports=wrapper(re.sub(r'\b(input|output)(?=\[)',r'\1 ',source.decode()))
    decl=[]
    for p in ports['ports']:
        driven = p['direction']=='input' and not p['name'].startswith(('ro_', 'fb_r_', 'fb_w_'))
        decl.append(('reg ' if driven else 'wire ')+(f'[{p["bits"]-1}:0] ' if p['bits']>1 else '')+p['name']+('=0;' if driven else ';'))
    count,nwords,jobs,meshes=assets(out,args.clear,args.dma_jobs,args.meshlet_blocks,args.nops)
    tb=TB
    for k,v in dict(PORTS='\n'.join(decl),BYTES=count,NWORDS=nwords,JOBS=jobs,MESH=meshes,CLEAR=int(args.clear)).items():tb=tb.replace('@'+k+'@',str(v))
    if args.clear_stop_after:
        tb=tb.replace('always @(negedge clk)if(!reset)ce=', 'always @(negedge clk)if(!reset)begin if(requests>='+str(args.clear_stop_after)+')abort=1;end\nalways @(negedge clk)if(!reset)ce=')
        tb=tb.replace('while(busy)@(negedge clk);', 'while(!(dut.processor.state==10&&dut.dma_idle&&!dut.memory_busy))@(negedge clk);if(!busy||!fault)$fatal(1,"abort falsely completed");')
        begin=tb.index('  if(requests!=3000')
        end=tb.index('  if(pin.mem[native_word(32\'h0ffffc)]',begin)
        tb=tb[:begin]+"""  if(requests<@STOP@||requests>@STOP@+1||terms!=requests||beats!=requests*16)$fatal(1,"clear short drain conservation");
  for(j=0;j<requests*32;j=j+1)
   if(pin.mem[native_word(32'h100000+j*4)]!==32'h5aa55aa5)$fatal(1,"clear short word%0d",j);
""".replace('@STOP@',str(args.clear_stop_after))+tb[end:]
    from device_bus import bench as device_bench
    tb=device_bench(tb)
    (out/'tb.v').write_text(tb)
    peer=args.peer.resolve();files=[]
    load_contract(args.peer)
    for name in ['bridge.v','combination.v','controller.v','cpu_v3_memory_arbiter.v','pins.v','shared_port.v','ports.vh']:
        shutil.copyfile(peer/name,out/name)
        if name.endswith('.v'):files.append(out/name)
    commands=[('compile',[os.environ['IVERILOG_EXE'],'-g2012','-D__ICARUS__','-I',out,'-s','tb','-o',out/'sim.vvp',out/'dut.v',*files,out/'tb.v',Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v'])]
    commands += [(f'run-{m}',[os.environ['VVP_EXE'],out/'sim.vvp',f'+MODE={m}']) for m in args.modes]
    passes=[]
    for name,cmd in commands:
        assert hashlib.sha256((out/'dut.v').read_bytes()).digest()==hashlib.sha256(source).digest()
        with (out/(name+'.log')).open('wb') as log:r=subprocess.run(list(map(str,cmd)),cwd=out,stdout=log,stderr=subprocess.STDOUT,timeout=180)
        text=(out/(name+'.log')).read_text(errors='replace')
        if r.returncode:raise RuntimeError(text[-2200:])
        passes += [l for l in text.splitlines() if l.startswith('PASS ')]
    receipt=dict(status='pass',export_sha256=hashlib.sha256(source).hexdigest(),clear=args.clear,clear_stop_after=args.clear_stop_after,dma_jobs=args.dma_jobs,meshlet_blocks=args.meshlet_blocks,nops=args.nops,cases=passes,
        peer_sha256={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in files})
    (out/'receipt.json').write_text(json.dumps(receipt,indent=2));print(json.dumps({k:v for k,v in receipt.items() if k != 'peer_sha256'}))


if __name__=='__main__':main()
