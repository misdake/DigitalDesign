"""Bounded exact cache-return comparison under stalls and conflicting set traffic.

The baseline is an approved physical Group35 command export. Both backends
replay all64 local origins and six ways mapped to one set; logs stay in output.
"""
from pathlib import Path
import argparse
import hashlib
import json
import os
import subprocess

TB = r'''
`timescale 1ns/1ps
module tb;
reg clk=0,reset=1;always #5 clk=~clk;
reg ce=1,allow=1,valid=0,ordy=0,mready=0,mvalid=0,complete=0,ww=0;
reg[34:0]payload=0;reg[6:0]wa=0;reg[3:0]index=0;reg[63:0]data=0;
wire ready,ov,mreq,drained,fault,bad;wire[71:0]op;wire[15:0]t0,t1,t2,t3;wire[31:0]addr;
reg active=0;reg[31:0]base=0;integer beat=0,cycle=0,sent=0,got=0,reqs=0,init=0,holds=0;reg[135:0]held;
function[34:0]item;input integer i;reg[15:0]sector;begin sector=16'h80+((i/64)%6)*16;item={4'hf,1'b0,8'd0,i[5:3],i[2:0],sector};end endfunction
function[15:0]value;input[31:0]a;begin value=((a>>1)*32'h17e9)^(a>>6)^16'ha35c;end endfunction
wire[34:0]next_item=item(sent+1);wire[15:0]item_sector_next=next_item[15:0];
gpu_v2_texture_runtime_cache_unique #(.NATIVE(@NATIVE@)) dut(.clk(clk),.reset(reset),.proposal_ce(ce),.commit_allow(allow),.abort(1'b0),
.in_valid(valid),.in_payload(payload),.texture_invalidate(1'b0),.weight_write(ww),.weight_address(wa),.weight_value({4{9'd64}}),
.next_valid(sent<383),.next_key(item_sector_next),.in_ready(ready),.out_ready(ordy),.out_valid(ov),.out_payload(op),
.out_tex0(t0),.out_tex1(t1),.out_tex2(t2),.out_tex3(t3),.mem_req_valid(mreq),.mem_req_addr(addr),.mem_req_ready(mready),
.mem_resp_valid(mvalid),.mem_resp_index(index),.mem_resp_data(data),.mem_resp_complete(complete),.mem_resp_ok(1'b1),
.drained(drained),.fault(fault),.preflight_bad(bad));
always @(negedge clk)begin
 ce=cycle%7!=2&&cycle%13!=6;allow=cycle%11!=5;ordy=cycle%19>5;mready=!active&&cycle%5!=1;
 mvalid=active&&cycle%3!=1;index=beat;complete=mvalid&&beat==15;
 data={value(base+beat*8+6),value(base+beat*8+4),value(base+beat*8+2),value(base+beat*8)};
 valid=!reset&&init>=128&&sent<384;payload=item(sent);
 ww=!reset&&init<128;wa=init;
end
always @(posedge clk)begin
 cycle=cycle+1;if(cycle>30000)$fatal(1,"cache bounded watchdog");
 if(!reset)begin
  if(ww)init=init+1;
  if(mreq&&mready)begin active=1;base=addr;beat=0;reqs=reqs+1;end
  else if(mvalid)begin if(beat==15)active=0;else beat=beat+1;end
  if(bad||fault)$fatal(1,"cache contract");
  if(ce&&allow&&ready)sent<=sent+1;
  if(ov&&!(ce&&allow&&ordy))begin
   if(holds&&held!=={op,t3,t2,t1,t0})$fatal(1,"held data changed");held={op,t3,t2,t1,t0};holds=holds+1;
  end else holds=0;
  if(ce&&allow&&ordy&&ov)begin $display("VALUE %0d %018h %04h%04h%04h%04h",got,op,t3,t2,t1,t0);got=got+1;end
  if(got==384)begin $display("PASS groups=%0d cycles=%0d refills=%0d",got,cycle,reqs);$finish;end
 end
end
initial begin repeat(3)@(negedge clk);reset=0;end
endmodule
'''


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--export', type=Path, required=True)
    parser.add_argument('--baseline', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    files = {'baseline': args.baseline.resolve(), 'current': args.export.resolve()}
    hashes = {name: digest(path) for name, path in files.items()}
    rows = []
    traces = []
    for native in (False, True):
        for name, source in files.items():
            folder = output / f'{name}-{int(native)}'
            folder.mkdir()
            bench = folder / 'tb.v'
            bench.write_text(TB.replace('@NATIVE@', str(int(native))))
            compile_command = [os.environ['IVERILOG_EXE'], '-g2012', '-D__ICARUS__',
                               '-s', 'tb', '-o', str(folder / 'sim.vvp'), str(source), str(bench)]
            if native:
                compile_command.append(str(Path(os.environ['GOWIN_HOME']) / 'IDE/simlib/gw2a/prim_sim.v'))
            for stage, command in [('compile', compile_command),
                                   ('run', [os.environ['VVP_EXE'], str(folder / 'sim.vvp')])]:
                assert {key: digest(path) for key, path in files.items()} == hashes
                with (folder / f'{stage}.log').open('wb') as log:
                    result = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, timeout=60)
                assert result.returncode == 0, (folder / f'{stage}.log').read_text(errors='replace')[-1500:]
            lines = (folder / 'run.log').read_text().splitlines()
            trace = [line for line in lines if line.startswith('VALUE ')]
            assert len(trace) == 384 and not any('x' in line.lower() for line in trace)
            traces.append(trace)
            rows.append(dict(native=native, source=name,
                             summary=[line for line in lines if line.startswith('PASS ')]))
    assert all(trace == traces[0] for trace in traces), 'cache output changed'
    receipt = dict(status='pass', groups=384, source_sha256=hashes, cases=rows,
                   exact_values_match=True, test_sha256=digest(Path(__file__)))
    (output / 'receipt.json').write_text(json.dumps(receipt, indent=2))
    print(json.dumps(receipt))


if __name__ == '__main__':
    main()
