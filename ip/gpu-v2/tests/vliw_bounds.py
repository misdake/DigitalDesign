"""Bounded preparation replay against integer tile-interval inequalities."""
from pathlib import Path
import argparse
import hashlib
import importlib.util
import json
import os
import random
import subprocess

ROOT = Path(__file__).resolve().parents[3]


def signed(value, width):
    return (value & ((1 << (width-1))-1)) - (value & (1 << (width-1)))


def expected_bounds(descriptor, words):
    """Enumerate tile coordinates, independently of the RTL binary search."""
    sigma = bool(descriptor & (1 << 22))
    coefficients, seeds = [], []
    for edge in range(3):
        raw = [signed(words[edge*3+k], 47 if k == 2 else 36) for k in range(3)]
        shift = max(0, *(abs(v).bit_length()-(27 if k == 2 else 17) for k, v in enumerate(raw)))
        coefficients.append([(-1 if (v < 0) ^ sigma else 1)*(abs(v) >> shift) for v in raw])
        seeds.append([(v < 0) ^ sigma for v in raw])
    ranges = [list(range(25)), list(range(15))]
    for i in range(3):
        j = (i+1) % 3
        for axis in range(2):
            orth = 1-axis
            p, q = coefficients[i][orth], coefficients[j][orth]
            if (p == q == 0) or (p*q > 0):
                continue
            d = abs(q)*coefficients[i][axis] + abs(p)*coefficients[j][axis]
            c = abs(q)*coefficients[i][2] + abs(p)*coefficients[j][2]
            c += 131071 if seeds[i][axis] else 0
            ranges[axis] = [tile for tile in ranges[axis]
                            if c+32*d*tile+(30*d if d > 0 else 0) >= 0]
    if not all(ranges):
        return 1 << 18
    x, y = ranges
    return (min(x) << 13) | (max(x) << 8) | (min(y) << 4) | max(y)


