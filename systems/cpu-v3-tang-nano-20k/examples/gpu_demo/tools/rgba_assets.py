"""Classic scene plus a straight-alpha RGBA4444 texture and one reusable quad."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import sys

sys.dont_write_bytecode = True


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def alpha_levels():
    """Keep RGB in transparent texels; box-filter straight channels before packing."""
    from gpu_texture_layout import rgba4444
    size = 128
    colors = [(255, 68, 204), (51, 221, 255), (255, 221, 51)]
    pixels = []
    for y in range(size):
        for x in range(size):
            color = colors[min(2, y * 3 // size)]
            alpha = (x // 8) * 17
            if (x - 64) ** 2 + (y - 64) ** 2 < 20 ** 2:
                alpha = 0
            pixels.append((*color, alpha))
    levels = {}
    for exponent in range(7, -1, -1):
        levels[exponent] = [rgba4444(pixel) for pixel in pixels]
        if exponent:
            width = 1 << exponent
            pixels = [tuple((sum(pixels[(2*y+dy)*width+2*x+dx][c]
                                 for dy in range(2) for dx in range(2)) + 2) // 4
                            for c in range(4))
                      for y in range(width // 2) for x in range(width // 2)]
    assert {p & 15 for p in levels[7]} == set(range(16))
    return levels


def pack(output):
    output = Path(output)
    classic = load('rgba_classic_public_assets', Path(__file__).with_name('public_assets.py'))
    classic.pack(output)
    native = load('rgba_classic_native_assets', Path(__file__).with_name('assets.py'))
    from gpu_layout import validate_asset_range
    from gpu_texture_layout import pack_levels
    manifest = json.loads((output / 'assets.json').read_text())
    mesh = manifest['mesh']
    room = bytearray((output / 'room.bin').read_bytes())
    address = mesh['first_sector'] * 128 + len(room)
    quad = dict(config=[0, 0, 0, 7], color_mode=0,
                vertices=[[x | (y << 10), 511 << 20, u | (v << 16)]
                          for x, y, u, v in [(0, 0, 0, 65535), (512, 0, 65535, 65535),
                                            (512, 512, 65535, 0), (0, 512, 0, 0)]],
                triangles=[[0, 1, 2], [0, 2, 3]])
    record = native.native_record(quad)
    struct.pack_into('<2H', room, 4 * mesh['count'], address // 128, len(record) // 128 - 1)
    room += record
    assert mesh['count'] == 24 and len(room) <= 65536
    assert mesh['first_sector'] * 128 + len(room) <= manifest['texture']['sector'] * 128
    (output / 'room.bin').write_bytes(room)
    mesh['entries'].append(dict(name='alpha_panel', address=address, size=len(record),
                                color_mode=0, unlit=True))
    mesh.update(count=mesh['count'] + 1, sectors=len(room) // 128)
    for resource in manifest['resources']:
        if resource['file'] == 'room.bin':
            resource.update(bytes=len(room), sha256=hashlib.sha256(room).hexdigest())
        validate_asset_range(resource['address'], resource['bytes'])
        if resource['kind'] == 'texture':
            resource['format'] = 'rgb565'
    texture = manifest['texture']
    alpha_address = (texture['sector'] + texture['sectors'] + 1) // 2 * 256
    alpha = pack_levels(alpha_levels())
    validate_asset_range(alpha_address, len(alpha))
    (output / 'alpha.bin').write_bytes(alpha)
    manifest['resources'].append(dict(file='alpha.bin', address=alpha_address, bytes=len(alpha),
                                     sha256=hashlib.sha256(alpha).hexdigest(), kind='texture',
                                     format='rgba4444', size_log2=7, full_mip=True))
    manifest.update(scene='classic_scene_rgba4444_panels', triangles=1145,
                    layout_identity='mixed-16bit-macro16x16-column-major-pad16-cache16x8-8KiB',
                    alpha=dict(format='rgba4444', size_log2=7, full_mip=True,
                               sector=alpha_address // 128, sectors=len(alpha) // 128,
                               mesh_index=24, uniform_alpha=[255, 128]))
    spans = sorted((r['address'], r['address'] + r['bytes']) for r in manifest['resources'])
    assert all(a[1] <= b[0] for a, b in zip(spans, spans[1:]))
    repo = Path(__file__).resolve().parents[5]
    manifest['sources'][Path(__file__).resolve().relative_to(repo).as_posix()] = native.sha(__file__)
    constants = (output / 'renderer_assets.rs').read_text()
    constants = constants.replace('GPU_RENDERER_MESH_COUNT:u16=24;', 'GPU_RENDERER_MESH_COUNT:u16=25;')
    old_sectors = len(room) // 128 - len(record) // 128
    constants = constants.replace(f'GPU_RENDERER_MESH_SECTORS:u16={old_sectors};',
                                  f'GPU_RENDERER_MESH_SECTORS:u16={len(room) // 128};')
    constants += f'pub const DEMO_ALPHA_SECTOR:u16={alpha_address // 128};\n'
    constants += f'pub const DEMO_ALPHA_SECTORS:u16={len(alpha) // 128};\n'
    (output / 'renderer_assets.rs').write_text(constants)
    (output / 'assets.json').write_text(json.dumps(manifest, indent=2) + '\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--output', type=Path, required=True)
    pack(parser.parse_args().output)
