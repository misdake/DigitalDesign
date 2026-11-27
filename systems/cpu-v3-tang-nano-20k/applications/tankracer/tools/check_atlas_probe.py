"""Resolve real meshlet sampler footprints back to their intended atlas source cells."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import zipfile


def check(snapshot, frames, atlas, source):
    with zipfile.ZipFile(atlas) as z:
        scene = json.loads(z.read('scene.json'))
    with zipfile.ZipFile(source) as z:
        original = json.loads(z.read('scene.json'))
    audit = scene['opaque_atlas']
    if hashlib.sha256(source.read_bytes()).hexdigest() != audit['source_sha256']:
        raise ValueError('Source scene identity mismatch')
    run = json.loads((frames/'uv-probe.json').read_text())
    package = (snapshot/'cpu-v3-boot.bin').read_bytes()
    fingerprint = 0xcbf29ce484222325
    for byte in package:
        fingerprint = ((fingerprint ^ byte) * 0x100000001b3) & ((1 << 64)-1)
    if run['package_bytes'] != len(package) or run['package_fnv64'] != f'{fingerprint:016x}':
        raise ValueError('Probe package differs from the supplied snapshot')
    build = json.loads((snapshot/'application.build.json').read_text())
    assets = json.loads((snapshot/'tankracer-static-assets/assets.json').read_text())
    if hashlib.sha256(atlas.read_bytes()).hexdigest() != assets['map_sha256']:
        raise ValueError('Atlas input differs from the compiled snapshot')
    mesh_bytes = next(r['bytes'] for r in assets['resources'] if r['symbol'] == 'TRACK_MESH_BYTES')
    candidates = [r for r in build['resources'] if r['kind'] == 'copy' and r['bytes'] == mesh_bytes]
    if len(candidates) != 1:
        raise ValueError('Mesh resource needs an unambiguous extent')
    mesh = (snapshot/candidates[0]['file']).read_bytes()
    groups = assets['groups']
    by_meshlet = {}
    first = 0
    for group in groups:
        material = original['materials'][group['material']]
        for i in range(group['meshlets']):
            address = struct.unpack_from('<H', mesh, (first+i)*4)[0] * 128
            by_meshlet[address] = (group['name'], material.get('texture'))
        first += group['meshlets']
    rows, samples, rgba_samples = [], 0, 0
    paths = sorted(frames.glob('frame-*.uv.json'))
    if len(paths) != run['requested_frames']:
        raise ValueError('Incomplete probe run')
    for path in paths:
        data = json.loads(path.read_text())
        for item in data['textures']:
            group, texture = by_meshlet[item['meshlet']]
            if item['rgba4444'] and texture not in audit.get('rgba_regions', []):
                continue
            if texture not in audit['regions']:
                raise ValueError('Sample has no original opaque source region')
            if item['rgba4444'] != (texture in audit.get('rgba_regions', [])):
                raise ValueError('Source region uses the wrong texture interpretation')
            region = audit['regions'][texture]
            if item['rgba4444']:
                rgba_samples += item['samples']
            else:
                samples += item['samples']
            unsafe = []
            for level in item['levels']:
                scale = audit['side'] // level['side']
                if scale > region['cell']:
                    unsafe.append(dict(side=level['side'], reason='mip merges source cells'))
                    continue
                lo = [region['x']//scale, region['y']//scale]
                hi = [v + region['cell']//scale - 1 for v in lo]
                if any(a < b for a,b in zip(level['min'],lo)) or any(a > b for a,b in zip(level['max'],hi)):
                    unsafe.append(dict(side=level['side'], reason='weighted taps cross source cell',
                                       observed_min=level['min'], observed_max=level['max'], cell_min=lo, cell_max=hi))
            if unsafe:
                rows.append(dict(frame=data['frame'],group=group,texture=texture,
                                 meshlet=item['meshlet'],levels=unsafe))
    return dict(format='tankracer.atlas-footprint-check.v1',artifact_id=build['artifact_id'],
                frames=len(paths),opaque_samples=samples,embedded_rgba_samples=rgba_samples,source_cell_isolated=not rows,
                unsafe_mesh_frame_cases=len(rows),cases=rows,
                scope='Exact weighted-tap bounds for exercised meshlets. No claim about untested cameras or texel-value equality after UV quantization.')


if __name__ == '__main__':
    p=argparse.ArgumentParser(__doc__)
    p.add_argument('snapshot',type=Path)
    p.add_argument('frames',type=Path)
    p.add_argument('--atlas',type=Path,required=True)
    p.add_argument('--source',type=Path,required=True)
    p.add_argument('--out',type=Path,required=True)
    args=p.parse_args()
    if args.out.exists():
        p.error('Use a fresh output path')
    result=check(args.snapshot,args.frames,args.atlas,args.source)
    args.out.write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps({k:v for k,v in result.items() if k!='cases'}))
    raise SystemExit(0 if result['source_cell_isolated'] else 1)
