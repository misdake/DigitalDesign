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
import subprocess
import sys
import time

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
sys.path.insert(0,str(ROOT/'hardware/vendor/gowin/tests'))
from stream_contract import stage_peer


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def copy_peer_sources(peer, out):
    """The selected export and generated bench own their reserved file names."""
    return stage_peer(peer,out)


def verify_dut_identity(source, staged, expected):
    for label, path in (('selected export', source), ('staged DUT', staged)):
        actual = sha(path)
        if actual != expected:
            raise ValueError(f'{label} hash mismatch before tool execution: '
                             f'{path}; expected {expected}, got {actual}')


def execute_logged(command, cwd, log, timeout):
    """Keep complete tool output on disk and return only replay counters."""
    started = time.monotonic()
    with log.open('wb') as output:
        result = subprocess.run(list(map(str, command)), cwd=cwd, stdout=output,
                                stderr=subprocess.STDOUT, timeout=timeout)
    seconds = time.monotonic()-started
    if result.returncode:
        with log.open('rb') as output:
            output.seek(max(0, log.stat().st_size-4096))
            tail = output.read().decode(errors='replace')
        raise RuntimeError(f'Tool exited {result.returncode}; full log: {log}\n{tail}')
    with log.open(errors='replace') as output:
        summary = ''.join(line for line in output
                          if line.startswith(('PASS ', 'ACTIVITY ', 'WALL ')))
    return summary, seconds


