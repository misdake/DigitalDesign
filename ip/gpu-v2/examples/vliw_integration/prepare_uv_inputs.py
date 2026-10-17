"""Freeze two actual varying-UV meshlets and their selected VLIW integer outputs.

The downstream sampler must consume the final per-quad LOD code. No helper-UV
gradient or historical near threshold is evaluated by this integration fixture.
"""
from pathlib import Path
from collections import Counter
import argparse
import hashlib
import importlib.util
import json
import shutil
import subprocess
import sys
import time

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[4]
SHA = lambda p: hashlib.sha256(Path(p).read_bytes()).hexdigest()


def main():
    ap = argparse.ArgumentParser(__doc__)
    ap.add_argument('--reference', type=Path, default=Path(__file__).resolve().parent / 'reference')
    ap.add_argument('--out', type=Path, default=ROOT / 'target/gpu-vliw-integration-20261018/uv-inputs')
    ap.add_argument('--max-quads', type=int, default=20000)
    ap.add_argument('--timeout-seconds', type=int, default=240)
    a = ap.parse_args()
    current = a.reference.resolve()
    out = a.out.resolve()
    assert not out.exists(), 'Use a fresh output directory'
    start = time.monotonic()
    origin = json.loads((current / 'origin.json').read_text())
    head = origin['origin_commit']
    assert head.startswith('72488fd'), ('Unexpected VLIW revision', head)
    for path, digest in origin.get('adopted_source_sha256', origin['source_sha256']).items():
        assert SHA(current / path) == digest, path
    relative = list(origin['source_sha256']) + ['origin.json']
    snapshot = out / 'source/reference'
    for rel in relative:
        dst = snapshot / rel
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(current / rel, dst)
    spec = importlib.util.spec_from_file_location('vliw_integer_oracle', snapshot / 'scene/oracle.py')
    oracle = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(oracle)
    fixture = json.loads((snapshot / 'uv-inputs.json').read_text())
    scene = next(s for s in fixture['scenes'] if s['name'] == 'front')
    wanted = ('bunny_patch_1', 'box_x_minus')
    summary = []
    for name in wanted:
        instance = next(i for i in scene['meshlets'] if i['name'] == name)
        assert instance['color_mode'] == 0
        dst = out / name
        meta, packets = oracle.prepare(instance, dst)
        (dst / 'input.json').write_text(json.dumps(dict(camera=scene['camera'], instance=instance), indent=2))
        stats = Counter()
        lods, uv_codes = set(), set()
        packet_meta = []
        with (dst / 'quads.jsonl').open('w') as f:
            for packet_id, packet in enumerate(packets):
                packet_meta.append(dict(packet_id=packet_id, triangle_id=packet['tid'],
                    descriptor=packet['descriptor'], raw19=list(packet['words']),
                    edges=packet['edges'], edge_exponents=packet['triangle'].exps,
                    delta=packet['triangle'].delta, tri_shift=packet['triangle'].tri_shift))
                for qi in sorted(packet['masks']):
                    assert stats['quads'] < a.max_quads
                    assert time.monotonic() - start < a.timeout_seconds, 'Bounded oracle timeout'
                    e = oracle.expected_quad(packet, qi)
                    words = e['words']
                    headword = words[0]
                    mask = headword >> 15 & 15
                    lod = headword >> 19 & 255
                    assert mask == e['passed'] and headword >> 27 & 1 == 0
                    lanes = [None] * 4
                    for ordinal, lane in enumerate((0, 1, 3, 2)):
                        w0, w1, w2 = words[1 + ordinal*3:4 + ordinal*3]
                        n = [oracle.signed(w0, 12), oracle.signed(w0 >> 12, 12), oracle.signed(w1, 12)]
                        uv = [oracle.signed(w1 >> 12, 18), oracle.signed(w2, 18)]
                        if packet['masks'][qi] >> lane & 1:
                            assert all(0 <= v <= 65535 for v in uv)
                            uv_codes.add(tuple(uv))
                        lanes[lane] = dict(natural_lane=lane, covered=bool(packet['masks'][qi] >> lane & 1),
                            passed=bool(mask >> lane & 1), normal_s12f10=n, uv_s18f16=uv,
                            depth_u16q8_8=w2 >> 18 & 65535)
                    row = dict(packet_id=packet_id, triangle_id=packet['tid'], quad_index=qi,
                        qx=headword & 255, qy=headword >> 8 & 127,
                        geometry_mask=packet['masks'][qi], numerical_mask=mask,
                        lod_u5f3=lod, lanes=lanes, raw111=f'{e["raw"]:028x}',
                        transport_words36=[f'{v:09x}' for v in words],
                        transport_compare_mask36=[f'{v:09x}' for v in e['compare']])
                    f.write(json.dumps(row, separators=(',', ':')) + '\n')
                    lods.add(lod)
                    stats.update(quads=1, covered_lanes=packet['masks'][qi].bit_count(),
                                 passed_lanes=mask.bit_count(), numerical_empty_quads=int(mask == 0))
                    stats.update(e['classes'])
        assert len(uv_codes) > 1 and len(lods) > 0
        (dst / 'packets.json').write_text(json.dumps(packet_meta, indent=2))
        summary.append(dict(name=name, metadata=meta, counts=dict(stats),
                            lod_codes=sorted(lods), distinct_uv_pairs=len(uv_codes),
                            output_sha256={p.name: SHA(p) for p in sorted(dst.iterdir()) if p.is_file()}))
    receipt = dict(status='pass', vliw_head=head,
        selected_hardware_source_sha256=origin['selected_hardware_source_sha256'],
        oracle_source_sha256={str(p.relative_to(out)): SHA(p) for p in snapshot.rglob('*') if p.is_file()},
        generator_sha256=SHA(__file__), cases=summary,
        limits=dict(max_quads_per_instance=a.max_quads, timeout_seconds=a.timeout_seconds),
        contract=dict(screen=[400,240], normal='S12F10', uv='S18F16 container, actual U16/V16 inputs',
            depth='U16 Q8.8, positive D; zero quantized Z allowed', lod='selected final U5F3 per quad',
            geometry='h30, per-edge exponent and common-E D32', reciprocal='R18, F16 clamped weights',
            transport_lane_order=[0,1,3,2], exported_lane_order=[0,1,2,3],
            invalid_lanes='Compare transport fields only under the exported per-word masks; rejected Z is unspecified',
            stale_source_metadata='Historical near/cache fields in the imported scene JSON are not consumed',
            texture='Procedural varying UV on a real bunny mesh; not original bunny texture coordinates'),
        boundary='Integer input/oracle preparation only; not a new-backend RTL test or whole GPU/board proof')
    (out / 'receipt.json').write_text(json.dumps(receipt, indent=2))
    print(json.dumps(dict(status='pass', cases=[dict(name=r['name'], counts=r['counts'], lod_codes=r['lod_codes']) for r in summary]), indent=2))


if __name__ == '__main__':
    main()
