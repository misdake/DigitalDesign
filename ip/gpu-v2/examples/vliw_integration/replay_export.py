"""Audit a pre-exported GPU against box or all 948 bunny faces with the real MC.

Build gpu-v2-vliw-export, vliw_integration and the vliw_sdram MC peer first.
No build, source modification or implicit selection of a historical export occurs.
"""
from pathlib import Path
from collections import Counter
import argparse
import copy
import hashlib
import importlib.util
import json
import os
import re
import shutil
import sys

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--export', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--gold', type=Path, required=True)
    parser.add_argument('--peer', type=Path, required=True)
    parser.add_argument('--scene', choices=('box', 'bunny'), required=True)
    parser.add_argument('--ply', type=Path)
    parser.add_argument('--native', action='store_true')
    parser.add_argument('--max-cycles', type=int, default=1200000)
    parser.add_argument('--timeout', type=int, default=600)
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    source = args.export.resolve()
    export_sha = sha(source)
    gold_sha = sha(args.gold.resolve())
    support = [Path(__file__), HERE/'full_bunny.py', HERE/'transport.py',
               HERE/'verify_stage2.py', HERE/'stage2_tb.v',
               ROOT/'ip/gpu-v2/tests/vliw_sdram.py']
    checker_hashes = {str(p.relative_to(ROOT)): sha(p) for p in support}
    B = load('export_replay_bunny', HERE/'full_bunny.py')
    S = load('export_replay_sdram', ROOT/'ip/gpu-v2/tests/vliw_sdram.py')
    V = load('export_replay_audit', HERE/'verify_stage2.py')
    O = load('export_replay_oracle', HERE/'reference/scene/oracle.py')
    O = load('export_replay_transport', HERE/'transport.py').adapt_oracle(O)
    fixtures = json.loads((HERE/'reference/uv-inputs.json').read_text())
    if args.scene == 'box':
        instances = [copy.deepcopy(next(v for v in fixtures['scenes'][0]['meshlets']
                                       if v['name'] == 'box_x_minus'))]
        instances[0].update(slot=0, cfg_tex_w_log2=6, cfg_tex_h_log2=6, cfg_max_mip=6)
        info = dict(scene='box_x_minus')
    else:
        assert args.ply and sha(args.ply) == fixtures['source']['member_sha256']
        batches, info = B.mesh(args.ply)
        assert info['triangles'] == 948
        uniform = B.camera_for('front')['uniform']
        instances = []
        for i, batch in enumerate(batches):
            instance = dict(name=f'bunny_full_{i}', group='bunny', slot=i%2,
                uniform=uniform, config=info['config'], vertices=batch['vertices'],
                triangles=batch['triangles'], color_mode=0, cull_negative=1,
                seq_base=0, generation=i%16, scale_s=0,
                cfg_tex_w_log2=6, cfg_tex_h_log2=6, cfg_max_mip=6)
            values = O.F.oracle(uniform, info['config'], batch['vertices'])
            maximum = max(abs(O.signed(values[6*j+k], 32))
                          for j in range(len(batch['vertices'])) for k in range(3))
            while maximum >> instance['scale_s'] > 131071:
                instance['scale_s'] += 1
            assert instance['scale_s'] < 16
            instances.append(instance)
    packets, metas = [], []
    for i, instance in enumerate(instances):
        meta, ps = O.prepare(instance, out/f'task{i}')
        metas.append(meta)
        packets.extend(ps)
    classes = Counter()
    for meta in metas:
        classes.update(meta['classes'])
    info.update(classes=dict(classes), packets=len(packets))
    info['memory'] = B.dma_fixture(out, V, metas)
    (out/'input.json').write_text(json.dumps(dict(info=info, instances=instances), indent=2))
    shutil.copyfile(source, out/'dut.v')
    for path in args.peer.resolve().iterdir():
        if path.suffix in ('.v', '.vh'):
            shutil.copyfile(path, out/path.name)
    bench = B.bench(S, V, metas, args.max_cycles)
    (out/'tb.v').write_text(bench)
    command = [os.environ['IVERILOG_EXE'], '-g2012', '-D__ICARUS__',
               '-I', out, '-s', 'tb', '-o', out/'sim.vvp']
    if args.native:
        command.append(Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v')
    command.extend(sorted(out.glob('*.v')))
    B.execute(command, out, out/'compile.log', 180)
    log, seconds = B.execute([os.environ['VVP_EXE'], out/'sim.vvp'],
                             out, out/'run.log', args.timeout)
    assert 'PASS BUNNY' in log
    counts = V.audit(out, packets, O, args.gold.resolve())
    def parse(prefix):
        return {k: int(v) for k, v in re.findall(r'(\w+)=(\d+)',
                next(line for line in log.splitlines() if line.startswith(prefix)))}
    timing, activity = parse('PASS BUNNY'), parse('ACTIVITY')
    assert activity['covered'] == sum(meta['coverage_pixels'] for meta in metas)
    assert activity['pixels'] == 4*counts['quads']
    wall = list(map(int, next(line for line in log.splitlines()
                             if line.startswith('WALL ')).split()[1:]))
    assert sum(wall) == timing['total']
    assert sha(source) == export_sha == sha(out/'dut.v')
    assert sha(args.gold.resolve()) == gold_sha
    assert checker_hashes == {str(p.relative_to(ROOT)): sha(p) for p in support}
    receipt = dict(status='pass', scene=args.scene, native=args.native,
        info=info, counts=counts, timing=timing, activity=activity, wall=wall,
        simulation_seconds=seconds, dut_sha256=export_sha,
        source_sha256={p.name: sha(p) for p in out.iterdir() if p.suffix in ('.v', '.vh')},
        checker_sha256=checker_hashes, gold_sha256=gold_sha,
        fixture_sha256={name: sha(out/name) for name in
                        ('input.json', 'seed.txt', 'jobs.hex', 'expected-dma.hex', 'expected-valid.hex')},
        front_sha256=sha(out/'front.txt'), back_sha256=sha(out/'back.txt'),
        memory_sha256=sha(out/'memory.mem'),
        boundary='Complete GPU plus current real arbiter/bridge/controller and pin model; '
                 'first load and final flush included; no CPU/display contention or board proof.')
    (out/'receipt.json').write_text(json.dumps(receipt, indent=2))
    print(json.dumps({k: receipt[k] for k in ('scene', 'timing', 'activity',
                                            'memory_sha256', 'simulation_seconds')}, indent=2), flush=True)


if __name__ == '__main__':
    main()
