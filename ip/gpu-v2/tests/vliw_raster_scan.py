"""Bounded raster traversal equivalence against exact quantized edge equations."""
from pathlib import Path
import argparse
import hashlib
import json
import os
import re
import subprocess

from vliw_prepare_chain import packets

ROOT = Path(__file__).resolve().parents[3]
HERE = Path(__file__).resolve().parent


def signed(value, width):
    value &= (1 << width)-1
    return value-(1 << width) if value >> (width-1) else value


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--source', type=Path, default=ROOT/'ip/gpu-v2/src/frontend/vliw')
    parser.add_argument('--mode', choices=('portable', 'native', 'both'), default='both')
    parser.add_argument('--limit', type=int, default=24)
    parser.add_argument('--small', action='store_true', help='Exclude full-screen shapes for fast vendor-primitive replay')
    parser.add_argument('--origin-cases', action='store_true', help='Four translated triangles with both root coordinates nonzero')
    parser.add_argument('--locality-cases', action='store_true', help='Large full subtrees next to partial/rejected siblings')
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    descs, words = packets()
    if args.small:
        selected = [i for i in range(len(descs)) if i % 6 not in (0, 3)]
        descs, words = [descs[i] for i in selected], [v for i in selected for v in words[16*i:16*i+16]]
    translated = [((33, 65), (48, 65), (33, 80)),
                  ((195, 145), (216, 147), (203, 166)),
                  ((339, 207), (365, 210), (349, 232)),
                  ((130, 35), (147, 37), (133, 60))]
    assert not (args.origin_cases and args.locality_cases)
    if args.locality_cases:
        translated = [((32, 32), (160, 32), (32, 160)),
                      ((180, 32), (315, 50), (220, 160)),
                      ((17, 17), (82, 17), (17, 78)),
                      ((33, 97), (159, 101), (70, 221))]
    direct_edges = args.origin_cases or args.locality_cases
    if direct_edges:
        descs = [0]*len(translated)
    assert 1 <= args.limit <= len(descs)
    descs, words = descs[:args.limit], words[:16*args.limit]
    coefficients, masks, bounds, counts, origins = [], [], [], [], []
    for p, descriptor in enumerate(descs):
        edges = []
        for i in range(3):
            if direct_edges:
                x0, y0 = translated[p][i]
                x1, y1 = translated[p][(i+1) % 3]
                scale = (256 if args.locality_cases else 4000) if p & 1 else 1
                edges.append((2*(y0-y1)*scale, 2*(x1-x0)*scale, 2*(x0*y1-y0*x1)*scale))
            else:
                n = [signed(words[16*p+3*i+j], 47 if j == 2 else 36) for j in range(3)]
                shift = max(0, *(abs(v).bit_length()-(27 if j == 2 else 17) for j, v in enumerate(n)))
                sign = -1 if descriptor & (1 << 22) else 1
                nx, ny, c = [sign*(-1 if v < 0 else 1)*(abs(v) >> shift) for v in n]
                edges.append((2*nx, 2*ny, c))
        coefficients.extend(v & 0xffffffff for edge in edges for v in edge)
        coverage = [0]*24000
        for y in range(240):
            lo, hi = 0, 399
            for dx, dy, c in edges:
                tau = int(not (dx > 0 or dx == 0 and dy > 0))
                need = tau-c-dy*y
                if dx > 0:
                    lo = max(lo, -((-need)//dx))
                elif dx < 0:
                    hi = min(hi, need//dx)
                elif need > 0:
                    lo, hi = 1, 0
                if lo > hi:
                    break
            for x in range(lo, hi+1):
                coverage[y//2*200+x//2] |= 1 << (2*(y & 1)+(x & 1))
        live = [i for i, value in enumerate(coverage) if value]
        masks.extend(coverage)
        counts.append(len(live))
        if live:
            xs, ys = [i % 200//8 for i in live], [i//200//8 for i in live]
            bounds.append(min(xs) | max(xs) << 5 | min(ys) << 10 | max(ys) << 14)
            root = max(4, ((min(xs) ^ max(xs)) | (min(ys) ^ max(ys))).bit_length()+4)
            keep = (31 << (root-4)) & 31
            origins.append(((min(xs) & keep) << 4) | ((min(ys) & keep) << 13))
        else:
            bounds.append(1 << 18)
            origins.append(0)
    for name, values, width in [('coefficients.hex', coefficients, 8), ('masks.hex', masks, 1),
                                 ('bounds.hex', bounds, 5), ('counts.hex', counts, 5), ('origins.hex', origins, 5)]:
        (out/name).write_text(''.join(f'{v:0{width}x}\n' for v in values))
    source = args.source.resolve()
    rtl = '\n'.join((source/name).read_text() for name in ('raster.v', 'raster_program.v'))
    rtl += '\n'+(ROOT/'ip/gpu-v2/src/frontend/vliw/raster_storage.v').read_text()
    (out/'dut.v').write_text(rtl)
    tb = (HERE/'vliw_raster_scan_tb.v').read_text().replace('@N@', str(len(descs)))
    (out/'tb.v').write_text(tb)
    reports = []
    for mode in (('portable', 'native') if args.mode == 'both' else (args.mode,)):
        command = [os.environ['IVERILOG_EXE'], '-g2012', '-s', 'tb', '-o', out/(mode+'.vvp')]
        if 'prefetch_handoff' in rtl:
            command += ['-DRASTER_PREFETCH_COVERAGE']
        if 'scan_classify_pending' in rtl:
            command += ['-DRASTER_NEXT_PREDICATE']
        if 'packed_step_q' in rtl:
            command += ['-DRASTER_ORIGIN_DSP']
            if args.origin_cases:
                command += ['-DRASTER_ORIGIN_TRANSLATED']
        if 'terminal_tile' in rtl:
            command += ['-DRASTER_LOCALITY_LIMIT']
        if 'wire reject_tail=' in rtl:
            command += ['-DRASTER_REJECT_TAIL']
        command += ['-DPORTABLE'] if mode == 'portable' else [Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v']
        command += [out/'dut.v', out/'tb.v']
        for label, cmd in [('compile', command), ('run', [os.environ['VVP_EXE'], out/(mode+'.vvp')])]:
            result = subprocess.run(list(map(str, cmd)), cwd=out, capture_output=True, timeout=120)
            (out/(mode+'-'+label+'.log')).write_bytes(result.stdout+result.stderr)
            assert result.returncode == 0, (result.stdout+result.stderr).decode(errors='replace')[-3000:]
        log = result.stdout.decode()
        assert 'PASS SCAN' in log
        fields = {k: int(v) for k, v in re.findall(r'(\w+)=(\d+)', log)}
        reports.append(dict(mode=mode, **fields))
        print(mode, log.strip())
    if len(reports) == 2:
        assert {k: v for k, v in reports[0].items() if k != 'mode'} == {k: v for k, v in reports[1].items() if k != 'mode'}
    receipt = dict(status='pass', source_sha256=hashlib.sha256(rtl.encode()).hexdigest(), runs=reports,
                   scope='All pixel h and nonempty quad masks, CE pauses, quad backpressure, full/partial tiles and row ends; no full-GPU performance claim')
    (out/'receipt.json').write_text(json.dumps(receipt, indent=2))


if __name__ == '__main__':
    main()
