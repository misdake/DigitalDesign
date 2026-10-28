"""Bounded full-GPU CLEAR address/data/terminal test for both padded column planes."""
from pathlib import Path
import argparse,hashlib,importlib.util,json,os,re,struct,subprocess,sys
sys.dont_write_bytecode=True
ROOT=Path(__file__).resolve().parents[3]
HERE=ROOT/'ip/gpu-v2/examples/vliw_integration'
sys.path.insert(0,str(HERE))
from replay_command import CommandBuffer,target_params
from device_bus import bench
sys.path.insert(0,str(HERE.parent/'postquad_ii4'))
from wrapper import wrapper

TB=r'''
`timescale 1ns/1ps
module tb;
@PORTS@
reg[31:0]command_base=32'h1000,command_bytes=@BYTES@;
GSR GSR(.GSRI(1'b1));always #5 clk=~clk;
gpu_v2_vliw_command dut(.*);
reg[63:0]memory[0:8191];reg active=0,writing=0;
integer cycle=0,ba=0,bi=0,bn=0,wr=0,terms=0,beats=0,wi=0,delay_left=0,mode=0,hold_left=0,held=0,j;
reg same_terminal=0,paused=0;reg[15:0]value;
always@(negedge clk)begin
 ce=cycle%7!=2;if(hold_left!=0)begin ce=0;hold_left=hold_left-1;end
 ro_request_ready=!active&&cycle%5!=1;ro_response_valid=active&&cycle%3!=1;
 ro_response_error=0;ro_response_last=bi==bn-1;ro_data=memory[(ba>>2)+bi];
 fb_r_request_ready=1;fb_r_response_valid=0;fb_r_response_error=0;fb_r_response_last=0;fb_r_data=0;
 fb_w_request_ready=!writing&&cycle%5!=1;fb_w_data_ready=writing&&wi<16&&cycle%4!=2;
 fb_w_response_valid=writing&&((wi==16&&delay_left==0)||(same_terminal&&wi==15&&fb_w_data_ready));
 fb_w_response_last=1;fb_w_response_error=0;
end
always@(posedge clk)begin
 cycle=cycle+1;if(cycle>200000)$fatal(1,"column CLEAR watchdog");
 if(!reset)begin
  if(ro_request_valid&&ro_request_ready)begin active=1;ba=ro_address;bi=0;bn=ro_count==3?16:ro_count==1?8:4;end
  else if(ro_response_valid)begin if(ro_response_last)active=0;else bi=bi+1;end
  if(fb_r_request_valid)$fatal(1,"CLEAR unexpectedly read framebuffer");
  if(fb_w_request_valid&&fb_w_request_ready)begin
   if(wr>=3200||fb_w_count!=3||fb_w_address!==(wr<1600?22'h80000+wr*64:22'ha0000+(wr-1600)*64))
    $fatal(1,"column CLEAR sector%0d address%h",wr,fb_w_address);
   value=wr<1600?16'h1234:16'habcd;wr=wr+1;writing=1;wi=0;delay_left=3;same_terminal=wr%2==0;
  end else if(writing)begin
   if(fb_w_data_ready)begin
    if(wi>=16||fb_w_data!=={4{value}})$fatal(1,"column CLEAR data sector%0d beat%0d data%h",wr,wi,fb_w_data);
    wi=wi+1;beats=beats+1;
   end
   if(wi==16&&delay_left!=0)delay_left=delay_left-1;
   if(fb_w_response_valid)begin
    terms=terms+1;writing=0;
    if(mode==1&&terms==3200)begin paused=1;hold_left=5;end
   end
  end
  if(paused&&!ce&&hold_left!=0)begin
   held=held+1;
   if(hold_left<=2&&(dut.processor.state!=14||!busy||dut.clear_active))$fatal(1,"column CLEAR final terminal CE0 retirement");
  end
 end
end
task tick;begin @(negedge clk);#1;end endtask
initial begin
 j=$value$plusargs("MODE=%d",mode);$readmemh("memory.hex",memory);
 reset=1;output_ready=1;tick;tick;reset=0;submit_commands(command_base,command_bytes);
 while(busy)tick;
 if(fault||device_read_data[1]||writing||active||wr!=3200||terms!=3200||beats!=51200||(mode==1&&held<3))
  $fatal(1,"column CLEAR conservation wr%0d terms%0d beats%0d held%0d",wr,terms,beats,held);
 $display("PASS COLUMN_CLEAR mode=%0d sectors=%0d bytes=%0d beats=%0d CE0holds=%0d cycles=%0d",mode,wr,wr*128,beats,held,cycle);$finish;
end
endmodule
'''


def main():
    ap=argparse.ArgumentParser(__doc__);ap.add_argument('--export',type=Path,required=True);ap.add_argument('--output',type=Path,required=True);a=ap.parse_args()
    out=a.output.resolve();out.mkdir(parents=True,exist_ok=False);raw=a.export.read_bytes();identity=hashlib.sha256(raw).hexdigest();(out/'dut.v').write_bytes(raw)
    cb=CommandBuffer();cb.emit('WAIT',a=3);cb.emit('FLUSH');cb.emit('DMA',288,0x4000,32);cb.emit('WAIT',a=3);cb.emit('SET_TARGET',288)
    cb.emit('CLEAR',0,0x1234);cb.emit('CLEAR',1,0xabcd);cb.emit('WAIT',a=3);cb.emit('FLUSH');cb.emit('END');commands=cb.encode()
    memory=bytearray(65536);memory[0x1000:0x1000+len(commands)]=commands;memory[0x4000:0x4020]=target_params(0x100000,0x140000)
    (out/'memory.hex').write_text(''.join(f'{int.from_bytes(memory[i:i+8],"little"):016x}\n' for i in range(0,len(memory),8)))
    _,info=wrapper(re.sub(r'\b(input|output)(?=\[)',r'\1 ',raw.decode()));ports=[]
    for p in info['ports']:
        ports.append(('reg ' if p['direction']=='input' else 'wire ')+(f'[{p["bits"]-1}:0] ' if p['bits']>1 else '')+p['name']+('=0;' if p['direction']=='input' else ';'))
    (out/'tb.v').write_text(bench(TB.replace('@PORTS@','\n'.join(ports)).replace('@BYTES@',str(len(commands)))))
    cases=[('compile',[os.environ['IVERILOG_EXE'],'-g2012','-s','tb','-o',out/'sim.vvp',out/'dut.v',out/'tb.v',Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v'])]
    cases += [(f'mode{m}',[os.environ['VVP_EXE'],out/'sim.vvp',f'+MODE={m}']) for m in (0,1)]
    passes=[]
    for name,command in cases:
        with (out/f'{name}.log').open('wb') as log:r=subprocess.run(list(map(str,command)),cwd=out,stdout=log,stderr=subprocess.STDOUT,timeout=300)
        text=(out/f'{name}.log').read_text(errors='replace');assert r.returncode==0,text[-2000:]
        passes += [l for l in text.splitlines() if l.startswith('PASS ')]
    assert hashlib.sha256(a.export.read_bytes()).hexdigest()==identity
    receipt=dict(status='pass',layout='macro16x16-column16',export_sha256=identity,passes=passes,
        sources_sha256={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in [Path(__file__),out/'tb.v',out/'memory.hex']},
        scope='Complete GPU/CP/CLEAR over a stalled64-bit peer; both1600-sector physical planes including padded tiles, exact address/count/data, same-edge and delayed terminals and final CE0 retirement; no physical MC or devices.')
    (out/'receipt.json').write_text(json.dumps(receipt,indent=2));print(json.dumps(receipt,indent=2))


if __name__=='__main__':main()
