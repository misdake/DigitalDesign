"""Bounded current-descriptor cache and old/new sampler arithmetic checks.

Inputs are exact exported production sources. Original logs stay beside the
receipt; only small PASS summaries are printed. No scene/DUT source is mutated.
"""
from pathlib import Path
import argparse
import hashlib
import json
import os
import subprocess


CACHE = r'''
`timescale 1ns/1ps
module tb;
reg clk=0,reset=1,ce=1,valid=0,inv=0,mip=1,out_ready=0;
reg[15:0]base=16'h20;reg[33:0]payload=0;
reg weight_write=0;reg[6:0]weight_address=0;reg[35:0]weight_value={4{9'd64}};
wire ready,ov,fault,preflight,drained,mreq;wire[71:0]op;
wire[15:0]t0,t1,t2,t3;wire[31:0]addr;
reg mrdy=0,rv=0,terminal=0;reg[3:0]index=0;reg[63:0]data=0;
integer cycle=0,requests=0,returns=0,beat=0,req_addr=0,stall=0,holds=0;
reg active=0,blue=0;reg[71:0]held_payload;reg[63:0]held_tex;
gpu_v2_texture_runtime_cache_unique dut(
 .clk(clk),.reset(reset),.proposal_ce(ce),.commit_allow(1'b1),.abort(1'b0),
 .in_valid(valid),.in_payload(payload),.texture_base(base),.texture_mip(mip),.texture_invalidate(inv),
 .weight_write(weight_write),.weight_address(weight_address),.weight_value(weight_value),
 .next_valid(1'b0),.next_key(18'd0),.in_ready(ready),.out_ready(out_ready),.out_valid(ov),.out_payload(op),
 .out_tex0(t0),.out_tex1(t1),.out_tex2(t2),.out_tex3(t3),
 .mem_req_valid(mreq),.mem_req_addr(addr),.mem_req_ready(mrdy),.mem_resp_valid(rv),
 .mem_resp_index(index),.mem_resp_data(data),.mem_resp_complete(terminal),.mem_resp_ok(1'b1),
 .drained(drained),.fault(fault),.preflight_bad(preflight));
always #5 clk=~clk;
always @(negedge clk)begin
 ce=cycle%7!=2;mrdy=!active&&cycle%5!=1;
 rv=active&&cycle%3!=1;index=beat;terminal=rv&&beat==15;
 if(req_addr<32'h3000)data={4{16'hf800}};
 else if(req_addr<32'h5000)data={4{blue?16'h001f:16'h07e0}};
 else data={4{16'had55}};
end
always @(posedge clk)begin
 cycle=cycle+1;
 if(cycle>4000)$fatal(1,"cache watchdog");
 if(!reset)begin
  if(fault||preflight)$fatal(1,"cache fault");
  if(mreq&&mrdy)begin
   if(active)$fatal(1,"two refills");
   active=1;req_addr=addr;beat=0;requests=requests+1;
  end else if(rv)begin if(beat==15)active=0;else beat=beat+1;end
  if(ov&&!out_ready)begin
   if(holds&&{t3,t2,t1,t0}!==held_tex)$fatal(1,"held texels changed");
   held_tex={t3,t2,t1,t0};held_payload=op;holds=holds+1;
  end else holds=0;
 end
end
task tick;begin @(negedge clk);#1;end endtask
task invalidate;begin
 while(!ce)tick;
 inv=1;@(posedge clk);#1;inv=0;
 if(dut.state_initialized!==0||dut.tag_valid!==0)$fatal(1,"invalidate missed masks");
end endtask
task sample;
 input[3:0]n;input[15:0]expected;input integer expected_requests;
 begin
  payload={30'd0,n};valid=1;
  while(!ready)begin @(posedge clk);#1;if(!ready)tick;end
  // in_ready is the paid edge enable. Hold through that physical edge.
  @(posedge clk);#1;valid=0;
  while(!ov)tick;
  if({t3,t2,t1,t0}!=={4{expected}})$fatal(1,"bad cache value got%h expected%h",t0,expected);
  repeat(3)tick;
  out_ready=1;while(!ce)tick;
  @(posedge clk);#1;out_ready=0;
  if(requests!=expected_requests)$fatal(1,"requests got%0d expected%0d",requests,expected_requests);
  if(!drained)$fatal(1,"cache still refilling");
  returns=returns+1;tick;
 end
endtask
initial begin
 tick;tick;reset=0;
 weight_write=1;while(!ce)tick;@(posedge clk);#1;weight_write=0;
 sample(6,16'hf800,1);sample(6,16'hf800,1);
 base=16'h60;invalidate;sample(6,16'h07e0,2);sample(6,16'h07e0,2);
 // No implicit invalidation: deliberately observe retained data before the
 // explicit driver command, then verify an in-place update is made visible.
 blue=1;sample(6,16'h07e0,2);invalidate;sample(6,16'h001f,3);
 base=16'h20;invalidate;sample(6,16'hf800,4);
 base=16'ha0;mip=0;invalidate;sample(2,16'had55,5);
 $display("PASS cache samples=%0d refills=%0d cycles=%0d",returns,requests,cycle);$finish;
end
endmodule
'''

