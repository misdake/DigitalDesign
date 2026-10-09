"""Reproduce the accepted 8-KiB, eight-DPB framebuffer component at 54 MHz.

The fit includes held shader/memory stimuli and synthetic execution, not cache
control, production depth/blend arithmetic or the full system.
"""
from pathlib import Path
import argparse
import hashlib
import json
import os
import re
import subprocess
import uuid

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--gowin-home', type=Path, default=os.environ.get('GOWIN_HOME'))
p.add_argument('--iverilog', default=os.environ.get('IVERILOG_EXE', 'iverilog'))
p.add_argument('--vvp', default=os.environ.get('VVP_EXE', 'vvp'))
p.add_argument('--bank-order', type=int, choices=[0,1], default=1)
p.add_argument('--execution-harness', choices=['rgb565-xor','rgba-fold'], default='rgba-fold')
p.add_argument('--source-dir', type=Path)
p.add_argument('--simulate-only', action='store_true')
args = p.parse_args()
if args.gowin_home is None:
    p.error('set GOWIN_HOME or pass --gowin-home')
repo = Path(__file__).resolve().parents[1]
source = args.source_dir or repo / 'systems/cpu-v3-tang-nano-20k/src/hardware/gpu'
out = repo / 'target' / ('framebuffer-dpb-' + uuid.uuid4().hex[:8])
out.mkdir(parents=True)
print('Framebuffer output: ' + str(out), flush=True)
files = ['framebuffer_lane_array.v', 'fused_framebuffer_pipe.v', 'fused_framebuffer_tb.v']
for name in files:
    (out / name).write_bytes((source / name).read_bytes())
(out / 'driver.py').write_bytes(Path(__file__).read_bytes())
slot_bits = 2
(out / 'probe.v').write_text('''module fused_probe(input wire clk,input wire reset,output reg [31:0] signature=0);
reg [223:0] source=224'h8617_c981_a765_1329_a571_319a_6342_751c_f752_d371_b391_a613_78c1_a349;
reg [63:0] memory=64'h2387_291a_634f_a357;
wire ir,ev,cv,mwr,mrr,mv;wire [3:0] cm,em;wire [63:0] oc,oz,md,ez;wire [127:0] ec;
wire [63:0] result_c;wire [3:0] result_m;
genvar lane;
generate for(lane=0;lane<4;lane=lane+1) begin: execution
 assign result_c[16*lane +:16]=COLOR_EXPRESSION^oc[16*lane +:16];
 assign result_m[lane]=ez[16*lane +:16]<oz[16*lane +:16];
end endgenerate
always @(posedge clk) begin
 if(ir) source<={source[222:0],source[223]^source[221]^source[219]^source[218]};
 memory<={memory[62:0],memory[63]^memory[61]^memory[60]^memory[58]};
 signature<=signature^{23'b0,ir,ev,cv,mwr,mrr,mv,cm[2:0]}^{28'b0,cm}
   ^(ev ? oc[31:0]^oc[63:32]^oz[31:0]^oz[63:32] : 32'b0)
   ^(mv ? md[31:0]^md[63:32] : 32'b0);
end
FusedFramebufferPipe #(.SLOT_BITS(SLOT_VALUE),.HALF_CAPACITY(1),.BANK_ORDER(BANK_VALUE)) dut (
 .clk(clk),.reset(reset),.input_valid(1'b1),.input_ready(ir),.resident(1'b1),
 .input_group(source[192]),.input_way(source[193 +: SLOT_VALUE]),.input_x({source[204:202],1'b0}),.input_y({source[207:205],1'b0}),
 .input_colors(source[127:0]),.input_depths(source[191:128]),.input_mask(source[211:208]),
 .depth_enable(source[212]),.depth_write(source[213]),.depth_func(source[216:214]),.blend_mode(source[218:217]),
 .execute_valid(ev),.execute_ready(memory[0]),.execute_colors(ec),.execute_depths(ez),.execute_mask(em),.old_colors(oc),.old_depths(oz),.result_colors(result_c),.result_mask(result_m),
 .commit_valid(cv),.commit_ready(memory[1]),.commit_mask(cm),
 .memory_write_valid(memory[2]),.memory_write_ready(mwr),.memory_write_group(memory[3]),.memory_write_address({1'b0,memory[4 +: 9]}),.memory_write_data(memory),.memory_write_mask(memory[23:16]),
 .memory_read_valid(memory[24]),.memory_read_ready(mrr),.memory_read_group(memory[25]),.memory_read_address({1'b0,memory[26 +: 9]}),
 .memory_response_valid(mv),.memory_response_ready(memory[40]),.memory_response_data(md));
endmodule
'''.replace('SLOT_VALUE', str(slot_bits)).replace('BANK_VALUE',str(args.bank_order)).replace('COLOR_EXPRESSION', 'ec[32*lane +:16]^ec[32*lane+16 +:16]' if args.execution_harness=='rgba-fold' else '{ec[32*lane+27 +:5],ec[32*lane+18 +:6],ec[32*lane+11 +:5]}'), encoding='utf-8')