def fixtures(count):
    spec = importlib.util.spec_from_file_location('prepare_fixture', Path(__file__).with_name('vliw_prepare_chain.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    descriptors, words = module.packets()
    rng = random.Random(0xB0A1D5)
    for sample in range(count):
        while True:
            p = [tuple(rng.randint(-131071, 131071) for _ in range(3)) for _ in range(3)]
            if sample % 3:
                p = [(x, y, max(1, abs(z))) for x, y, z in p]
            def cross(a, b):
                return (a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0])
            n = [cross(p[1], p[2]), cross(p[2], p[0]), cross(p[0], p[1])]
            det = sum(a*b for a, b in zip(p[0], n[0]))
            if det:
                break
        rgb = bool(sample & 1)
        descriptors.append((int(det < 0) << 22) | (int(rgb) << 23))
        for nx, ny, nz in n:
            words += [nx & ((1 << 36)-1), ny & ((1 << 36)-1),
                      (-399*nx-239*ny+512*nz) & ((1 << 47)-1)]
        words.append(abs(det))
        for vertex in range(3):
            words += [rng.getrandbits(36), rng.getrandbits(24 if rgb else 32)]
    params = []
    for packet, descriptor in enumerate(descriptors):
        values = []
        for vertex in range(3):
            normal, attr = words[16*packet+10+2*vertex:16*packet+12+2*vertex]
            n = [signed(normal >> (12*k), 12) for k in range(3)]
            a = [(attr >> (8*k)) & 255 for k in range(3)] if descriptor & (1 << 23) else [attr & 65535, attr >> 16, 0]
            values.append(n+a)
        for component in range(6):
            a, b, c = (v[component] for v in values)
            params.append(((c & 131071) << 34) | (((b-c) & 131071) << 17) | ((a-c) & 131071))
    return descriptors, words, params


def main():
    ap = argparse.ArgumentParser(__doc__)
    ap.add_argument('--export', type=Path, required=True)
    ap.add_argument('--output', type=Path, required=True)
    ap.add_argument('--count', type=int, default=128)
    ap.add_argument('--native', action='store_true')
    args = ap.parse_args()
    assert 0 <= args.count <= 512
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    descriptors, words, params = fixtures(args.count)
    bounds = [expected_bounds(d, words[16*i:16*i+16]) for i, d in enumerate(descriptors)]
    for name, values in [('descriptors', descriptors), ('words', words), ('bounds', bounds), ('params', params)]:
        (out/f'{name}.hex').write_text(''.join(f'{v:x}\n' for v in values))
    tb = r'''
`timescale 1ns/1ps
module tb;
`ifndef PORTABLE
GSR GSR(.GSRI(1'b1));
`endif
reg clk=0;always #5 clk=~clk;
reg reset=1,ce=0,start=0,raw_valid=0;reg[35:0]descriptor;reg[4:0]raw_index;reg[53:0]raw_data;
integer tick=0,wall=0,p=0,wi=0,taken=0,seen=0,attr_count=0,empty_count=0;
always @(negedge clk)begin tick=tick+1;ce=!reset&&(tick%11!=3&&tick%11!=4);end
wire ready,busy,done,bvalid,bempty;wire[4:0]xl,xh;wire[3:0]yl,yh;wire[16:0]error;
gpu_v2_vliw_interpolator I(.clk(clk),.reset(reset),.ce(ce),.prepare_start(start),.descriptor(descriptor),
 .raw_valid(raw_valid),.raw_ready(ready),.raw_index(raw_index),.raw_data(raw_data),
 .cfg_far_z(32'd13041664),.cfg_tex_u(5'd6),.cfg_tex_v(5'd6),.cfg_max_mip(5'd6),.cfg_bias(8'sd0),
 .table_valid(1'b0),.table_index(8'd0),.table_data(36'd0),.quad_valid(1'b0),.quad_x(8'd0),.quad_y(7'd0),
 .sample_valid(1'b0),.sample_lane(2'd0),.sample_cover(1'b0),.sample_anchor(1'b0),.sample_h(30'd0),
 .quad_end_valid(1'b0),.quad_mask(4'd0),.pool_ready(1'b1),.pool_quad_id(3'd0),.s_busy(busy),.s_done(done),.error(error),
 .bound_valid(bvalid),.bound_empty(bempty),.bound_xlo(xl),.bound_xhi(xh),.bound_ylo(yl),.bound_yhi(yh));
reg[35:0]ds[0:@N@-1];reg[53:0]ws[0:16*@N@-1];reg[18:0]bs[0:@N@-1];reg[50:0]ps[0:6*@N@-1];
always @(posedge clk)begin
 wall=wall+1;if(wall>@LIMIT@)$fatal(1,"bounded bounds replay");
 if(!reset&&ce)begin
  if(error)$fatal(1,"preparation error packet%0d code%h",p,error);
  if(start)begin seen=0;attr_count=0;end
  if(I.prepare.stream_param_valid)begin
   if({I.prepare.stream_param_index,I.prepare.stream_param_bank}!=attr_count||
      I.prepare.stream_param_data!==ps[6*p+attr_count])$fatal(1,"attribute packet%0d component%0d",p,attr_count);
   attr_count=attr_count+1;
  end
  if(bvalid)begin
   if(seen||bempty!==bs[p][18])$fatal(1,"empty mismatch packet%0d got%0d want%0d",p,bempty,bs[p][18]);
   if(!bempty&&{xl,xh,yl,yh}!==bs[p][17:0])$fatal(1,"bounds mismatch packet%0d got %0d %0d %0d %0d want%h",p,xl,xh,yl,yh,bs[p]);
   seen=1;empty_count=empty_count+bempty;
  end
 end
end
initial begin
 $readmemh("descriptors.hex",ds);$readmemh("words.hex",ws);$readmemh("bounds.hex",bs);$readmemh("params.hex",ps);
 repeat(4)@(negedge clk);reset=0;
 for(p=0;p<@N@;p=p+1)begin
  descriptor=ds[p];start=1;@(posedge clk);while(!ce)@(posedge clk);@(negedge clk);start=0;
  wi=0;raw_valid=1;raw_index=0;raw_data=ws[16*p];
  while(wi<16)begin
   @(posedge clk);taken=ready&&ce;@(negedge clk);
   if(taken)begin wi=wi+1;raw_index=wi;if(wi<16)raw_data=ws[16*p+wi];end
  end
  raw_valid=0;while(busy)@(negedge clk);
  if(!seen||(!bs[p][18]&&attr_count!=6))$fatal(1,"incomplete packet%0d",p);
 end
 $display("PASS BOUNDS packets=%0d empty=%0d wall=%0d",p,empty_count,wall);$finish;
end
initial begin #@TIME@;$fatal(1,"absolute replay timeout");end
endmodule
'''
    limit = 1200*len(descriptors)
    for key, value in dict(N=len(descriptors), LIMIT=limit, TIME=10*limit+100).items():
        tb = tb.replace('@'+key+'@', str(value))
    (out/'tb.v').write_text(tb)
    source = args.export.resolve()
    command = [os.environ['IVERILOG_EXE'], '-g2012', '-s', 'tb', '-o', str(out/'sim.vvp')]
    if args.native:
        command.append(str(Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v'))
    command += [str(source), str(out/'tb.v')]
    for name, cmd in [('compile', command), ('run', [os.environ['VVP_EXE'], str(out/'sim.vvp')])]:
        result = subprocess.run(cmd, cwd=out, capture_output=True, timeout=90)
        (out/f'{name}.log').write_bytes(result.stdout+result.stderr)
        assert result.returncode == 0, (result.stdout+result.stderr).decode(errors='replace')[-3000:]
    assert b'PASS BOUNDS' in result.stdout
    receipt = dict(status='pass', packets=len(descriptors), native=args.native,
                   export_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
                   test_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                   log=result.stdout.decode())
    (out/'receipt.json').write_text(json.dumps(receipt, indent=2))
    print(receipt['log'])


if __name__ == '__main__':
    main()
