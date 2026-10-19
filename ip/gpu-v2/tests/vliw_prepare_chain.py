"""Small bounded R-owned-origin replay; no external fixture or old origin port."""
from pathlib import Path
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[3]
HERE = Path(__file__).resolve().parent


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def packets():
    # Large root, offscreen cancellation, partial small root, and zero origin bits.
    shapes = [
        [(-1024, -1024), (2048, -1024), (-1024, 2048)],
        [(1000, 1000), (1010, 1000), (1000, 1010)],
        [(-2000, -2000), (-1980, -2000), (-2000, -1980)],
        [(0, 0), (800, 0), (0, 480)],
        [(383, 223), (399, 223), (383, 239)],
        [(0, 0), (16, 0), (0, 16)],
    ]
    descriptors, words = [], []
    def cross(a, b):
        return (a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0])
    for scale in (1, 16):
        for reverse in (False, True):
            for xy in shapes:
                p = [((2*x-399)*scale, (2*y-239)*scale, 512*scale) for x, y in xy]
                if reverse:
                    p[1], p[2] = p[2], p[1]
                assert all(-(1 << 17) <= v < (1 << 17) for xyz in p for v in xyz)
                a, b, c = p
                n = [cross(b, c), cross(c, a), cross(a, b)]
                det = sum(x*y for x, y in zip(a, n[0]))
                assert 0 < abs(det) < (1 << 53)
                packet = len(descriptors)
                rgb = bool(packet & 1)
                descriptors.append((int(det < 0) << 22) | (int(rgb) << 23))
                for nx, ny, nz in n:
                    cv = -399*nx-239*ny+512*nz
                    assert -(1 << 35) <= nx < (1 << 35)
                    assert -(1 << 35) <= ny < (1 << 35)
                    assert -(1 << 46) <= cv < (1 << 46)
                    words += [nx & ((1 << 36)-1), ny & ((1 << 36)-1), cv & ((1 << 47)-1)]
                words += [abs(det)]
                corners = (-2048, 2047, -1, 0, 1, -1024, 1023)
                for vertex in range(3):
                    normal = [corners[(packet + 2*vertex + axis) % len(corners)] for axis in range(3)]
                    packed = sum((value & 4095) << (12*axis) for axis, value in enumerate(normal))
                    if rgb:
                        attr = sum(((packet*37 + vertex*71 + channel*83) & 255) << (8*channel) for channel in range(3))
                    else:
                        attr = ((packet*12347 + vertex*32767) & 65535) | (((65535 - packet*977 - vertex*19001) & 65535) << 16)
                        if packet == 0:
                            attr = (65535 if vertex == 1 else 0) | ((0 if vertex == 1 else 65535) << 16)
                    words += [packed, attr]
    # Small integer rays exercise zero and short reduction counts, including
    # consecutive LOAD/retire edges. Source attributes keep the existing modes.
    for reverse in (False, True):
        for scale in (1, 256, 384, 512, 768, 131071):
            p = [(-scale, -scale, scale), (scale, -scale, scale), (-scale, scale, scale)]
            if reverse:
                p[1], p[2] = p[2], p[1]
            a, b, c = p
            n = [cross(b, c), cross(c, a), cross(a, b)]
            det = sum(x*y for x, y in zip(a, n[0]))
            packet = len(descriptors)
            descriptors.append((int(det < 0) << 22) | ((packet & 1) << 23))
            tail = words[(packet % 24)*16+10:(packet % 24)*16+16]
            for nx, ny, nz in n:
                cv = -399*nx-239*ny+512*nz
                words += [nx & ((1 << 36)-1), ny & ((1 << 36)-1), cv & ((1 << 47)-1)]
            words += [abs(det), *tail]
    return descriptors, words


