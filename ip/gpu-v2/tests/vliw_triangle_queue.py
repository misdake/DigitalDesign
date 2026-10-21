"""Bounded T queue fill, wrap, reset and stalled transport against integer gold."""
from pathlib import Path
import argparse
import copy
import hashlib
import importlib.util
import json
import os
import re
import subprocess

ROOT = Path(__file__).resolve().parents[3]
HERE = ROOT / 'ip/gpu-v2/examples/vliw_integration'


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--export', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--slots', type=int, choices=(2, 4, 8, 16, 32), required=True)
    parser.add_argument('--native', action='store_true')
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    oracle = load('queue_oracle', HERE / 'reference/scene/oracle.py')
    oracle = load('queue_transport', HERE / 'transport.py').adapt_oracle(oracle)
    fixture = json.loads((HERE / 'reference/uv-inputs.json').read_text())
    instance = copy.deepcopy(next(m for m in fixture['scenes'][0]['meshlets']
                                  if m['name'] == 'bunny_patch_1'))
    instance['cull_negative'] = 0
    meta, _ = oracle.prepare(instance, out)
    assert meta['packets'] >= 2 * args.slots
    template = Path(__file__).with_name('vliw_triangle_queue_tb.v')
    bench = template.read_text()
    fields = dict(DATA=out.as_posix(), NV=meta['vertices'], NT=meta['triangles'],
                  NP=meta['packets'], DEPTH=args.slots, SLOT=meta['slot'])
    for key, value in fields.items():
        bench = bench.replace('@' + key + '@', str(value))
    assert not re.search('@[A-Z]+@', bench)
    (out / 'tb.v').write_text(bench)
    source = args.export.resolve()
    before = sha(source)
    command = [os.environ.get('IVERILOG_EXE', 'iverilog'), '-g2012', '-s', 'tb',
               '-o', str(out / 'sim.vvp')]
    if args.native:
        command.append(str(Path(os.environ['GOWIN_HOME']) / 'IDE/simlib/gw2a/prim_sim.v'))
    command += [str(source), str(out / 'tb.v')]
    for label, cmd, timeout in [('compile', command, 60),
                               ('run', [os.environ.get('VVP_EXE', 'vvp'), str(out / 'sim.vvp')], 120)]:
        with (out / (label + '.log')).open('wb') as log:
            result = subprocess.run(cmd, cwd=out, stdout=log, stderr=subprocess.STDOUT,
                                    timeout=timeout)
        assert result.returncode == 0, (out / (label + '.log')).read_text(errors='replace')[-3000:]
    assert before == sha(source)
    match = re.search(r'PASS T_QUEUE (.*)', (out / 'run.log').read_text())
    assert match
    counters = dict((key, int(value)) for key, value in re.findall(r'(\w+)=(\d+)', match[1]))
    receipt = dict(status='pass', native=args.native, source_sha256=before,
                   bench_sha256=sha(template), counters=counters)
    (out / 'receipt.json').write_text(json.dumps(receipt, indent=2))
    print(json.dumps(receipt))


if __name__ == '__main__':
    main()
