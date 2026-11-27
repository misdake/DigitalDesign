"""Build a borderless fixed-grid candidate without changing the bordered baseline.

The two-bit descriptor selects the number of cells per axis (1,2,4,8).
All source mip data comes from the retained native archive. Uniform cells may
resample a source to fit. Native-cell packing preserves larger sources across
multiple cells, while every draw keeps the same sampling grid. Unused cells
are filled with an existing source, never black.
"""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import struct
import zipfile

from opaque_atlas import unpack, pack_levels, average


def rgba_mips(pixels, size):
    from texture_source import encode_pixels

    rgba = [tuple(((p >> shift) & 15)*17 for shift in (12, 8, 4, 0)) for p in pixels]
    levels = unpack(encode_pixels(size, rgba, 'Rgba4444', mipmaps=True), size)
    # Preserve every original base word, including RGB under zero alpha.
    levels[size.bit_length()-1] = pixels
    return levels


def rgba_region(payload, side, x, y, size):
    from gpu_texture_layout import level_offset

    if len(payload) != max(512, side*side*2) or min(x,y) < 0 or max(x,y)+size > side:
        raise ValueError('RGBA source footprint mismatch')
    return [struct.unpack_from('<H', payload, level_offset(side.bit_length()-1, x+i, y+j))[0]
            for j in range(size) for i in range(size)]


def grid_uv(value, origin, cell, side):
    # Leave room for the interpolator to truncate a Q16 endpoint.
    return max(origin/side+2/65536,
               min((origin+value*cell)/side, (origin+cell)/side-2/65536))


