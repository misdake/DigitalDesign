"""Exhaustive check of actual Vertex normal transport against frozen math.

The 18-bit MAC output window is enumerated in both normal modes, including
normal16's prior two-bit discard, signed wrapping, ties and saturation.
"""
from pathlib import Path
import argparse
import hashlib
import importlib.util
import json
import os
import re
import subprocess
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[3]


def main():
    ap = argparse.ArgumentParser(__doc__)
    ap.add_argument('--tag', required=True)
    args = ap.parse_args()
    assert re.fullmatch(r'[A-Za-z0-9_.-]+', args.tag)
    out = ROOT/'target/gpu-vliw-normal-quantization'/args.tag
    out.mkdir(parents=True, exist_ok=False)
    source = (ROOT/'ip/gpu-v2/src/frontend/vliw/frontend.v').read_text()
    prefix = source[source.index(' wire [17:0] normal16_pack='):source.index(' reg [53:0] packed_buffer;')]
    spec = importlib.util.spec_from_file_location('frozen_normal_math', ROOT/'ip/gpu-v2/examples/vliw_integration/reference/math/model.py')
    math = importlib.util.module_from_spec(spec);sys.modules[spec.name] = math;spec.loader.exec_module(math)
    expected = []
    for mode in (0, 1):
        for raw in range(1 << 18):
            if mode:
                old = raw-(1 << 18) if raw & (1 << 17) else raw
            else:
                old = raw >> 2
                if old & (1 << 15): old -= 1 << 16
                old *= 4
            expected.append(math.normal12_from18(old) & 4095)
    (out/'expected.hex').write_text(''.join(f'{v:03x}\n' for v in expected))
    tb = """`timescale 1ns/1ps
module tb;
reg [31:0] pack_value=0;reg normal18=0;
@PREFIX@
reg [11:0] expected[0:524287];integer i;
initial begin
 $readmemh("expected.hex",expected);
 for(i=0;i<524288;i=i+1)begin
  normal18=i>=262144;pack_value=i&262143;#1;
  if(packed_normal12!==expected[i])$fatal(1,"normal code %0d got %h expected %h",i,packed_normal12,expected[i]);
 end
 $display("PASS NORMAL exhaustive=524288 modes=2");$finish;
end
initial begin #524300;$fatal(1,"absolute quantizer bound");end
endmodule
""".replace('@PREFIX@', prefix)
    (out/'tb.v').write_text(tb)
    for label,command in [('compile',[os.environ.get('IVERILOG_EXE','iverilog'),'-g2012','-s','tb','-o','sim.vvp','tb.v']),
                          ('run',[os.environ.get('VVP_EXE','vvp'),'sim.vvp'])]:
        r = subprocess.run(command, cwd=out, capture_output=True, timeout=90)
        (out/(label+'.log')).write_bytes(r.stdout+r.stderr)
        assert r.returncode == 0, (r.stdout+r.stderr).decode(errors='replace')[-4000:]
    receipt = dict(status='pass', cases=len(expected), modes=2, log=r.stdout.decode(),
        production_scalar_sha256=hashlib.sha256(prefix.encode()).hexdigest(),
        source_sha256=hashlib.sha256(source.encode()).hexdigest())
    (out/'receipt.json').write_text(json.dumps(receipt,indent=2));print(json.dumps(receipt))


if __name__ == '__main__':
    main()
