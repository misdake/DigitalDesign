"""Build a borderless fixed-grid candidate without changing the bordered baseline.

The two-bit descriptor selects the number of cells per axis (1,2,4,8).
All source mip data comes from the retained native archive. Larger sources use
their existing mip; smaller ones repeat nearest texels only above their native
resolution. Unused cells are filled with an existing source, never black.
"""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import zipfile

from opaque_atlas import unpack, pack_levels, average


def build(source, destination, side=1024, grid_log2=3):
    if destination.exists() or not 0 <= grid_log2 <= 3:
        raise ValueError('Use a fresh output directory and a two-bit grid')
    if side not in (64, 128, 256, 512, 1024) or side >> grid_log2 < 1:
        raise ValueError('Unsupported atlas extent')
    original = source.read_bytes()
    with zipfile.ZipFile(source) as package:
        scene = json.loads(package.read('scene.json'))
        sources = {}
        for material in scene['materials']:
            if material.get('blend') or not material.get('texture'):
                continue
            if (material.get('encoding') != 'gpu-column16' or material.get('rgba')
                    or not material.get('mipmaps') or not material.get('clamp')):
                raise ValueError('Expected clamped native RGB565 full mip sources')
            name = material['texture']
            payload = package.read(name)
            sources[name] = dict(size=material['size'], levels=unpack(payload, material['size']),
                                 source_sha256=hashlib.sha256(payload).hexdigest())
        names = sorted(sources)
        count, cell = 1 << grid_log2, side >> grid_log2
        if len(names) > count * count:
            raise ValueError('Source count exceeds fixed grid capacity')
        regions = {name: dict(x=(i % count) * cell, y=(i // count) * cell,
                             size=sources[name]['size'], cell=cell, gutter=0)
                   for i, name in enumerate(names)}
        levels = {}
        for exponent in range(side.bit_length()):
            mip_side = 1 << exponent
            if exponent < grid_log2:
                continue
            tile = mip_side >> grid_log2
            image = [0] * (mip_side * mip_side)
            for index in range(count * count):
                item = sources[names[min(index, len(names)-1)]]
                native_n = min(tile.bit_length()-1, item['size'].bit_length()-1)
                native_side, pixels = 1 << native_n, item['levels'][native_n]
                for y in range(tile):
                    for x in range(tile):
                        image[((index // count) * tile+y)*mip_side+(index % count)*tile+x] = pixels[(y*native_side//tile)*native_side+x*native_side//tile]
            levels[exponent] = image
        # Retain the ordinary allocation/layout ABI. These coarse levels are
        # present but must never be sampled in grid mode.
        for exponent in reversed(range(grid_log2)):
            prior, size = levels[exponent+1], 1 << exponent
            levels[exponent] = [average([prior[(2*y+dy)*size*2+2*x+dx]
                for dy in range(2) for dx in range(2)]) for y in range(size) for x in range(size)]
        result = copy.deepcopy(scene)
        for group in result['groups']:
            material = scene['materials'][group['material']]
            if material.get('blend') or not material.get('texture'):
                continue
            region = regions[material['texture']]
            for geometry in [group, *([group['lod']] if isinstance(group.get('lod'), dict) else [])]:
                at = 4 if group.get('billboard') else 3
                for vertex in geometry['vertices']:
                    for axis, origin in enumerate((region['x'], region['y'])):
                        uv = vertex[at+axis]
                        if not 0 <= uv <= 1:
                            raise ValueError('Source UV is outside the normalized domain')
                        # The existing interpolator can truncate one Q16 unit
                        # below a constant endpoint. Keep both ends two units
                        # inside their cell; this allocates no texture gutter.
                        vertex[at+axis] = max(origin/side+2/65536,
                            min((origin+uv*cell)/side, (origin+cell)/side-2/65536))
        texture = 'textures/OPAQUE_GRID.bin'
        for material in result['materials']:
            if not material.get('blend'):
                if not material.get('texture'):
                    material['vertex_color'] = True
                material.update(texture=texture, size=side, encoding='gpu-column16',
                                mipmaps=True, clamp=True, rgba=False, atlas_grid_log2=grid_log2)
        audit = dict(format='tankracer.opaque-grid.v1', source_sha256=hashlib.sha256(original).hexdigest(),
                     side=side, grid_log2=grid_log2, source_count=len(names), regions=regions,
                     sampling_contract='Fixed-grid local clamp and LOD limit are required; UV endpoints stay inside their original cell.')
        result['opaque_atlas'] = audit
        files = {name: package.read(name) for name in package.namelist()
                 if name not in sources and name not in ('scene.json', 'inventory.json')}
        files['scene.json'] = json.dumps(result, separators=(',', ':')).encode()
        files[texture] = pack_levels(levels)
        destination.mkdir(parents=True)
        with zipfile.ZipFile(destination/'scene.zip', 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=9) as out:
            for name, data in sorted(files.items()):
                info = zipfile.ZipInfo(name, (1980,1,1,0,0,0))
                info.create_system, info.compress_type, info.external_attr = 3, zipfile.ZIP_DEFLATED, 0o100644 << 16
                out.writestr(info, data, compresslevel=9)
    (destination/'atlas.json').write_text(json.dumps(audit,indent=2)+'\n')
    print(json.dumps({k:v for k,v in audit.items() if k != 'regions'}))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('source',type=Path)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--side',type=int,default=1024)
    parser.add_argument('--grid-log2',type=int,default=3)
    args = parser.parse_args()
    build(args.source,args.output,args.side,args.grid_log2)
