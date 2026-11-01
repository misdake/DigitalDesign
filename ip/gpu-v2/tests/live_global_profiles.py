"""Bounded export-equivalence, private-profile scope and generic context checks."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess


def modules(source):
    source = re.sub(r'/\*.*?\*/|//[^\n]*', '', source, flags=re.S)
    return [(name, re.findall(r'"(?:\\.|[^"\\])*"|[A-Za-z_$][\w$]*|\d+\x27[sS]?[bBdDhHoO][0-9a-fA-F_xXzZ?]+|\d+|\S', body))
            for body, name in re.findall(r'(^module\s+(\w+)\b.*?endmodule)', source, re.S | re.M)]


GENERIC_TB = r'''
`timescale 1ns/1ps
module tb;
reg clk=0;always #5 clk=~clk;
reg reset=1,ce=1,context_valid=0,in_valid=0,out_ready=1;
reg[15:0]context_epoch=1;reg[8:0]ambient=32;
reg[31:0]in_id=32'h31;
wire context_ready,in_ready,out_valid;
wire[31:0]out_id;wire[15:0]out_epoch;wire[8:0]out_g,out_h;
GSR GSR(.GSRI(1'b1));
gpu_v2_lighting dut(.clk(clk),.reset(reset),.ce(ce),
 .context_valid(context_valid),.context_ready(context_ready),
 .context_epoch(context_epoch),.context_mode(2'd1),.context_code(5'd12),
 .light_x(16'sd0),.light_y(-16'sd16384),.light_z(16'sd0),
 .ambient(ambient),.directional(9'd0),.ray_x(16'sd10000),
 .ray_y(16'sd12000),.ray_k(16'sd10000),
 .in_valid(in_valid),.in_ready(in_ready),.in_id(in_id),
 .in_row0(36'd0),.in_row1(36'd0),.in_row2(36'd0),
 .out_valid(out_valid),.out_ready(out_ready),.out_id(out_id),
 .out_epoch(out_epoch),.out_g(out_g),.out_h(out_h));
integer cycles=0;
always @(posedge clk)begin cycles=cycles+1;if(cycles>800)$fatal(1,"context watchdog");end
task tick;begin @(negedge clk);#1;end endtask
task check_old;begin
 if(!out_valid||out_id!=32'h31||out_epoch!=1||out_g!=32||out_h!=0)
  $fatal(1,"generic installed context changed under external replacement/CE0");
end endtask
initial begin
 tick;tick;reset=0;tick;
 if(in_ready)$fatal(1,"generic accepted pixel before context install");
 ce=0;context_valid=1;repeat(2)tick;
 if(context_ready||dut.configured)$fatal(1,"generic installed context during CE0");
 ce=1;#1;if(!context_ready)$fatal(1,"empty generic context not ready");
 tick;context_valid=0;
 while(!in_ready)tick;
 in_valid=1;tick;in_valid=0;
 // Changing the external globals without installation must not alter old work.
 context_epoch=2;ambient=128;
 while(!out_valid)tick;
 check_old;ce=0;out_ready=0;context_valid=1;
 repeat(4)begin tick;check_old;if(context_ready)$fatal(1,"CE0 context ready");end
 ce=1;out_ready=1;#1;
 if(context_ready)$fatal(1,"generic installed before old token drained");
 while(!context_ready)tick;
 tick;context_valid=0;
 while(!in_ready)tick;
 in_id=32'h32;in_valid=1;tick;in_valid=0;
 while(!out_valid)tick;
 if(out_id!=32'h32||out_epoch!=2||out_g!=128||out_h!=0)
  $fatal(1,"generic replacement context result");
 $display("PASS GENERIC_CONTEXT contexts=2 CE0_hold=4 cycles=%0d",cycles);$finish;
end
endmodule
'''


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--exports', type=Path, required=True)
    parser.add_argument('--base-exports', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    iv, vvp = os.environ['IVERILOG_EXE'], os.environ['VVP_EXE']

    def call(command, log):
        result = subprocess.run(list(map(str, command)), cwd=out,
                                capture_output=True, timeout=60)
        (out / log).write_bytes(result.stdout + result.stderr)
        assert result.returncode == 0, (out / log).read_text(errors='replace')[-2000:]

    def preprocess(source, name, synthesis):
        path = out / (name + '.v')
        path.write_text(source)
        preprocessed = out / (name + '.pp')
        call([iv, '-g2012', '-E', *(['-DSYNTHESIS'] if synthesis else []),
              '-o', preprocessed, path], name + '.log')
        return preprocessed.read_text()

    rows = []
    exports = sorted(args.exports.glob('*.v'))
    assert len(exports) == 10, 'expected the ten public factory exports'
    for source in exports:
        raw = source.read_text()
        for synthesis in [False, True]:
            name = source.stem + ('-synthesis' if synthesis else '-simulation')
            probe = raw + '\n`ifdef GPU_V2_LIVE_GLOBALS\nRB04_PROFILE_LEAK\n`endif\n'
            actual = preprocess(probe, name, synthesis)
            assert 'RB04_PROFILE_LEAK' not in actual, name
            actual_modules = modules(actual)
            if args.base_exports:
                base = preprocess((args.base_exports / source.name).read_text(),
                                  'base-' + name, synthesis)
                assert modules(base) == actual_modules, 'ordered module tokens changed: ' + name
            rows.append(dict(export=source.name, synthesis=synthesis,
                             modules=len(actual_modules), sha256=hashlib.sha256(source.read_bytes()).hexdigest()))
            if source.name.startswith('command-'):
                generic = (args.exports / source.name.replace('command-', 'generic-')).read_text()
                # Select the standalone public port/DSP profile explicitly. These
                # older caller-visible defines intentionally survive command exports;
                # do not clear either private live-global/trusted-frontend define.
                public_profile = '\n`undef GPU_V2_COMMAND_PORTS\n`undef GPU_V2_GOWIN_DSP\n'
                combined = preprocess(raw + public_profile + 'module rb04_scope_boundary;endmodule\n' + generic,
                                      'concatenated-' + name, synthesis)
                expected = modules(preprocess(generic, 'generic-alone-' + name, synthesis))
                combined_modules = modules(combined)
                boundary = next(i for i, (module, _) in enumerate(combined_modules)
                                if module == 'rb04_scope_boundary')
                assert combined_modules[boundary + 1:] == expected, 'private profile changed subsequent generic export'
    for backend in ['portable', 'native']:
        path = args.exports / ('generic-' + backend + '.v')
        tb = out / 'generic-context.v'
        tb.write_text(GENERIC_TB)
        executable = out / ('generic-' + backend + '.vvp')
        call([iv, '-g2012', '-s', 'tb', '-o', executable, path.resolve(), tb,
              Path(os.environ['GOWIN_HOME']) / 'IDE/simlib/gw2a/prim_sim.v'], backend + '-compile.log')
        call([vvp, executable], backend + '-run.log')
        assert 'PASS GENERIC_CONTEXT' in (out / (backend + '-run.log')).read_text()
    (out / 'receipt.json').write_text(json.dumps(dict(status='pass', profiles=rows,
                                                    generic_context=['portable', 'native']), indent=2))
    print('PASS: 20 profile modes, concatenated generic scope, native/portable installed contexts')


if __name__ == '__main__':
    main()