def merge_alpha_pages(package, scene, moved):
    """Recover native cells from the archived alpha pages using their inventory."""
    inventory = json.loads(package.read('inventory.json'))
    pages = inventory['alpha_split']['pages']
    names = [name for page in pages for name in page]
    if len(names) != 17 or len(set(names)) != 17 or moved not in names:
        raise ValueError('Expected 17 alpha sources and one source to move')
    cells = {name: ((i % 4)*64, (i // 4)*64)
             for i, name in enumerate(name for name in names if name != moved)}
    rows = {row['name']:row for row in inventory['textures'] if row['name'] in names}
    if sorted(rows) != sorted(names):
        raise ValueError('Incomplete alpha page inventory')
    page_files = {row['file'] for row in rows.values()}
    payloads = {name:package.read(name) for name in page_files}
    image = {exponent:[0]*(1 << (2*exponent)) for exponent in range(9)}
    moved_levels = {}
    for name,row in rows.items():
        region = row['region']
        if row['size'] != 256 or row['mipmaps'] or region['width'] != 64 or region['height'] != 64:
            raise ValueError('Expected base-only 256-square pages of 64-square cells')
        raw = rgba_region(payloads[row['file']], 256, region['x'], region['y'], 64)
        levels = rgba_mips(raw, 64)
        if name == moved:
            moved_levels = levels
            continue
        x,y = cells[name]
        for exponent,pixels in levels.items():
            size, level_side = 1 << exponent, 1 << (exponent+2)
            ox,oy = x >> (6-exponent), y >> (6-exponent)
            for row_index in range(size):
                at = (oy+row_index)*level_side+ox
                image[exponent+2][at:at+size] = pixels[row_index*size:(row_index+1)*size]
    name_in_rgb = f'textures/{moved}.bin'
    combined = 'textures/ALPHA_GRID.bin'
    moved_materials = {}
    materials = scene['materials']
    for group in scene['groups']:
        original = group['material']
        texture = materials[original].get('texture')
        if texture not in page_files:
            continue
        entries = [(name,row['region']) for name,row in rows.items() if row['file'] == texture]
        vertex_uv = 4 if group.get('billboard') else 3
        sources = set()
        vertices = list(group['vertices'])
        if group.get('bounds_geometry'):
            vertices.extend(group['bounds_geometry']['vertices'])
        for vertex in vertices:
            x,y = [vertex[vertex_uv+i]*256 for i in (0,1)]
            matches = [(name,r) for name,r in entries
                       if r['x']-1e-7 <= x <= r['x']+64+1e-7 and r['y']-1e-7 <= y <= r['y']+64+1e-7]
            if len(matches) != 1:
                raise ValueError(f'Ambiguous alpha source in {group["name"]}')
            name,region = matches[0]
            sources.add(name)
            u,v = (x-region['x'])/64,(y-region['y'])/64
            if name == moved:
                vertex[vertex_uv:vertex_uv+2] = [max(0,min(1,u)), max(0,min(1,v))]
            else:
                ox,oy = cells[name]
                vertex[vertex_uv:vertex_uv+2] = [grid_uv(u,ox,64,256), grid_uv(v,oy,64,256)]
        if moved in sources:
            if len(sources) != 1:
                raise ValueError('The moved source shares a render group with other alpha images')
            if original not in moved_materials:
                moved_materials[original] = len(materials)
                materials.append(dict(materials[original], texture=name_in_rgb))
            group['material'] = moved_materials[original]
    if not moved_materials:
        raise ValueError('Moved alpha source must be used by the scene')
    for material in materials:
        if material.get('texture') in page_files:
            material.update(texture=combined, size=256, mipmaps=True, atlas_grid_log2=2)
    return (name_in_rgb, dict(size=64, levels=moved_levels,
                             source_sha256=rows[moved]['source_sha256']),
            page_files, combined, pack_levels(image), cells)


def build(source, destination, side=1024, grid_log2=3, native_cells=False, embed_rgba=None,
          move_alpha_source=None):
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
        rgba_names = set()
        moved_name = None
        if embed_rgba is not None:
            if not native_cells or embed_rgba in sources:
                raise ValueError('RGBA embedding requires native-cell packing and a separate source')
            material = next((m for m in scene['materials'] if m.get('texture') == embed_rgba), None)
            if (material is None or not material.get('rgba') or not material.get('blend')
                    or material.get('encoding') != 'gpu-column16' or material.get('mipmaps')):
                raise ValueError('Expected a native base-only RGBA source to embed')
            size = material['size']
            payload = package.read(embed_rgba)
            native = rgba_mips(rgba_region(payload, size, 0, 0, size), size)
            sources[embed_rgba] = dict(size=size, levels=native,
                                       source_sha256=hashlib.sha256(payload).hexdigest())
            rgba_names.add(embed_rgba)
        alpha_files = set()
        if move_alpha_source is not None:
            if not native_cells or side >> grid_log2 != 64:
                raise ValueError('Alpha page consolidation needs native 64-square packing')
            moved_name, moved, alpha_files, combined, alpha_payload, alpha_cells = merge_alpha_pages(
                package, scene, move_alpha_source)
            if moved_name in sources:
                raise ValueError('Moved alpha source conflicts with another source')
            sources[moved_name] = moved
            rgba_names.add(moved_name)
        names = sorted(sources)
        count, cell = 1 << grid_log2, side >> grid_log2
        if not names:
            raise ValueError('Expected at least one opaque texture')
        if not native_cells and len(names) > count * count:
            raise ValueError('Source count exceeds fixed grid capacity')
        if native_cells:
            occupied = set()
            regions = {}
            for name in sorted(names, key=lambda name: (name == moved_name, -sources[name]['size'], name)):
                pitch = max(cell, sources[name]['size'])
                units = pitch // cell
                if pitch > side or pitch & (pitch - 1):
                    raise ValueError('Native sources require aligned power-of-two cells')
                found = False
                for y in range(0, count - units + 1, units):
                    for x in range(0, count - units + 1, units):
                        footprint = {(x + dx, y + dy) for dy in range(units) for dx in range(units)}
                        if not footprint & occupied:
                            occupied.update(footprint)
                            regions[name] = dict(x=x * cell, y=y * cell,
                                                 size=sources[name]['size'], cell=pitch, gutter=0)
                            found = True
                            break
                    if found:
                        break
                if not found:
                    raise ValueError('Native sources do not fit the fixed grid')
            fill = [(names[-1], dict(x=x * cell, y=y * cell, cell=cell))
                    for y in range(count) for x in range(count) if (x, y) not in occupied]
        else:
            regions = {name: dict(x=(i % count) * cell, y=(i // count) * cell,
                                 size=sources[name]['size'], cell=cell, gutter=0)
                       for i, name in enumerate(names)}
            fill = [(names[-1], dict(x=(i % count) * cell, y=(i // count) * cell, cell=cell))
                    for i in range(len(names), count * count)]
        levels = {}
        for exponent in range(side.bit_length()):
            mip_side = 1 << exponent
            if exponent < grid_log2:
                continue
            image = [0] * (mip_side * mip_side)
            lod = side.bit_length() - 1 - exponent
            for name, region in [*regions.items(), *fill]:
                tile = region['cell'] >> lod
                ox, oy = region['x'] >> lod, region['y'] >> lod
                item = sources[name]
                native_n = min(tile.bit_length()-1, item['size'].bit_length()-1)
                native_side, pixels = 1 << native_n, item['levels'][native_n]
                for y in range(tile):
                    for x in range(tile):
                        image[(oy+y)*mip_side+ox+x] = pixels[(y*native_side//tile)*native_side+x*native_side//tile]
            levels[exponent] = image
        # Retain the ordinary allocation/layout ABI. These coarse levels are
        # present but must never be sampled in grid mode.
        for exponent in reversed(range(grid_log2)):
            prior, size = levels[exponent+1], 1 << exponent
            levels[exponent] = [average([prior[(2*y+dy)*size*2+2*x+dx]
                for dy in range(2) for dx in range(2)]) for y in range(size) for x in range(size)]
            # A separate RGBA view can continue below the opaque view's limit.
            # Preserve its native tail instead of interpreting these words as RGB565.
            lod = side.bit_length()-1-exponent
            for name in rgba_names:
                region = regions[name]
                tile = region['cell'] >> lod
                if tile == 0:
                    continue
                native = sources[name]['levels'][tile.bit_length()-1]
                x, y = region['x'] >> lod, region['y'] >> lod
                for row in range(tile):
                    levels[exponent][(y+row)*size+x:(y+row)*size+x+tile] = native[row*tile:(row+1)*tile]
        result = copy.deepcopy(scene)
        for group in result['groups']:
            material = scene['materials'][group['material']]
            if not material.get('texture') or (material.get('blend') and material['texture'] not in rgba_names):
                continue
            region = regions[material['texture']]
            at = 4 if group.get('billboard') else 3
            for vertex in group['vertices']:
                for axis, origin in enumerate((region['x'], region['y'])):
                    uv = vertex[at+axis]
                    if not 0 <= uv <= 1:
                        raise ValueError('Source UV is outside the normalized domain')
                    # The existing interpolator can truncate one Q16 unit
                    # below a constant endpoint. Keep both ends two units
                    # inside their cell; this allocates no texture gutter.
                    vertex[at+axis] = grid_uv(uv, origin, region['cell'], side)
        texture = 'textures/OPAQUE_GRID.bin'
        for material in result['materials']:
            if material.get('texture') in rgba_names:
                region = regions[material['texture']]
                rgba_grid = grid_log2 if move_alpha_source is not None else (
                    side.bit_length()-region['cell'].bit_length())
                if not 0 <= rgba_grid <= 3:
                    raise ValueError('Embedded RGBA region exceeds the two-bit grid description')
                region['grid_log2'] = rgba_grid
                material.update(texture=texture, size=side, encoding='gpu-column16',
                                mipmaps=True, clamp=True, rgba=True, atlas_grid_log2=rgba_grid,
                                atlas_region=[region['x'], region['y'], region['cell'], region['cell']])
            if not material.get('blend'):
                if not material.get('texture'):
                    material['vertex_color'] = True
                material.update(texture=texture, size=side, encoding='gpu-column16',
                                mipmaps=True, clamp=True, rgba=False, atlas_grid_log2=grid_log2)
        audit = dict(format='tankracer.opaque-grid.v1', source_sha256=hashlib.sha256(original).hexdigest(),
                     side=side, grid_log2=grid_log2, source_count=len(names), regions=regions,
                     sampling_contract='Fixed-grid local clamp and LOD limit are required; UV endpoints stay inside their original cell.')
        if native_cells:
            audit.update(native_cells=True, used_cells=len(occupied), empty_cells=count*count-len(occupied),
                         sampling_contract='All draws use the same fixed grid. Larger sources span multiple cells; filtering clamps at internal cell boundaries and their final source mip can exceed 1x1.')
        if rgba_names:
            audit['rgba_regions'] = sorted(rgba_names)
            audit['rgb_source_count'] = len(names)-len(rgba_names)
            audit['sampling_contract'] = 'Opaque draws share one grid. Embedded RGBA regions use a separate grid matching their source size and retain their own 1x1 mip. Filtering and mip limits isolate each interpretation.'
        result['opaque_atlas'] = audit
        files = {name: package.read(name) for name in package.namelist()
                 if name not in sources and name not in alpha_files
                 and name not in ('scene.json', 'inventory.json')}
        if move_alpha_source is not None:
            files[combined] = alpha_payload
            audit['sampling_contract'] = ('RGB and RGBA views use the same 64-square grid. '
                'Larger sources span multiple cells and keep a 2x2 source mip tail.')
            result['alpha_atlas'] = dict(texture=combined, side=256, grid_log2=2, cell=64,
                                         regions=alpha_cells, moved_source=move_alpha_source)
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
    parser.add_argument('--native-cells',action='store_true',help='Preserve source resolution across multiple fixed-grid cells')
    parser.add_argument('--embed-rgba',help='Embed one native base-only RGBA texture into the same mip storage')
    parser.add_argument('--move-alpha-source',help='Move one alpha source into the shared atlas and merge the other 16')
    args = parser.parse_args()
    build(args.source,args.output,args.side,args.grid_log2,args.native_cells,args.embed_rgba,args.move_alpha_source)