(out / 'probe.sdc').write_text('create_clock -name clk -period 18.518519 [get_ports {clk}]\n', encoding='utf-8')
manifest = dict(execution_harness=args.execution_harness, bank_order=args.bank_order, clock_mhz=54, bsram=8, capacity_bytes=8192, tile_entries=8,
                source_held_until_actual_commit=True, numerical_queue_bsram=0,
                boundary='array + transaction owner + held shader/memory/signature harness; synthetic execution; controller external')
manifest['sha256'] = {f.name: hashlib.sha256(f.read_bytes()).hexdigest() for f in out.iterdir() if f.is_file()}
(out / 'manifest.json').write_text(json.dumps(manifest, indent=2)+'\n', encoding='utf-8')
primitive = args.gowin_home / 'IDE/simlib/gw2a/prim_sim.v'
subprocess.run([args.iverilog, '-g2012', '-s', 'fused_framebuffer_tb', f'-Pfused_framebuffer_tb.BANK_ORDER={args.bank_order}', f'-Pfused_framebuffer_tb.EXEC_FOLD={int(args.execution_harness == "rgba-fold")}', '-o', str(out / 'test.vvp'),
                *[str(out / f) for f in files], str(primitive)], check=True, timeout=120)
test = subprocess.run([args.vvp, str(out / 'test.vvp')], capture_output=True, text=True, timeout=120)
(out / 'native.log').write_text(test.stdout + test.stderr, encoding='utf-8')
print(test.stdout + test.stderr, flush=True)
test.check_returncode()
assert 'PASS fused framebuffer:' in test.stdout
if args.simulate_only:
    raise SystemExit(0)
(out / 'build.tcl').write_text('''set here [file normalize [file dirname [info script]]]
cd $here
set_device -name GW2AR-18C -device_version C GW2AR-LV18QN88C8/I7
add_file [file join $here framebuffer_lane_array.v]
add_file [file join $here fused_framebuffer_pipe.v]
add_file [file join $here probe.v]
add_file [file join $here probe.sdc]
set_option -synthesis_tool gowinsynthesis
set_option -top_module fused_probe
set_option -output_base_name fused_probe
set_option -verilog_std v2001
set_option -gen_text_timing_rpt 1
set_option -place_option 1
set_option -route_option 1
run all
''', encoding='utf-8')
with (out / 'build.log').open('w', encoding='utf-8') as log:
    subprocess.run([str(args.gowin_home / 'IDE/bin/gw_sh.exe'), str(out / 'build.tcl')], cwd=out,
                   stdout=log, stderr=subprocess.STDOUT, check=True, timeout=600)
assert 'does not have a driver' not in (out / 'build.log').read_text(encoding='utf-8')
report = (out / 'impl/pnr/fused_probe.rpt.txt').read_text(encoding='utf-8')
timing = (out / 'impl/pnr/fused_probe.tr').read_text(encoding='utf-8')
def number(pattern, document):
    return re.search(pattern, document).group(1)
metrics = dict(
    logic=int(number(r'Logic\s+\| (\d+)/', report)),
    lut=int(number(r'\((\d+) LUT,', report)),
    alu=int(number(r'LUT, (\d+) ALU,', report)),
    ram16=int(number(r'--SSRAM\(RAM16\)\s+\| (\d+)', report)),
    ff=int(number(r'--Logic Register as FF\s+\| (\d+)/', report)),
    bsram=int(number(r'--DPB\s+\| (\d+)', report)),
    fmax_mhz=float(number(r'54\.000\(MHz\)\s+(\d+\.\d+)\(MHz\)', timing)),
)
assert metrics['bsram'] == 8 and metrics['fmax_mhz'] >= 54, metrics
assert metrics['logic'] == metrics['lut'] + metrics['alu'] + 6*metrics['ram16'], metrics
for kind in ['Setup', 'Hold']:
    assert number(r'<Numbers of ' + kind + r' Violated Endpoints>:(\d+)', timing) == '0'
for kind in ['setup', 'hold']:
    assert re.search(r'clk\s+' + kind + r'\s+0\.000\s+0\s', timing), kind
(out / 'metrics.json').write_text(json.dumps(metrics, indent=2)+'\n', encoding='utf-8')
print('PASS component PnR at 54 MHz: ' + json.dumps(metrics), flush=True)