COORD = r'''
`timescale 1ns/1ps
module tb;
reg clk=0,reset=1,ce=1,valid=0,ready_out=1;reg[31:0]uv=0;
reg[7:0]lod=0;reg[3:0]n=0;reg mip=0;reg[1:0]filter=0;
wire ready,ov,fault,bad;wire[37:0]owner;wire[35:0]result;
integer cycle=0,emitted=0,k=0;
gpu_v2_texture_direct_lod dut(.clk(clk),.reset(reset),.ce(ce),.commit_allow(1'b1),
 .in_valid(valid),.in_uv(uv),.in_quad(4'd3),.in_mask(4'd1),@SLOT@.in_lane(2'd0),
 .in_lod(lod),.in_max_n(n),.in_has_mip(mip),.in_filter(filter),.in_ready(ready),
 .out_ready(ready_out),.out_valid(ov),.out_owner(owner),.out_result(result),.fault(fault),.preflight_bad(bad));
always #5 clk=~clk;
always @(negedge clk)begin ce=cycle%7!=2;ready_out=cycle%5!=1;end
always @(posedge clk)begin
 cycle=cycle+1;if(cycle>5000)$fatal(1,"coordinate watchdog");
 if(!reset)begin
  if(fault||bad)$fatal(1,"coordinate fault");
  if(ce&&ov&&ready_out)begin $display("VALUE %010h %09h",owner,result);emitted=emitted+1;end
 end
end
task tick;begin @(negedge clk);#1;end endtask
initial begin
 tick;tick;reset=0;
 for(k=0;k<24;k=k+1)begin
  n=k%11;mip=(k/3)%2;filter=k%3;lod=k*13;uv=32'h9a734bc5^((k+1)*32'h2ef397b1);valid=1;
  while(!ce||!ready)tick;
  @(posedge clk);#1;valid=0;
  while(emitted<=k)tick;
 end
 $display("PASS coordinates samples=%0d cycles=%0d",emitted,cycle);$finish;
end
endmodule
'''


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--export', type=Path, required=True)
    parser.add_argument('--baseline', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--native', action='store_true')
    parser.add_argument('--physical', action='store_true', help='Producer stores physical Group35 instead of logical Group34')
    args = parser.parse_args()
    out = args.output.resolve(); out.mkdir(parents=True, exist_ok=False)
    files = dict(current=args.export.resolve(), baseline=args.baseline.resolve())
    identities = {k: sha(v) for k, v in files.items()}
    results = {}
    cache = CACHE
    if args.physical:
        cache = cache.replace('reg[33:0]payload=0', 'reg[34:0]payload=0')
        cache = cache.replace('.texture_base(base),.texture_mip(mip),', '')
        cache = cache.replace('.next_key(18\'d0)', '.next_key(16\'d0)')
        cache = cache.replace('task tick;', '''function[15:0]ref_prefix;
 input[3:0]n;integer level;reg[15:0]total;
 begin total=0;for(level=0;level<n;level=level+1)
 total=total+((level<3)?1:(1<<(2*(level-3))));ref_prefix=total;end
endfunction
task tick;''')
        cache = cache.replace("payload={30'd0,n};valid=1;",
            "payload[15:0]=base+(mip?ref_prefix(n):16'd0);payload[34:16]={4'hf,n[0],8'd0,6'd0};valid=1;")
        cache = cache.replace("base=16'h60;invalidate;sample(6,16'h07e0,2);sample(6,16'h07e0,2);",
            "base=16'h60;sample(6,16'h07e0,2);base=16'h20;sample(6,16'hf800,2);base=16'h60;sample(6,16'h07e0,2);")
    for label, source, bench in [('cache', files['current'], cache),
                                 ('coord-current', files['current'], COORD.replace('@SLOT@', '')),
                                 ('coord-baseline', files['baseline'], COORD.replace('@SLOT@', ".in_slot(4'd0),"))]:
        d = out / label; d.mkdir(); (d/'tb.v').write_text(bench)
        command = [os.environ['IVERILOG_EXE'], '-g2012', '-s', 'tb', '-o', d/'sim.vvp', source, d/'tb.v']
        if args.native:
            command.append(Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v')
        for name, cmd in [('compile', command), ('run', [os.environ['VVP_EXE'], d/'sim.vvp'])]:
            assert {k: sha(v) for k, v in files.items()} == identities
            with (d/(name+'.log')).open('wb') as log:
                r = subprocess.run(list(map(str, cmd)), cwd=d, stdout=log, stderr=subprocess.STDOUT, timeout=60)
            assert r.returncode == 0, (d/(name+'.log')).read_text(errors='replace')[-2500:]
        log = (d/'run.log').read_text()
        results[label] = [line for line in log.splitlines() if line.startswith('PASS ')]
        assert results[label]
    def values(label):
        return [l for l in (out/label/'run.log').read_text().splitlines() if l.startswith('VALUE ')]
    assert len(values('coord-current')) == 24
    assert values('coord-current') == values('coord-baseline'), 'Sampler numerical output changed'
    receipt = dict(status='pass', native=args.native, physical=args.physical, exports=identities, results=results,
                   sampler_equivalence_samples=24, test_sha256=sha(Path(__file__)))
    (out/'receipt.json').write_text(json.dumps(receipt, indent=2))
    print(json.dumps(receipt))


if __name__ == '__main__':
    main()
