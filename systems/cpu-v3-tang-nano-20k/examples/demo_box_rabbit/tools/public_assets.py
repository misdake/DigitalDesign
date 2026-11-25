"""Expose the existing native classic assets through public demo root names."""
import argparse
import importlib.util
import json
import hashlib
import math
import os
from pathlib import Path
import struct
import sys

sys.dont_write_bytecode = True

RING_SECTIONS = 32
RING_TRACK_BATCH = 8


def ring_meshlet(points, triangles, color, depth):
    # All meshlets use the same 1/256 grid so edges agree across slice entries.
    origin_x = min(x for x, _ in points)
    origin_y = min(y for _, y in points)
    vertices = [[(x - origin_x) | ((y - origin_y) << 10), 511 << 20, color]
                for x, y in points]
    assert all(0 <= x - origin_x < 1024 and 0 <= y - origin_y < 1024 for x, y in points)
    return dict(vertices=vertices, triangles=triangles,
                config=[origin_x * 256, origin_y * 256, depth, 8 | 256],
                color_mode=1, cull_negative=False)


def ring_strip(first, count, inner, outer, color, depth):
    points = []
    triangles = []
    # Two straight edges per drawable section keep the annulus smooth.
    steps = RING_SECTIONS * 2
    for i in range(count * 2 + 1):
        angle = math.tau * ((first * 2 + i) % steps) / steps
        for radius in (inner, outer):
            points.append((round(radius * math.sin(angle)), round(radius * math.cos(angle))))
        if i != 0:
            a = (i - 1) * 2
            triangles.extend(((a, a + 1, a + 2), (a + 2, a + 1, a + 3)))
    return ring_meshlet(points, triangles, color, depth)


def pack_ring(output, native, manifest):
    # The fixed pale marker identifies the wrap at twelve o'clock.
    marker = ring_meshlet([(-12, 380), (12, 380), (0, 362)], [(0, 1, 2)], 0xffe9dd, 0)
    palette = (0xf4e135, 0xe7c95d, 0x42b7ee, 0x405bff)
    arc = [ring_strip(i, 1, 245, 345, palette[i // 8], 4096) for i in range(RING_SECTIONS)]
    track = [ring_strip(i, RING_TRACK_BATCH, 243, 347, 0x352b24, 0)
             for i in range(0, RING_SECTIONS, RING_TRACK_BATCH)]
    meshes = arc + track + [marker]
    address = (manifest['texture']['sector'] + manifest['texture']['sectors']) * 128
    payload = bytearray((len(meshes) * 4 + 127) // 128 * 128)
    for i, mesh in enumerate(meshes):
        record = native.native_record(mesh)
        struct.pack_into('<2H', payload, i * 4, (address + len(payload)) // 128, len(record) // 128 - 1)
        payload += record
    native.validate_asset_range(address, len(payload))
    name = 'ring.bin'
    (output / name).write_bytes(payload)
    manifest['resources'].append(dict(file=name, address=address, bytes=len(payload),
                                      sha256=native.sha(output / name), kind='mesh'))
    return address // 128, len(payload) // 128, len(track) + 1


def pack(output, motion='manual'):
    assert motion in ('manual', 'orbit', 'slices', 'ring')
    output = Path(output)
    source = Path(__file__).with_name('assets.py')
    spec = importlib.util.spec_from_file_location('classic_native_assets', source)
    native = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(native)
    native.pack(output)
    manifest = json.loads((output / 'assets.json').read_text())
    mesh, texture = manifest['mesh'], manifest['texture']
    ring_sector, ring_sectors, ring_track_count = (0, 0, 0)
    if motion == 'ring':
        ring_sector, ring_sectors, ring_track_count = pack_ring(output, native, manifest)
    constants = dict(
        GPU_DEMO_AUTO_ORBIT=int(motion == 'orbit'),
        GPU_DEMO_RABBIT_SLICES=int(motion == 'slices'),
        GPU_DEMO_RING_SLICES=int(motion == 'ring'),
        GPU_DEMO_RING_SECTIONS=RING_SECTIONS,
        GPU_DEMO_RING_SECTOR=ring_sector,
        GPU_DEMO_RING_SECTORS=ring_sectors,
        GPU_DEMO_RING_TRACK_COUNT=ring_track_count,
        GPU_RENDERER_MESH_DIRECTORY=mesh['directory_sector'],
        GPU_RENDERER_MESH_DIRECTORY_WORD=mesh['directory_word'],
        GPU_RENDERER_MESH_COUNT=mesh['count'],
        GPU_RENDERER_MESH_FIRST=mesh['first_sector'],
        GPU_RENDERER_MESH_SECTORS=mesh['sectors'],
        GPU_RENDERER_TEXTURE=texture['sector'],
        GPU_RENDERER_TEXTURE_SECTORS=texture['sectors'],
        GPU_RENDERER_TEXTURE_LOG2=texture['size_log2'],
        GPU_RENDERER_TEXTURE_MIP=texture['full_mip'])
    repo = Path(__file__).resolve().parents[5]
    manifest['sources'][Path(__file__).resolve().relative_to(repo).as_posix()] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    (output / 'assets.json').write_text(json.dumps(manifest, indent=2) + '\n')
    (output / 'renderer_assets.rs').write_text(''.join(
        f'pub const {name}:u16={value};\n' for name, value in constants.items()))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--output', type=Path, required=True)
    pack(parser.parse_args().output, os.environ.get('BOX_RABBIT_MOTION', 'manual'))