def main():
    ap = argparse.ArgumentParser(__doc__)
    ap.add_argument('--tag', required=True)
    ap.add_argument('--export-root', type=Path, required=True)
    ap.add_argument('--mode', choices=['portable', 'native', 'both'], default='both')
    ap.add_argument('--operand-table', action='store_true')
    args = ap.parse_args()
    assert re.fullmatch(r'[A-Za-z0-9_.-]+', args.tag)
    out = ROOT/'target/gpu-vliw-prepare-chain'/args.tag
    out.mkdir(parents=True, exist_ok=False)
    snapshot = out/'source'
    snapshot.mkdir()
    files = [Path(__file__), HERE/'vliw_prepare_chain_tb.v', HERE/'vliw_prepare_chain.rs']
    if args.operand_table:
        files.append(HERE/'vliw_operand_table_tb.v')
    files += [args.export_root/f'frontend-{m}.v' for m in ('portable', 'native')]
    manifest = {}
    for p in files:
        shutil.copyfile(p, snapshot/p.name)
        manifest[p.name] = sha(snapshot/p.name)
    (out/'manifest.json').write_text(json.dumps(manifest, indent=2))
    desc, words = packets()
    geometry = []
    for packet, descriptor in enumerate(desc):
        for edge in range(3):
            values = []
            for component in range(3):
                width = 47 if component == 2 else 36
                value = words[packet*16+edge*3+component] & ((1 << width)-1)
                values.append(value-(1 << width) if value & (1 << (width-1)) else value)
            shift = max(0, *(abs(value).bit_length()-(27 if k == 2 else 17)
                             for k, value in enumerate(values)))
            for k, value in enumerate(values):
                reduced = (abs(value) >> shift) * (-1 if value < 0 else 1)
                if descriptor & (1 << 22):
                    reduced = -reduced
                geometry.append((reduced*(1 if k == 2 else 2)) & ((1 << 30)-1))
    params = []
    for packet, descriptor in enumerate(desc):
        values = []
        for vertex in range(3):
            normal, attr = words[packet*16+10+vertex*2:packet*16+12+vertex*2]
            n = [(normal >> (axis*12)) & 4095 for axis in range(3)]
            n = [value-4096 if value & 2048 else value for value in n]
            attributes = [(attr >> (channel*8)) & 255 for channel in range(3)] if descriptor & (1 << 23) else [attr & 65535, attr >> 16, 0]
            values.append(n + attributes)
        for component in range(6):
            a, b, c = (value[component] for value in values)
            params.append(((c & 262143) << 36) | (((b-c) & 262143) << 18) | ((a-c) & 262143))
    rows = []
    for mode in (['portable', 'native'] if args.mode == 'both' else [args.mode]):
        run = out/mode
        run.mkdir()
        (run/'descriptors.hex').write_text(''.join(f'{v:09x}\n' for v in desc))
        (run/'words.hex').write_text(''.join(f'{v:014x}\n' for v in words))
        (run/'geometry.hex').write_text(''.join(f'{v:08x}\n' for v in geometry))
        (run/'params.hex').write_text(''.join(f'{v:014x}\n' for v in params))
        tb = (snapshot/'vliw_prepare_chain_tb.v').read_text()
        for k, v in dict(N=len(desc), LAST=len(desc)-1, WLAST=len(words)-1).items():
            tb = tb.replace('@'+k+'@', str(v))
        (run/'tb.v').write_text(tb)
        command = [os.environ.get('IVERILOG_EXE', 'iverilog'), '-g2012', '-s', 'tb', '-o', str(run/'sim.vvp')]
        if mode == 'native':
            command += [str(Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v')]
        command += [str(snapshot/f'frontend-{mode}.v'), str(run/'tb.v')]
        for label, cmd in [('compile', command), ('run', [os.environ.get('VVP_EXE', 'vvp'), str(run/'sim.vvp')])]:
            result = subprocess.run(cmd, cwd=run, capture_output=True, timeout=90)
            (run/f'{label}.log').write_bytes(result.stdout+result.stderr)
            assert result.returncode == 0, (result.stdout+result.stderr).decode(errors='replace')[-4000:]
        log = result.stdout.decode(errors='replace')
        assert 'PASS RROOT' in log
        rows.append(dict(mode=mode, packets=len(desc), trace_sha256=sha(run/'roots.txt'), log=log))
        print(log, flush=True)
        if args.operand_table:
            unit = run/'operand-table.vvp'
            cmd = command[:]
            cmd[cmd.index('-o')+1] = str(unit)
            cmd[-1] = str(snapshot/'vliw_operand_table_tb.v')
            for label, command2 in [('table-compile', cmd), ('table-run', [os.environ.get('VVP_EXE', 'vvp'), str(unit)])]:
                result = subprocess.run(command2, cwd=run, capture_output=True, timeout=90)
                (run/f'{label}.log').write_bytes(result.stdout+result.stderr)
                assert result.returncode == 0, (result.stdout+result.stderr).decode(errors='replace')[-4000:]
            assert b'PASS operand table' in result.stdout
            rows[-1]['operand_table_log'] = result.stdout.decode(errors='replace')
            print(rows[-1]['operand_table_log'], flush=True)
    if len(rows) == 2:
        assert rows[0]['trace_sha256'] == rows[1]['trace_sha256']
    (out/'receipt.json').write_text(json.dumps(dict(status='pass', runs=rows,
        scope='Small real I-to-R origin replay: independent root equations, fused-DSP old-Q operands and fixed retirement, root9, empty-bound cancellation, CE hold and in-flight reset; no full scanning or whole-chain qualification',
        manifest_sha256=sha(out/'manifest.json')), ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
