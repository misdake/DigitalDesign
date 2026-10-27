"""Independent bounded Group35 reference: physical address, masks, parity and owner.

All levels, taps and local tile-boundary combinations are checked while CE
pauses the two-edge packet pipeline. No baseline DUT result supplies the oracle.
"""
from pathlib import Path
import argparse
import hashlib
import json
import os
import random
import subprocess

TB = r'''
`timescale 1ns/1ps
module tb;
reg clk=0,reset=1,ce=1,valid=0;reg[75:0]member=0;reg[1:0]tap=0;
reg[15:0]base=0;reg mip=0;wire ready,ov,fault;wire[34:0]packet;
reg[94:0]stim[0:@LAST@];reg[34:0]expected[0:@LAST@];
integer cycle=0,issued=0,returned=0,first=-1;
packet_leaf_unique dut(.clk(clk),.reset(reset),.ce(ce),.in_valid(valid),.in_member(member),.in_tap(tap),
 .texture_base(base),.texture_mip(mip),.in_ready(ready),.out_valid(ov),.out_packet(packet),.fault(fault));
always #5 clk=~clk;
always @(negedge clk)begin
 ce=cycle%7!=2;
 if(issued<@COUNT@)begin {mip,base,tap,member}=stim[issued];valid=1;end else valid=0;
end
always @(posedge clk)begin
 cycle=cycle+1;if(cycle>3000)$fatal(1,"packet watchdog");
 if(!reset)begin
  if(fault)$fatal(1,"packet fault");
  if(ce&&ov)begin
   if(returned>=@COUNT@||packet!==expected[returned])$fatal(1,"packet%0d got%h expected%h",returned,packet,expected[returned]);
   returned=returned+1;
  end
  if(ready&&valid)begin issued=issued+1;if(first<0)first=cycle;end
 end
end
initial begin
 $readmemh("stim.hex",stim);$readmemh("expected.hex",expected);
 repeat(2)@(posedge clk);#1;reset=0;
 while(returned<@COUNT@)begin @(negedge clk);#1;end
 $display("PASS packet groups=%0d cycles=%0d",returned,cycle);$finish;
end
endmodule
'''


def fixtures():
    rng = random.Random(3741)
    rows = []
    for n in range(11):
        prefix = sum(1 << (2 * max(level-3, 0)) for level in range(n))
        for tap in range(4):
            for boundary in range(4):
                lx, ly = (7 if boundary&1 else 2), (7 if boundary&2 else 4)
                mask = rng.randrange(1, 16) | (1 << tap)
                tx, ty = rng.randrange(128), rng.randrange(128)
                quad, lane = rng.randrange(16), rng.randrange(4)
                first, last = rng.randrange(2), rng.randrange(2)
                mip, base = rng.randrange(2), rng.randrange(65536)
                member = rng.getrandbits(36) | (mask<<36) | (tx<<40) | (ty<<47) | (lx<<54) | (ly<<57)
                member |= (n<<64) | (quad<<68) | (lane<<72) | (first<<74) | (last<<75)
                x, y = tap&1, tap>>1
                sx, sy = n<=3 or lx!=7, n<=3 or ly!=7
                side = 1 << max(n-3, 0)
                tile_x, tile_y = (tx+(x and not sx))%side, (ty+(y and not sy))%side
                sector = (base + (prefix if mip else 0) + tile_y*side + tile_x) & 65535
                coverage = sum(int((x or sx) if i&1 else (not x or sx)) *
                               int((y or sy) if i&2 else (not y or sy)) << i for i in range(4))
                first = first and tap==0
                last = last and (mask >> (tap+1))==0
                owner = (lane<<6) | (quad<<2) | (int(last)<<1) | int(first)
                result = sector | (lx<<16) | (ly<<19) | (owner<<22) | ((n&1)<<30) | (coverage<<31)
                rows.append((member | (tap<<76) | (base<<78) | (mip<<94), result))
    return rows


def main():
    ap = argparse.ArgumentParser(__doc__)
    ap.add_argument('--export', type=Path, required=True)
    ap.add_argument('--output', type=Path, required=True)
    ap.add_argument('--native', action='store_true')
    args = ap.parse_args()
    out = args.output.resolve(); out.mkdir(parents=True, exist_ok=False)
    source = args.export.resolve(); identity = hashlib.sha256(source.read_bytes()).hexdigest()
    rows = fixtures()
    (out/'stim.hex').write_text(''.join(f'{s:024x}\n' for s,_ in rows))
    (out/'expected.hex').write_text(''.join(f'{e:09x}\n' for _,e in rows))
    (out/'tb.v').write_text(TB.replace('@LAST@',str(len(rows)-1)).replace('@COUNT@',str(len(rows))))
    command = [os.environ['IVERILOG_EXE'],'-g2012','-s','tb','-o',out/'sim.vvp',source,out/'tb.v']
    if args.native:command.append(Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v')
    cases = []
    for label, cmd in [('compile',command),('run',[os.environ['VVP_EXE'],out/'sim.vvp'])]:
        assert hashlib.sha256(source.read_bytes()).hexdigest()==identity
        with (out/(label+'.log')).open('wb') as log:
            r = subprocess.run(list(map(str,cmd)),cwd=out,stdout=log,stderr=subprocess.STDOUT,timeout=60)
        assert r.returncode==0,(out/(label+'.log')).read_text(errors='replace')[-2000:]
        cases += [line for line in (out/(label+'.log')).read_text().splitlines() if line.startswith('PASS ')]
    receipt = dict(status='pass',native=args.native,groups=len(rows),export_sha256=identity,cases=cases,
                   test_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest())
    (out/'receipt.json').write_text(json.dumps(receipt,indent=2));print(json.dumps(receipt))


if __name__ == '__main__':main()
