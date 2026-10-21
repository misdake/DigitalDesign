"""Bounded native/portable math initialization and mutable H-row regression."""
from pathlib import Path
import argparse
import hashlib
import importlib.util
import json
import os
import subprocess

ROOT = Path(__file__).resolve().parents[3]

BENCH = r'''`timescale 1ns/1ps
module tb;
 reg clk=0;always #5 clk=~clk;
`ifndef PORTABLE
 GSR GSR(.GSRI(1'b1));
`endif
 reg reset=1,ce=1,we=0,re=0;reg[1:0]wa=0;
 reg[31:0]wd=0;reg[8:0]ra=0;wire[35:0]q;
 iz_numeric_ram dut(.*);
 reg[35:0]gold[0:255];integer i,cycle=0;reg[35:0]held;
 always @(posedge clk)begin cycle=cycle+1;if(cycle>5000)$fatal(1,"numeric RAM watchdog");end
 task check;
  input[8:0]address;input[35:0]value;
  begin
   @(negedge clk);ce=1;re=1;ra=address;
   @(posedge clk);#1;if(q!==value)$fatal(1,"numeric RAM address%0d got%h expected%h",address,q,value);
   @(negedge clk);re=0;
  end
 endtask
 initial begin
  $readmemh("gold.hex",gold);
  repeat(2)@(negedge clk);reset=0;
  for(i=0;i<256;i=i+1)check(i,gold[i]);
  for(i=0;i<4;i=i+1)begin
   @(negedge clk);we=1;wa=i;wd=32'habcdef00+i;
   @(posedge clk);#1;@(negedge clk);we=0;
  end
  for(i=0;i<4;i=i+1)check(256+i,36'h0abcdef00+i);
  held=q;ce=0;we=1;wa=3;wd=32'hdeadbeef;re=1;ra=0;
  repeat(4)begin @(posedge clk);#1;if(q!==held)$fatal(1,"CE0 changed numeric Q");end
  @(negedge clk);we=0;re=0;ce=1;reset=1;
  @(posedge clk);#1;if(q!==0)$fatal(1,"numeric Q reset");
  @(negedge clk);reset=0;
  for(i=0;i<256;i=i+1)check(i,gold[i]);
  for(i=0;i<4;i=i+1)check(256+i,36'h0abcdef00+i);
  $display("PASS NUMERIC_RAM constants=256 reads=520 h_rows=4 reset=1 ce0=4 cycles=%0d",cycle);$finish;
 end
 initial begin #50010;$fatal(1,"numeric RAM time watchdog");end
endmodule
'''


def main():
    ap = argparse.ArgumentParser(__doc__)
    ap.add_argument('--export', type=Path, required=True)
    ap.add_argument('--output', type=Path, required=True)
    ap.add_argument('--native', action='store_true')
    a = ap.parse_args()
    out = a.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    model_path = ROOT/'ip/gpu-v2/examples/vliw_integration/reference/math/model.py'
    spec = importlib.util.spec_from_file_location('math_gold', model_path)
    model = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(model)
    words = [model.SLOPE[j] | model.SEED[j] << 9 | model.LOG[j >> 2] << 27
             for j in range(256)]
    (out/'gold.hex').write_text(''.join(f'{w:09x}\n' for w in words))
    (out/'tb.v').write_text(BENCH)
    source = a.export.resolve()
    identity = hashlib.sha256(source.read_bytes()).hexdigest()
    cmd = [os.environ['IVERILOG_EXE'], '-g2012', '-s', 'tb', '-o', str(out/'sim.vvp')]
    if a.native:
        cmd.append(str(Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v'))
    cmd += [str(source), str(out/'tb.v')]
    for label, command in [('compile', cmd), ('run', [os.environ['VVP_EXE'], str(out/'sim.vvp')])]:
        with (out/(label+'.log')).open('wb') as log:
            p = subprocess.run(command, cwd=out, stdout=log, stderr=subprocess.STDOUT, timeout=60)
        assert p.returncode == 0, (out/(label+'.log')).read_text(errors='replace')[-2400:]
    assert identity == hashlib.sha256(source.read_bytes()).hexdigest()
    result = next(l for l in (out/'run.log').read_text().splitlines() if l.startswith('PASS NUMERIC_RAM'))
    receipt = dict(status='pass', native=a.native, result=result, export_sha256=identity,
                   oracle_sha256=hashlib.sha256(model_path.read_bytes()).hexdigest())
    (out/'receipt.json').write_text(json.dumps(receipt, indent=2))
    print(json.dumps(receipt))


if __name__ == '__main__':
    main()
