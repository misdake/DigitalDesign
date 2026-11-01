"""Bounded command-coordinate/source-Q/queue qualification, with integer gold.

Consumes real command exports. This peer does not rasterize or model SDRAM.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import random
import re
import subprocess


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def vectors():
    rng = random.Random(0x52423032)
    edges = [0, 1, 127, 128, 255, 256, 32767, 32768, 65407, 65535,
             65536, 131071, 131072, 262143]
    rows = []
    for n in range(11):
        for mip in range(2):
            for filt in range(3):
                for lod in sorted({0, 1, 7, max(0, n * 8 - 1), n * 8, 255}):
                    for index in range(4):
                        u = edges[(len(rows) + index) % len(edges)]
                        v = rng.randrange(1 << 18)
                        rows.append((0, n, mip, filt, lod, u, v))
    for filt in range(2):
        for u in edges:
            for v in edges:
                rows.append((1, 9, 0, filt, 0, u, v))
    result = []
    for i, (view, n, mip, filt, lod, raw_u, raw_v) in enumerate(rows):
        quad, lane = (i // 4) % 16, i % 4
        mask = 15
        packed = 0
        for value, bits in [(view, 1), (n, 4), (mip, 1), (filt, 2),
                            (lod, 8), (quad, 4), (lane, 2), (mask, 4),
                            (raw_u, 18), (raw_v, 18)]:
            packed = (packed << bits) | value
        fine = max(0, n - lod // 8) if mip else n
        fraction = lod % 8 if mip and lod < n * 8 else 0
        parent = (fraction * 511 + 4) // 8 if filt == 2 else 0
        owner = ((511 - parent) | (parent << 9) | (int(filt == 0) << 18)
                 | (fine << 19) | (max(0, fine - 1) << 23)
                 | ((quad * 4 + lane) << 31) | (int(parent == 0) << 37))
        axes = []
        for raw, side in [(raw_u, 400), (raw_v, 240)]:
            if view:
                signed = raw if raw < 131072 else raw - 262144
                uv = min(65535, max(0, signed))
                value = min((side - 1) * 256,
                            max(0, uv * side // 256 - (0 if filt == 0 else 128)))
                axes.append((value // 256, value % 256))
            else:
                side = 1 << max(1, fine)
                value = (raw & 65535) * side // 256 - (0 if filt == 0 else 128)
                axes.append(((value // 256) % side, value % 256))
        (x, fx), (y, fy) = axes
        coordinates = fx | (fy << 8) | (x << 16) | (y << 26)
        result.append((packed, (owner << 36) | coordinates))
    return result


COMMON = r'''
reg clk=0,reset=1,ce=1,allow=1,iv=0,ready=0;always #5 clk=~clk;
@GSR@
reg view=0,mip=0;reg[3:0]n=0,q=0,mask=1;reg[1:0]filt=0,lane=0;
reg[7:0]lod=0;reg[17:0]ru=0,rv=0;
wire ir,advance,bad,fault,ov,issue,returned;wire[37:0]owner;wire[35:0]coords;
wire[1:0]phase;wire[4:0]occupied,completed;
@CLAMP@
gpu_v2_texture_direct_lod dut(.clk(clk),.reset(reset),.ce(ce),.commit_allow(allow),
 .texture_fb400240(view),.in_valid(iv),
 .in_uv(view?{view_clamp_uv(rv),view_clamp_uv(ru)}:{rv[15:0],ru[15:0]}),
 .in_quad(q),.in_mask(mask),.in_lane(lane),.in_lod(lod),.in_max_n(n),
 .in_has_mip(mip),.in_filter(filt),.in_ready(ir),.advance(advance),
 .preflight_bad(bad),.fault(fault),.out_ready(ready),.out_valid(ov),
 .out_owner(owner),.out_result(coords),.phase(phase),.issue(issue),.returned(returned),
 .occupied(occupied),.completed(completed));
'''

LEGAL = r'''`timescale 1ns/1ps
module tb;
@COMMON@
reg[61:0]stim[0:@LAST@];reg[73:0]gold[0:@LAST@];
integer cycle=0,sent=0,got=0,fd,pauses=0,holds=0,fulls=0,wraps=0,joins=0,returns=0;
reg held=0;reg[73:0]head;
reg[55:0]frozen_state;reg paid_edge;
always@(posedge clk)if(!reset)begin
 paid_edge=advance;
 frozen_state={dut.coord.phase,dut.coord.valid,dut.coord.result_bank,
  dut.queue.wp,dut.queue.rp,dut.queue.cp,dut.queue.count,dut.queue.ready};
 if(!advance&&(dut.queue.owner_write||dut.queue.result_write||dut.queue.consume))
  $fatal(1,"write escaped disabled commit");
 #1;if(!paid_edge&&frozen_state!=={dut.coord.phase,dut.coord.valid,dut.coord.result_bank,
  dut.queue.wp,dut.queue.rp,dut.queue.cp,dut.queue.count,dut.queue.ready})
  $fatal(1,"coordinate/queue storage escaped CE/commit hold");
end
always@(negedge clk)if(!reset)begin
 ce=cycle%7!=2&&cycle%11!=6;allow=cycle%43<37;
 ready=cycle>80&&cycle%97>15&&cycle%5!=1;iv=0;
 // Installed descriptor changes only after every old coordinate owner drains.
 if(ir&&sent<@COUNT@)begin
  if(stim[sent][61:54]=={view,n,mip,filt}||
     (occupied==0&&completed==0&&dut.coord.valid==0))begin
   {view,n,mip,filt,lod,q,lane,mask,ru,rv}=stim[sent];iv=cycle%13!=4;
  end
 end
 #1;$fdisplay(fd,"%h",{ir,advance,bad,fault,ov,owner,coords,phase,issue,returned,occupied,completed,
  dut.queue.owner_write,dut.queue.result_write,dut.queue.consume,
  dut.queue.write_row,dut.queue.return_row,dut.queue.read_row});
end
always@(posedge clk)if(!reset)begin
 cycle=cycle+1;if(cycle>20000)$fatal(1,"bounded coordinate/queue timeout");
 if(fault||bad)$fatal(1,"legal command fault");
 if(held&&(!ov||{owner,coords}!==head))$fatal(1,"head changed before consume");
 held=ov&&!(advance&&ready);head={owner,coords};
 if(!ce)pauses=pauses+1;if(ov&&!ready)holds=holds+1;if(occupied==6)fulls=fulls+1;
 if(issue)begin sent=sent+1;if(dut.queue.write_row==0)wraps=wraps+1;end
 if(returned&&dut.queue.consume)joins=joins+1;if(issue&&returned)returns=returns+1;
 if(advance&&ov&&ready)begin
  if(got>=@COUNT@||{owner,coords}!==gold[got])
   $fatal(1,"gold[%0d] got %h expected %h",got,{owner,coords},gold[got]);
  got=got+1;
 end
 if(got==@COUNT@)begin
  if(pauses<100||holds<100||fulls<10||wraps<10||joins<10||returns<10)
   $fatal(1,"missing CE/backpressure/full/wrap/simultaneous coverage");
  $display("PASS vectors=%0d cycles=%0d ce0=%0d holds=%0d full=%0d wraps=%0d return_take=%0d issue_return=%0d",
    got,cycle,pauses,holds,fulls,wraps,joins,returns);$finish;
 end
end
initial begin
 fd=$fopen("trace.txt","w");$readmemh("stim.hex",stim);$readmemh("gold.hex",gold);
 repeat(3)@(negedge clk);#1;reset=0;
end
endmodule
'''

NEGATIVE = r'''`timescale 1ns/1ps
module tb;
@COMMON@
integer which=0;
initial begin
 if(!$value$plusargs("CASE=%d",which))$fatal(1,"missing negative case");
 repeat(3)@(negedge clk);#1;reset=0;
 @(negedge clk);
 case(which)
  1:begin force dut.coord.phase=2'b10;force dut.coord.in_valid=1'b1;end
  2:begin force dut.queue.count=6;force dut.queue.issue=1'b1;end
  3:begin force dut.queue.returned=1'b1;end
  4:begin force dut.queue.take=1'b1;end
  5:begin while(!ir)@(negedge clk);iv=1;mask=0;end
  6:begin
   while(!ir)@(negedge clk);
   iv=1;ru=100;rv=200;
   @(posedge clk);@(negedge clk);iv=0;rv=201;
  end
  default:$fatal(1,"unknown negative case");
 endcase
 @(posedge clk);#1;
 if(fault!==0||bad!==0||advance!==1)$fatal(1,"release gained diagnostic feedback");
 if(which==2&&dut.queue.owner_write!==1)$fatal(1,"release rejected trusted issue");
 if(which==3&&dut.queue.result_write!==1)$fatal(1,"release rejected trusted return");
 if(which==4&&dut.queue.consume!==1)$fatal(1,"release rejected trusted take");
 $display("PASS release diagnostic-only case=%0d",which);$finish;
end
initial begin #1000;$fatal(1,"negative wall bound");end
endmodule
'''


def run(command, directory, label, expected=None):
    with (directory / (label + '.log')).open('wb') as log:
        result = subprocess.run(list(map(str, command)), cwd=directory,
                                stdout=log, stderr=subprocess.STDOUT, timeout=45)
    text = (directory / (label + '.log')).read_text(errors='replace')
    if expected:
        assert result.returncode != 0 and expected in text, text[-2000:]
    else:
        assert result.returncode == 0, text[-2000:]
    return [line for line in text.splitlines() if line.startswith('PASS ')]


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--exports', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--baseline', type=Path)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    rows = vectors()
    cases = []
    for backend in ['native', 'portable']:
        source = (args.exports / f'command-{backend}.v').resolve()
        identity = sha(source)
        clamp = re.search(r' function\[15:0\]view_clamp_uv;.*?endfunction',
                          source.read_text(), re.S)[0]
        common = COMMON.replace('@CLAMP@', clamp).replace(
            '@GSR@', "GSR GSR(.GSRI(1'b1));" if backend == 'native' else '')
        for monitors in [True, False]:
            directory = output / f'{backend}-{int(monitors)}'
            directory.mkdir()
            (directory / 'stim.hex').write_text(''.join(f'{a:016x}\n' for a, _ in rows))
            (directory / 'gold.hex').write_text(''.join(f'{b:019x}\n' for _, b in rows))
            body = LEGAL.replace('@COMMON@', common).replace('@LAST@', str(len(rows) - 1))
            (directory / 'legal.v').write_text(body.replace('@COUNT@', str(len(rows))))
            (directory / 'negative.v').write_text(NEGATIVE.replace('@COMMON@', common))
            command = [os.environ['IVERILOG_EXE'], '-g2012', '-s', 'tb']
            if not monitors:
                command += ['-DSYNTHESIS']
            files = [source]
            if backend == 'native':
                files += [Path(os.environ['GOWIN_HOME']) / 'IDE/simlib/gw2a/prim_sim.v']
            run(command + ['-o', 'legal.vvp'] + files + ['legal.v'], directory, 'compile-legal')
            summary = run([os.environ['VVP_EXE'], 'legal.vvp'], directory, 'legal')
            trace = sha(directory / 'trace.txt')
            if args.baseline:
                assert trace == sha(args.baseline / directory.name / 'trace.txt'), 'baseline cycle trace'
            run(command + ['-o', 'negative.vvp'] + files + ['negative.v'], directory, 'compile-negative')
            for index, error in enumerate(['GPU contract: fault', 'GPU contract: bad',
                                          'GPU contract: bad', 'GPU contract: bad',
                                          'GPU contract: preflight_bad',
                                          'coordinate V input changed'], 1):
                run([os.environ['VVP_EXE'], 'negative.vvp', f'+CASE={index}'], directory,
                    f'negative-{index}', error if monitors else None)
            assert sha(source) == identity
            cases.append(dict(backend=backend, monitors=monitors, export_sha256=identity,
                              vectors=len(rows), summary=summary, trace_sha256=trace,
                              negatives=6, inputs={p.name: sha(p) for p in directory.iterdir()
                                                   if p.suffix in ('.hex', '.v')}))
    receipt = dict(status='pass', test_sha256=sha(Path(__file__)), cases=cases,
                   boundary='Actual selected command coordinate/DirectLOD/six-row queue only; '
                            'integer gold, CE/commit/backpressure and injected monitor contracts; '
                            'no raster, SDRAM, whole-frame, timing or board claim.')
    (output / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(receipt, indent=2))


if __name__ == '__main__':
    main()
