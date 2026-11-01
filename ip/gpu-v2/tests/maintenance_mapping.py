"""Actual command TDP sweep: all UI tile addresses, raw-copy boundaries and CE0."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
sys.dont_write_bytecode=True
from maintenance_profiles import DIAGNOSTIC_TB

TB = DIAGNOSTIC_TB[:DIAGNOSTIC_TB.index('integer cycle=0')]
TB = TB.replace('reg ui_active=0,ui_lower_ready=1;', '''reg ui_active=1,ui_lower_ready=1,cmd_valid=0,copy_active=0;
reg[8:0]ui_tile=0;reg[13:0]copy_address=0;
reg mem_req_ready=0,mem_read_valid=0;reg[63:0]mem_read_data=0;
wire cmd_ready,cmd_done,mem_req_valid;wire[31:0]mem_req_addr;''')
for before,after in [
    ('.mem_req_ready(1\'b0),.mem_read_data(64\'d0),.mem_read_valid(1\'b0)',
     '.mem_req_ready(mem_req_ready),.mem_read_data(mem_read_data),.mem_read_valid(mem_read_valid),.mem_req_valid(mem_req_valid),.mem_req_addr(mem_req_addr)'),
    ('.ui_cmd_valid(1\'b0)', '.ui_cmd_valid(cmd_valid)'),
    ('.ui_tile(9\'d0),.copy_active(1\'b0),.copy_address(14\'d0)',
     '.ui_tile(ui_tile),.copy_active(copy_active),.copy_address(copy_address),.ui_cmd_ready(cmd_ready),.ui_cmd_done(cmd_done)')]:
    assert TB.count(before)==1,before
    TB=TB.replace(before,after)
TB += r'''
integer cycle=0,t=0,requests=0,beats=0,halves=0,beat=0,expected_base=0;
integer all_requests=0,all_beats=0;reg pending=0,held=0;reg[31:0]held_addr;
always @(negedge clk)begin
 mem_req_ready=!pending&&cycle%5!=1;
 mem_read_valid=pending&&cycle%3!=1;
 mem_complete_valid=mem_read_valid&&beat==15;
 mem_read_data={32'h7193ac08,expected_base[31:0]}^{58'd0,beat[5:0]};
end
always @(posedge clk)if(!reset)begin
 cycle=cycle+1;if(cycle>100000)$fatal(1,"maintenance mapping watchdog");
 if(fault)$fatal(1,"legal mapping fault");
 if(held&&(!mem_req_valid||mem_req_addr!==held_addr))$fatal(1,"maintenance request withdrew under backpressure");
 held=mem_req_valid&&!mem_req_ready;held_addr=mem_req_addr;
 if(mem_req_valid&&mem_req_ready)begin
  if(mem_req_addr!==expected_base+requests*128)$fatal(1,"physical mapping t=%0d sector=%0d addr=%h expected=%h",t,requests,mem_req_addr,expected_base+requests*128);
  pending=1;beat=0;requests=requests+1;all_requests=all_requests+1;
 end else if(mem_read_valid)begin
  beats=beats+1;all_beats=all_beats+1;
  if(beat==15)pending=0;else beat=beat+1;
 end
 if(cmd_done)halves=halves+1;
 if(dut.banks[0].b_we&&dut.banks[0].b_addr>=64)$fatal(1,"serial refill wrote depth/other cache line");
end
task tick;begin @(negedge clk);#1;end endtask
task transfer;begin
 requests=0;beats=0;halves=0;cmd_valid=1;tick;cmd_valid=0;
 while(!cmd_ready)tick;
 if(requests!=4||beats!=64||halves!=2||pending)$fatal(1,"half-tile/byte/terminal conservation");
 if(dut.tvalid[0]||dut.tvalid[1]||dut.tvalid[2]||dut.tvalid[3]||dut.tvalid[4]||dut.tvalid[5]||dut.tvalid[6]||dut.tvalid[7])
  $fatal(1,"temporary maintenance published a cache resident");
end endtask
initial begin
 repeat(3)tick;reset=0;tick;
 // CE remains zero throughout: actual request, RAM and retirement must advance.
 for(t=0;t<375;t=t+1)begin
  ui_tile=t;expected_base=32'h100000+((t%25)*16+t/25)*512;transfer;
 end
 copy_active=1;
 for(t=0;t<7;t=t+1)begin
  case(t)
   0:copy_address=0;1:copy_address=127;2:copy_address=128;
   3:copy_address=4095;4:copy_address=4096;5:copy_address=8192;6:copy_address=16383;
  endcase
  expected_base=copy_address*512;transfer;
 end
 $display("PASS MAINTENANCE_MAPPING tiles=375 raw_blocks=7 req=%0d beats=%0d CE0 cycles=%0d",all_requests,all_beats,cycle);$finish;
end
endmodule
'''


def main():
    parser=argparse.ArgumentParser(__doc__)
    parser.add_argument('--exports',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    out=args.output.resolve();out.mkdir(parents=True,exist_ok=False)
    tb=out/'mapping_tb.v';tb.write_text(TB)
    rows=[]
    for backend in ['native','portable']:
        for synthesis in [False,True]:
            name=f'mapping-{backend}-{int(synthesis)}';exe=out/(name+'.vvp')
            cmd=[os.environ['IVERILOG_EXE'],'-g2012','-DRB03_COMMAND','-s','tb','-o',exe]
            if synthesis:cmd+=['-DSYNTHESIS']
            cmd += [(args.exports/f'command-{backend}.v').resolve(),tb,Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v']
            for label,command in [('compile',cmd),('run',[os.environ['VVP_EXE'],exe])]:
                result=subprocess.run(list(map(str,command)),capture_output=True,timeout=60)
                log=result.stdout+result.stderr;(out/(name+'-'+label+'.log')).write_bytes(log)
                assert result.returncode==0,log.decode(errors='replace')[-2000:]
            assert b'PASS MAINTENANCE_MAPPING' in log
            rows.append(dict(backend=backend,synthesis=synthesis,result=log.decode(errors='replace').splitlines()[0]))
    (out/'receipt.json').write_text(json.dumps(dict(status='pass',cases=rows),indent=2))
    print(json.dumps(rows))


if __name__=='__main__':
    main()
