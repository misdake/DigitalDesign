"""Fast CP-only maximum-length/cursor test; payload correctness is tested separately."""
import argparse
import os
from pathlib import Path
import re
import subprocess
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT/'ip/gpu-v2/examples/postquad_ii4'))
from wrapper import wrapper


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--output',type=Path,required=True)
    args = parser.parse_args()
    out = args.output.resolve(); out.mkdir(parents=True,exist_ok=False)
    source = ROOT/'ip/gpu-v2/src/system/command_processor.v'
    _, info = wrapper(re.sub(r'\b(input|output)(?=\[)',r'\1 ',source.read_text()))
    ports = [''.join(['reg ' if p['direction']=='input' else 'wire ',
        f'[{p["bits"]-1}:0] ' if p['bits']>1 else '',p['name'],
        '=0;' if p['direction']=='input' else ';']) for p in info['ports']]
    tb = r'''
`timescale 1ns/1ps
module tb;
@PORTS@
gpu_command_processor dut(.*);
always #5 clk=~clk;
reg[63:0]ram[0:511];integer cycle=0,reads=0,writes=0,remaining=0,timer=0;
always @(negedge clk)begin
 copy_ready=(remaining==0&&!copy_complete);
 if(!reset)ce=cycle%7!=3;
end
always @(posedge clk)begin
 cycle=cycle+1;if(cycle>100000)$fatal(1,"MEMCPY cursor watchdog state%0d reads%0d writes%0d phase%0d rem%0d ready%b Q%h",dut.state,reads,writes,dut.object_index,remaining,copy_ready,cp_q);
 cache_done<=cache_valid&&cache_ready;cache_done_bank<=cache_bank;
 if(cp_re)cp_q<=ram[cp_addr];
 copy_complete<=0;
 if(remaining!=0)begin
  if(timer!=0)timer<=timer-1;
  else begin copy_complete<=1;remaining<=remaining-1;timer<=2;end
 end
 if(!reset&&copy_request&&copy_ready)begin
  if(copy_write)begin
   if(copy_address!==14'd8192+writes)$fatal(1,"MEMCPY maximum destination cursor");
   writes=writes+1;remaining<=1;
  end else begin
   if(copy_address!==14'd4096+reads)$fatal(1,"MEMCPY maximum source cursor");
   reads=reads+1;remaining<=2;
  end
  timer<=2;
 end
end
initial begin
 reset=1;ce=1;cp_ready=1;render_idle=1;dma_idle=1;cache_ready=1;flush_complete=1;
 ram[448]=64'h7000000040000003;
 ram[449]=64'hf0000000c0000044;
 ram[272]=(64'd4096)|(64'd8192<<14)|(64'd2047<<28);
 repeat(4)@(negedge clk);reset=0;
 load_count=1;control_data=3;@(negedge clk);load_count=0;submit=1;control_data=0;
 @(negedge clk);submit=0;while(busy)@(negedge clk);
 if(reads!=2048||writes!=2048||stop||remaining!=0)$fatal(1,"MEMCPY maximum length conservation");
 $display("PASS MEMCPY 1MiB CP cursor: reads=%0d writes=%0d cycles=%0d CE pauses enabled",reads,writes,cycle);$finish;
end
endmodule
'''.replace('@PORTS@','\n'.join(ports))
    (out/'tb.v').write_text(tb)
    commands=[('compile',[os.environ['IVERILOG_EXE'],'-g2012','-s','tb','-o',out/'sim.vvp',source,out/'tb.v']),
              ('run',[os.environ['VVP_EXE'],out/'sim.vvp'])]
    for name, command in commands:
        with (out/f'{name}.log').open('w') as log:
            result=subprocess.run(list(map(str,command)),cwd=out,stdout=log,stderr=subprocess.STDOUT,timeout=60)
        text=(out/f'{name}.log').read_text(errors='replace')
        if result.returncode: raise RuntimeError(text[-1600:])
    print('\n'.join(line for line in text.splitlines() if line.startswith('PASS ')))


if __name__=='__main__': main()
