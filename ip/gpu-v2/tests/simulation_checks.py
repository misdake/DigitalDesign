"""Check one production datapath with passive simulation monitors on and off.

The bounded three-draw fixture exercises CE pauses, backpressure, DMA and all
texture filters. Compare every top-level output at every sampled cycle. An
invalid packet also proves that the release path contains no hidden rejection.
"""
from pathlib import Path
import argparse
import hashlib
import json
import os
import subprocess
import sys

sys.dont_write_bytecode = True

TB = r'''
`timescale 1ns/1ps
module tb;
reg clk=0,reset=1,ce=1,in_valid=0;
always #5 clk=~clk;
wire in_ready,out_valid,fault;wire[34:0]out_packet;
reg seen=0;
always @(posedge clk)if(!reset&&out_valid)seen<=1;
packet_leaf_unique dut(.clk(clk),.reset(reset),.ce(ce),.in_valid(in_valid),
 .in_member(76'd0),.in_tap(2'd0),.texture_base(16'd0),.texture_mip(1'b0),
 .in_ready(in_ready),.out_valid(out_valid),.out_packet(out_packet),.fault(fault));
initial begin
 repeat(3)@(negedge clk);reset=0;in_valid=1;
 @(negedge clk);in_valid=0;
 repeat(4)@(negedge clk);
 if(fault!==0)$fatal(1,"diagnostic feedback survived in the production path");
 if(!seen)$fatal(1,"hidden rejection remained in the release packet pipeline");
 $display("PASS release packet remains a pipeline; invalid input has no recovery contract");
 $finish;
end
initial begin #1000;$fatal(1,"packet monitor wall bound");end
endmodule
'''


def main():
    ap = argparse.ArgumentParser(__doc__)
    ap.add_argument('--export', required=True, type=Path)
    ap.add_argument('--output', required=True, type=Path)
    ap.add_argument('--baseline', type=Path, help='Optional checked export from before the passive-monitor change')
    args = ap.parse_args()
    source = args.export.resolve()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    identity = hashlib.sha256(source.read_bytes()).hexdigest()

    def run(label, cmd, expected=None):
        assert hashlib.sha256(source.read_bytes()).hexdigest() == identity
        with (out/(label+'.log')).open('wb') as log:
            result = subprocess.run(list(map(str, cmd)), cwd=out, stdout=log,
                                    stderr=subprocess.STDOUT, timeout=90)
        text = (out/(label+'.log')).read_text(errors='replace')
        if expected is None:
            assert result.returncode == 0, text[-1800:]
        else:
            assert result.returncode != 0 and expected in text, text[-1800:]

    child = Path(__file__).with_name('current_texture_command.py')
    for mode in ('on', 'off'):
        cmd = [sys.executable, child, '--export', source, '--output', out/mode,
               '--native', '--physical']
        if mode == 'off':
            cmd.append('--no-monitors')
        run('fixture-'+mode, cmd)
    a = (out/'on/valid.trace').read_bytes()
    b = (out/'off/valid.trace').read_bytes()
    assert a == b, 'Simulation-only diagnostics changed a top-level output or its cycle'
    if args.baseline:
        baseline = args.baseline.resolve()
        baseline_identity = hashlib.sha256(baseline.read_bytes()).hexdigest()
        run('fixture-baseline', [sys.executable, child, '--export', baseline,
            '--output', out/'baseline', '--native', '--physical', '--hardware-errors'])
        assert (out/'baseline/valid.trace').read_bytes() == a, 'Valid output/cycle differs from the checked baseline'
        assert hashlib.sha256(baseline.read_bytes()).hexdigest() == baseline_identity

    (out/'packet_tb.v').write_text(TB)
    for mode in ('on', 'off'):
        exe = out/('packet-'+mode+'.vvp')
        cmd = [os.environ['IVERILOG_EXE'], '-g2012', '-s', 'tb', '-o', exe,
               source, out/'packet_tb.v']
        if mode == 'off':
            cmd.insert(1, '-DSYNTHESIS')
        run('compile-packet-'+mode, cmd)
        run('packet-'+mode, [os.environ['VVP_EXE'], exe],
            'GPU contract: physical packet tap' if mode == 'on' else None)

    receipt = dict(status='pass', export_sha256=identity,
                   trace_sha256=hashlib.sha256(a).hexdigest(),
                   sampled_cycles=len(a.splitlines()),
                   scope='All command-profile top-level outputs, monitors on/off; illegal command and internal packet assertions; no full frame or board claim')
    (out/'receipt.json').write_text(json.dumps(receipt, indent=2))
    print(json.dumps(receipt))


if __name__ == '__main__':
    main()