def run_checked_simulation(execute, command, vvp, out, source, export_sha, timeout):
    verify_dut_identity(source, out/'dut.v', export_sha)
    execute(command, out, out/'compile.log', 180)
    verify_dut_identity(source, out/'dut.v', export_sha)
    return execute([vvp, '-i', out/'sim.vvp'], out, out/'run.log', timeout)


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--export', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--gold', type=Path, required=True)
    parser.add_argument('--peer', type=Path, required=True)
    parser.add_argument('--scene', choices=('box', 'bunny', 'quick', 'solid-room', 'materials', 'recorded'), required=True)
    parser.add_argument('--recorded', type=Path, help='Recorded instances and shading; no camera or asset rewriting')
    parser.add_argument('--camera', choices=('front', 'side', 'oblique'), default='front')
    parser.add_argument('--ply', type=Path)
    parser.add_argument('--meshlet-limit', type=int, help='Bound the standalone bunny scene to its first N complete meshlets')
    parser.add_argument('--native', action='store_true')
    parser.add_argument('--max-cycles', type=int, default=1200000)
    parser.add_argument('--timeout', type=int, default=600)
    args = parser.parse_args()
    if args.meshlet_limit is not None:
        assert args.scene == 'bunny' and args.meshlet_limit > 0
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
    shading = None
    solid = None
    texture = None
    if args.scene == 'recorded':
        assert args.recorded
        recording = json.loads(args.recorded.read_text())
        instances = recording['instances']
        info = dict(recording.get('info', {}), recorded_sha256=sha(args.recorded))
        shading = recording['shading']
        solid = load('solid_room_scene', HERE/'solid_scene.py')
        sys.path.insert(0, str(HERE))
        materials = load('mixed_material_scene', HERE/'material_scene.py')
        texture = materials.texture_levels()
    elif args.scene in ('solid-room', 'materials'):
        assert args.ply and sha(args.ply) == fixtures['source']['member_sha256']
        solid = load('solid_room_scene', HERE/'solid_scene.py')
        support.append(HERE/'solid_scene.py')
        checker_hashes[str((HERE/'solid_scene.py').relative_to(ROOT))] = sha(HERE/'solid_scene.py')
        instances, info, shading = solid.instances(B, args.ply, O, args.camera)
        if args.scene == 'materials':
            sys.path.insert(0, str(HERE))
            materials = load('mixed_material_scene', HERE/'material_scene.py')
            instances, info, shading = materials.instances(B, args.ply, O, args.camera)
            texture = materials.texture_levels()
            for path in (HERE/'material_scene.py', HERE/'assets/room-tiles.png'):
                support.append(path)
                checker_hashes[str(path.relative_to(ROOT))] = sha(path)
    elif args.scene == 'box':
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
        selected_batches = batches[:2] if args.scene == 'quick' else batches[:args.meshlet_limit]
        if args.meshlet_limit is not None:
            assert args.meshlet_limit <= len(batches)
        for i, batch in enumerate(selected_batches):
            instance = dict(name=f'bunny_full_{i}', group='bunny', slot=i%2,
                uniform=uniform, config=info['config'], vertices=batch['vertices'],
                triangles=batch['triangles'], color_mode=0, cull_negative=1,
                seq_base=0, generation=i%16,
                cfg_tex_w_log2=6, cfg_tex_h_log2=6, cfg_max_mip=6)
            instances.append(instance)
        if args.scene == 'bunny' and args.meshlet_limit is not None:
            info.update(source_triangles=info['triangles'], source_vertices=info['vertices'],
                        source_meshlets=info['meshlets'], source_used_vertices=info.pop('used_vertices'),
                        source_submitted_vertices=info['submitted_vertices'],
                        triangles=sum(len(b['triangles']) for b in selected_batches),
                        vertices=sum(len(b['vertices']) for b in selected_batches),
                        submitted_vertices=sum(len(b['vertices']) for b in selected_batches),
                        meshlets=len(selected_batches),
                        note='Standalone prefix of complete bunny meshlets; no box or inherited cache contents.')
        if args.scene == 'quick':
            face = copy.deepcopy(next(v for v in fixtures['scenes'][0]['meshlets']
                                      if v['name'] == 'box_x_minus'))
            face.update(group='box', slot=0, cfg_tex_w_log2=6, cfg_tex_h_log2=6, cfg_max_mip=6)
            instances.insert(0, face)
            for task, instance in enumerate(instances):
                instance['slot'] = task % 2
            bunny_triangles = sum(len(b['triangles']) for b in selected_batches)
            source_vertices, source_meshlets = info['vertices'], info['meshlets']
            source_used_vertices = info.pop('used_vertices')
            source_submitted_vertices = info['submitted_vertices']
            info.update(scene='single_box_face_two_bunny_meshlets', source_triangles=948,
                        source_vertices=source_vertices, source_meshlets=source_meshlets,
                        source_used_vertices=source_used_vertices,
                        source_submitted_vertices=source_submitted_vertices,
                        vertices=sum(len(v['vertices']) for v in instances), meshlets=len(instances),
                        submitted_vertices=sum(len(v['vertices']) for v in instances),
                        note='Subset: existing single box face and first two complete bunny meshlets.',
                        bunny_triangles=bunny_triangles, box_triangles=len(face['triangles']),
                        triangles=bunny_triangles+len(face['triangles']))
    packets, metas = [], []
    for i, instance in enumerate(instances):
        meta, ps = O.prepare(instance, out/f'task{i}')
        for packet in ps:
            packet['unlit'] = instance.get('unlit', False)
        metas.append(meta)
        packets.extend(ps)
    classes = Counter()
    for meta in metas:
        classes.update(meta['classes'])
    info.update(classes=dict(classes), packets=len(packets))
    info['memory'] = B.dma_fixture(out, V, metas, initial=V.initial_memory(texture))
    (out/'input.json').write_text(json.dumps(dict(info=info, instances=instances), indent=2))
    shutil.copyfile(source, out/'dut.v')
    peer_sources = copy_peer_sources(args.peer.resolve(), out)
    bench = B.bench(S, V, metas, args.max_cycles)
    if args.scene in ('materials', 'recorded'):
        bench = materials.configure_bench(bench, shading, instances)
    elif shading is not None:
        bench = solid.configure_bench(bench, shading)
    (out/'tb.v').write_text(bench)
    command = [os.environ['IVERILOG_EXE'], '-g2012', '-D__ICARUS__',
               '-I', out, '-s', 'tb', '-o', out/'sim.vvp']
    if args.native:
        command.append(Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v')
    else:
        command.append('-DSTREAM_STATIC_INPUT_DELAY')
    command.extend([out/'dut.v', *peer_sources, out/'tb.v'])
    log, seconds = run_checked_simulation(
        execute_logged, command, os.environ['VVP_EXE'], out, source, export_sha, args.timeout)
    assert 'PASS BUNNY' in log
    counts = V.audit(out, packets, O, args.gold.resolve(), shading=shading, texture=texture)
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
    if solid is not None:
        assert (counts['sample'] > 0) == (texture is not None) and counts['specular_nonzero'] > 0
        receipt.update(shading=shading, **solid.render(out, ROOT))
    (out/'receipt.json').write_text(json.dumps(receipt, indent=2))
    print(json.dumps(dict(status=receipt['status'], scene=args.scene,
        timing={k: timing[k] for k in ('total', 'preload', 'frontend', 'tail')},
        quads=counts['quads'], covered=activity['covered'],
        memory_sha256=receipt['memory_sha256'], simulation_seconds=seconds,
        receipt=str(out/'receipt.json')), indent=2), flush=True)


if __name__ == '__main__':
    main()
