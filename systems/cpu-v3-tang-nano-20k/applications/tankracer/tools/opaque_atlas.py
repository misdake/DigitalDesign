"""Build an opt-in opaque atlas from the exact native source mip levels.

Geometry, group identity, tint and transparent assets remain unchanged. Each
source owns a power-of-two cell twice its side, with clamped edge extension.
Coarse atlas levels which merge cells are explicitly not source-isolated; the
sampler probe must audit them before this input can be accepted.
"""
import argparse
import copy
import hashlib
import io
import json
from pathlib import Path
import struct
import sys
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'tools'))
from gpu_texture_layout import level_offset, pack_levels


def unpack(payload, side):
    result, start = {}, 0
    for exponent in range(side.bit_length()):
        size = 1 << exponent
        result[exponent] = [struct.unpack_from('<H', payload, start + level_offset(exponent, x, y))[0]
                            for y in range(size) for x in range(size)]
        start += max(512, size * size * 2)
    if start != len(payload):
        raise ValueError('Source mip extent mismatch')
    return result


def average(values):
    return sum(((sum((p >> shift) & mask for p in values) + 2) // 4) << shift
               for shift, mask in [(11, 31), (5, 63), (0, 31)])


def layout(sources, side):
    occupied = [[False] * (side // 128) for _ in range(side // 128)]
    placed = {}
    for name, item in sorted(sources.items(), key=lambda x: (-x[1]['size'], x[0])):
        size, pitch = item['size'], item['size'] * 2
        units = pitch // 128
        found = False
        for y in range(0, len(occupied) - units + 1, units):
            for x in range(0, len(occupied) - units + 1, units):
                if any(occupied[j][i] for j in range(y, y + units) for i in range(x, x + units)):
                    continue
                for j in range(y, y + units):
                    for i in range(x, x + units):
                        occupied[j][i] = True
                placed[name] = dict(x=x * 128, y=y * 128, size=size, cell=pitch, gutter=size // 2)
                found = True
                break
            if found:
                break
        if not found:
            raise ValueError('Opaque sources do not fit the atlas')
    return placed


def pixels(sources, placed, side):
    levels = {}
    for lod in range(side.bit_length()):
        size = side >> lod
        if lod == 0:
            level = [0] * (size * size)
        else:
            prior = levels[(size * 2).bit_length() - 1]
            level = [average([prior[(y * 2 + dy) * size * 2 + x * 2 + dx]
                              for dy in range(2) for dx in range(2)])
                     for y in range(size) for x in range(size)]
        for name, region in placed.items():
            cell = region['cell'] >> lod
            if cell == 0:
                continue  # This mip merges source cells; retain the explicit reduction.
            x, y = region['x'] >> lod, region['y'] >> lod
            original = region['size']
            exponent = max(0, original.bit_length() - 1 - lod)
            source = sources[name]['levels'][exponent]
            source_side = 1 << exponent
            # Once a source becomes 1x1, extend its exact native tail throughout
            # its complete cell rather than averaging the base image again.
            gutter = region['gutter'] >> lod
            for j in range(cell):
                sy = min(source_side - 1, max(0, j - gutter))
                for i in range(cell):
                    sx = min(source_side - 1, max(0, i - gutter))
                    level[(y + j) * size + x + i] = source[sy * source_side + sx]
        levels[size.bit_length() - 1] = level
    return levels


def build(source, destination, side=1024):
    if destination.exists():
        raise ValueError('Use a fresh output directory')
    original = Path(source).read_bytes()
    with zipfile.ZipFile(io.BytesIO(original)) as package:
        scene = json.loads(package.read('scene.json'))
        sources = {}
        for material in scene['materials']:
            if material.get('blend') or not material.get('texture'):
                continue
            if (material.get('rgba', False) or not material.get('clamp') or not material.get('mipmaps')
                    or material.get('encoding') != 'gpu-column16' or material['size'] not in (64, 128)):
                raise ValueError('Opaque atlas requires clamped 64/128 RGB565 native full mips')
            name = material['texture']
            if name not in sources:
                data = package.read(name)
                sources[name] = dict(size=material['size'], levels=unpack(data, material['size']),
                                     sha256=hashlib.sha256(data).hexdigest())
        placed = layout(sources, side)
        payload = pack_levels(pixels(sources, placed, side))
        result = copy.deepcopy(scene)
        atlas_file = 'textures/OPAQUE_ATLAS.bin'
        for group in result['groups']:
            material = scene['materials'][group['material']]
            if material.get('blend') or not material.get('texture'):
                continue
            region = placed[material['texture']]
            for geometry in [group, *([group['lod']] if isinstance(group.get('lod'), dict) else [])]:
                at = 4 if group.get('billboard') else 3
                for vertex in geometry['vertices']:
                    for axis, origin in enumerate((region['x'], region['y'])):
                        uv = vertex[at + axis]
                        if not 0 <= uv <= 1:
                            raise ValueError('Source vertex UV is outside the normalized domain')
                        vertex[at + axis] = (origin + region['gutter'] + uv * region['size']) / side
        for material in result['materials']:
            if not material.get('blend'):
                if not material.get('texture'):
                    # Native RGB triangles bypass sampling and can share the
                    # same installed CP material as UV triangles.
                    material['vertex_color'] = True
                material.update(texture=atlas_file, size=side, encoding='gpu-column16',
                                mipmaps=True, clamp=True, rgba=False)
        audit = dict(format='tankracer.opaque-atlas.v1', source_sha256=hashlib.sha256(original).hexdigest(),
                     side=side, bytes=len(payload), source_count=len(sources),
                     regions={name: dict(**region, source_sha256=sources[name]['sha256'],
                                         isolated_mip_through=region['size'].bit_length() - 1)
                              for name, region in placed.items()},
                     sampling_contract='Source texels are isolated through the listed mip levels; deeper atlas levels merge source cells. Validate sampled footprints and rendered quality for the intended content.')
        result['opaque_atlas'] = audit
        destination.mkdir(parents=True)
        files = {name: package.read(name) for name in package.namelist()
                 if name not in sources and name != 'scene.json'}
        files['scene.json'] = json.dumps(result, separators=(',', ':')).encode()
        files[atlas_file] = payload
        if 'inventory.json' in files:
            inventory = json.loads(files['inventory.json'])
            provenance = [entry for entry in inventory['textures'] if entry['file'] in sources]
            inventory['textures'] = [entry for entry in inventory['textures'] if entry['file'] not in sources]
            inventory['textures'].append(dict(name='OPAQUE_ATLAS', file=atlas_file, size=side,
                bytes=len(payload), sha256=hashlib.sha256(payload).hexdigest(), format='Rgb565', mipmaps=True,
                levels=[dict(size=1 << i, bytes=max(512, 2 << (2*i))) for i in range(side.bit_length())]))
            inventory['texture_bytes'] = sum(len(data) for name,data in files.items() if name.endswith('.bin'))
            inventory['opaque_atlas'] = dict(**audit, source_textures=provenance)
            files['inventory.json'] = (json.dumps(inventory, indent=2) + '\n').encode()
        with zipfile.ZipFile(destination / 'scene.zip', 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=9) as out:
            for name, data in sorted(files.items()):
                info = zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0))
                info.create_system = 3
                info.compress_type = zipfile.ZIP_DEFLATED
                info.external_attr = 0o100644 << 16
                out.writestr(info, data, compresslevel=9)
    (destination / 'atlas.json').write_text(json.dumps(audit, indent=2) + '\n')
    print(json.dumps({k: v for k, v in audit.items() if k != 'regions'}))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    build(args.source, args.output)
