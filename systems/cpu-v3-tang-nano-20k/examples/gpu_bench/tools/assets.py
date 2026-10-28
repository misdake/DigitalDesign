"""Static native GPU workloads. No CPU-side CB construction in measured runs."""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import struct
import sys

sys.dont_write_bytecode = True
SYSTEM = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(SYSTEM / "tools"))
from gpu_layout import sectors, validate_asset_range

WIDTH, HEIGHT = 400, 240
WARMUP, SAMPLES = 3, 31
PREFIX = [0, 1, 2, 3, 4, 8, 24, 88, 344, 1368, 5464]


def u32(value):
    return value & 0xffffffff


def native_mesh(points, triangles, *, rgb=True, uv=None, reverse=False, vertices=None):
    """Encode and independently decode the actual V96/index54 record."""
    source = [tuple(round(c * 65536) for c in p) for p in points]
    if vertices:
        source += [source[i % len(source)] for i in range(vertices - len(source))]
    origin = [min(p[k] for p in source) for k in range(3)]
    extent = max(p[k] - origin[k] for p in source for k in range(3))
    shift = max(0, extent.bit_length() - 10)
    while round(extent / (1 << shift)) > 1023:
        shift += 1
    count = len(source)
    assert 1 <= count <= 64 and len(triangles) <= 96
    offset = (3 * count + 1) // 2
    payload = ((offset + (len(triangles) + 2) // 3) * 8 + 127) // 128 * 128
    record = bytearray(128 + payload)
    header = count | len(triangles) << 8 | 20 << 16 | offset << 24 | 1 << 31
    struct.pack_into("<4Q", record, 0, header, int(rgb) << 4,
                     u32(origin[0]) | u32(origin[1]) << 32,
                     u32(origin[2]) | (shift | 256) << 32)
    decoded = []
    for i, p in enumerate(source):
        delta = [round((p[k] - origin[k]) / (1 << shift)) for k in range(3)]
        assert all(0 <= d <= 1023 for d in delta)
        packed = sum(d << (10 * k) for k, d in enumerate(delta))
        # A legal constant unit normal; interpolation still uses the real path.
        normal = 511 << 20
        color = 0x68b8e8
        if not rgb:
            value = uv[i % len(uv)] if uv else (0.25, 0.25)
            color = round(value[0] * 65535) | round(value[1] * 65535) << 16
        struct.pack_into("<3I", record, 128 + 12 * i, packed, normal, color)
        decoded.append(tuple((origin[k] + (delta[k] << shift)) / 65536 for k in range(3)))
    for i, face in enumerate(triangles):
        face = (face[0], face[2], face[1]) if reverse else face
        assert len(face) == 3 and all(0 <= v < count for v in face)
        at = 128 + (offset + i // 3) * 8
        word = struct.unpack_from("<Q", record, at)[0]
        word |= sum(v << (6 * k) for k, v in enumerate(face)) << (18 * (i % 3))
        struct.pack_into("<Q", record, at, word)
    return bytes(record), decoded


def screen_point(x, y, z=2):
    return ((x - 199.5) * z / 256, (y - 119.5) * z / 256, z)


def coverage(points, faces):
    """Float edge oracle of decoded positions, including overlapping fragments."""
    pixels, quads = 0, 0
    for f in faces:
        if min(points[i][2] for i in f) <= 0:
            continue
        p = [(199.5 + 256 * points[i][0] / points[i][2],
              119.5 + 256 * points[i][1] / points[i][2]) for i in f]
        edge = lambda a, b, x, y: (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0])
        if edge(p[0], p[1], *p[2]) <= 0:
            continue
        seen = set()
        for y in range(max(0, math.floor(min(q[1] for q in p))), min(HEIGHT, math.ceil(max(q[1] for q in p)) + 1)):
            for x in range(max(0, math.floor(min(q[0] for q in p))), min(WIDTH, math.ceil(max(q[0] for q in p)) + 1)):
                if all(edge(p[k], p[(k + 1) % 3], x, y) >= 0 for k in range(3)):
                    pixels += 1
                    seen.add((x // 2, y // 2))
        quads += len(seen)
    return pixels, quads


def texture(exponent, mip, palette=False):
    """RGB565 periodic 8x8 tiles; lower levels are actual box-filtered mips."""
    side = 1 << exponent
    pixels = []
    for y in range(side):
        for x in range(side):
            color = (232, 184, 72) if ((x // 16) ^ (y // 16)) & 1 else (48, 112, 208)
            if x % 32 == 0 or y % 32 == 0:
                color = (24, 40, 56)
            pixels.append(color)
    levels = {exponent: pixels}
    if mip:
        for n in range(exponent - 1, -1, -1):
            width = 1 << n
            parent = levels[n + 1]
            levels[n] = [tuple(sum(parent[(2*y+dy)*(2*width)+2*x+dx][c]
                                     for dy in range(2) for dx in range(2)) // 4 for c in range(3))
                         for y in range(width) for x in range(width)]
    if palette:
        colors = [(248, 32, 32), (32, 248, 32), (32, 32, 248), (248, 248, 32),
                  (32, 248, 248), (248, 32, 248), (248, 248, 248), (248, 128, 32), (128, 128, 128)]
        levels = {n: [colors[n]] * (1 << (2*n)) for n in levels}
    out = bytearray()
    for n in range(0 if mip else exponent, exponent + 1):
        width = 1 << n
        for ty in range(max(1, width // 8)):
            for tx in range(max(1, width // 8)):
                for y in range(8):
                    for x in range(8):
                        r, g, b = levels[n][((ty*8+y) % width)*width+(tx*8+x) % width]
                        out += struct.pack("<H", (r >> 3) << 11 | (g >> 2) << 5 | b >> 3)
    return out


def dma(address, qword, size):
    code = {32: 0, 64: 1, 128: 2}[size]
    assert address % size == 0 and qword % 4 == 0
    return 0x10000000 | address // 32 << 10 | qword // 4 << 3 | code << 1


class Arena:
    def __init__(self, base):
        self.base, self.data = base, bytearray()

    def put(self, data):
        self.data += bytes((-len(self.data)) % 128)
        address = self.base + len(self.data)
        self.data += data
        return address

    def commands(self, words):
        assert 0 < len(words) <= 16384 and words[-1] == 0xf0000000
        data = struct.pack('<' + 'I' * len(words), *words)
        return self.put(data + bytes((-len(data)) % 128)), len(words)


def generate(output):
    output.mkdir(parents=True, exist_ok=True)
    layout = sectors()
    arena = Arena(layout['GPU_ASSET_FIRST_SECTOR'] * 128)
    texture_defs = {'small': (5, True), 'medium': (8, True), 'large': (10, False),
                    'mip_ids': (8, True), 'wide_mip': (9, True)}
    textures = {name: arena.put(texture(n, mip, name == 'mip_ids'))
                for name, (n, mip) in texture_defs.items()}
    cases = []
    full = [screen_point(x, y) for x, y in [(-32, -32), (864, -32), (-32, 544)]]
    square = [screen_point(x, y) for x, y in [(96, 64), (160, 64), (96, 128)]]

    def add(name, family, points=None, faces=None, repeats=1, **kw):
        points = full if points is None else points
        faces = [(0, 1, 2)] if faces is None else faces
        rgb = kw.get('rgb', True)
        if family == 'sampling':
            repeats = 8
        record, decoded = native_mesh(points, faces, rgb=rgb, uv=kw.get('uv'),
                                      reverse=kw.get('reverse', False), vertices=kw.get('vertices'))
        record_at = arena.put(record)
        actual_faces = [(f[0], f[2], f[1]) for f in faces] if kw.get('reverse') else faces
        pixel_count, quad_count = coverage(decoded, actual_faces)
        global_data = [199 << 16, 0, 16384 | (57344 << 16), 60621 | (8192 << 16),
                       64 | 192 << 16, kw.get('light_mode', 3) | 12 << 2] + [0] * 10
        tex = kw.get('tex', 'small')
        n, mip = texture_defs[tex]
        render = [textures[tex] // 128 | n << 16 | int(mip) << 20 | kw.get('filter', 0) << 21,
                  0xa0a0a0, kw.get('depth_func', 7) | kw.get('depth_write', 0) << 3 |
                  int(kw.get('blend', False)) << 4 | int(kw.get('unlit', True)) << 5,
                  kw.get('lod_bias', 0) & 255] + [0] * 4
        target = (layout['GPU_COLOR_B_SECTOR'] >> 2) | (layout['GPU_DEPTH_SECTOR'] << 12)
        objects = arena.put(struct.pack('<16I8I8I', *global_data, *render, target, *([0]*7)))
        matrix = [65536, 0, 0, 0, 0, 65536, 0, 0, 0, 0, 65536, 0,
                  65536, 0, 0, 0, 65536, 0, 0, 0, 65536, 65536,
                  0x80ffffff if kw.get('blend') else 0xffffffff] + [0] * 9
        instance = arena.put(struct.pack('<32I', *matrix))
        bindings = [0x40000003, 0x70000000, dma(objects, 264, 64), 0x40000001, 0x50000042,
                 dma(objects+64, 272, 32), 0x40000001, 0x60000044,
                 dma(objects+96, 276, 32), 0x40000001, 0xa0000045,
                 dma(instance, 320, 128), 0x40000001]
        setup = list(bindings)
        if kw.get('cold'):
            setup += [0x90000000]
        setup += [0x80000000, 0x80010000 | kw.get('depth_clear', 65535),
                  0x40000003, 0x70000000, 0xf0000000]
        setup_at, setup_count = arena.commands(setup)
        work = []
        mesh_dma = lambda slot: 0x20000000 | record_at // 128 << 12 | (len(record)//128-2) << 1 | slot
        if family == 'control':
            work = [0]
        elif family == 'clear':
            work = [0x80000000 | int(kw.get('clear_depth', False)) << 16 | kw.get('clear_value', 0x2945)]
        else:
            work = [mesh_dma(0), 0x40000001]
            for i in range(repeats):
                if i + 1 < repeats:
                    work += [mesh_dma((i + 1) % 2)]
                work += [0x30000000 | i % 2]
                if i + 1 < repeats:
                    work += [0x40000001]
        # Binding-valid flags reset on every accepted submission. Include the
        # real binding/DMA prologue in measured work rather than assuming state.
        work = list(bindings) + work + [0x40000003, 0x70000000, 0xf0000000]
        work_at, work_count = arena.commands(work)
        cases.append(dict(id=len(cases), name=name, family=family,
                          setup_base=setup_at, setup_instructions=setup_count,
                          work_base=work_at, work_instructions=work_count,
                          meshlet_base=record_at, meshlet_bytes=len(record), repeats=repeats,
                          vertices=0 if family in ('control', 'clear') else len(decoded)*repeats,
                          triangles=0 if family in ('control', 'clear') else len(faces)*repeats,
                          reference_pixels=0 if family in ('control', 'clear') else pixel_count*repeats,
                          reference_quads=0 if family in ('control', 'clear') else quad_count*repeats,
                          coverage_reference='float edges of decoded V96; shared edges may differ by one pixel',
                          rgb=rgb, texture=tex, texture_log2=n, mip=mip,
                          options=kw))

    add('empty_submission', 'control')
    add('clear_color_plane', 'clear')
    add('clear_depth_plane', 'clear', clear_depth=True, clear_value=65535)
    add('fill_rgb_always', 'fill')
    add('fill_rgb_depth_less_pass', 'fill', depth_func=1)
    add('fill_rgb_depth_less_reject', 'fill', depth_func=1, depth_clear=0)
    add('fill_rgb_depth_write', 'fill', depth_func=1, depth_write=1)
    add('fill_rgb_alpha_blend', 'fill', blend=True)
    add('fill_texture_nearest', 'fill', rgb=False, uv=[(0, 0), (1, 0), (0, 1)], tex='medium')
    add('fill_texture_bilinear', 'fill', rgb=False, uv=[(0, 0), (1, 0), (0, 1)], tex='medium', filter=1)
    add('fill_texture_trilinear', 'fill', rgb=False, uv=[(0, 0), (1, 0), (0, 1)], tex='medium', filter=2, lod_bias=16)
    add('fill_light_ambient', 'lighting', unlit=False, light_mode=1)
    add('fill_light_diffuse', 'lighting', unlit=False, light_mode=2)
    add('fill_light_specular', 'lighting', unlit=False, light_mode=3)
    add('fill_texture_light', 'lighting', unlit=False, rgb=False, tex='medium', filter=2,
        uv=[(0, 0), (1, 0), (0, 1)], lod_bias=16)
    for filt, label in enumerate(['nearest', 'bilinear', 'trilinear']):
        add('sample_constant_' + label, 'sampling', points=square, rgb=False, tex='medium', filter=filt, lod_bias=8)
        add('sample_coherent_' + label, 'sampling', points=square, rgb=False, tex='medium', filter=filt,
            uv=[(0, 0), (0.25, 0), (0, 0.25)], lod_bias=8)
    for filt, label in enumerate(['nearest', 'bilinear']):
        for cold in [False, True]:
            add('sample_large_' + label + ('_cold' if cold else '_warm'), 'sampling', points=square,
                rgb=False, tex='large', filter=filt, uv=[(0, 0), (1, 0), (0, 1)], cold=cold)
    tiny = [screen_point(x, y) for x, y in [(100, 80), (104, 80), (100, 84)]]
    for n in [3, 16, 32, 64]:
        add('vertex_v' + str(n), 'vertex', points=tiny, vertices=n, reverse=True, repeats=128)
    for n in [1, 24, 96]:
        add('triangle_t' + str(n), 'triangle', points=tiny, faces=[(0, 1, 2)]*n,
            reverse=True, repeats=64)
    for name, points in [('behind', [(-0.1, -0.1, -8), (0.1, -0.1, -8), (-0.1, 0.1, -8)]),
                         ('left', [(-20, -0.1, 2), (-19.8, -0.1, 2), (-20, 0.1, 2)]),
                         ('right', [(20, -0.1, 2), (20.2, -0.1, 2), (20, 0.1, 2)]),
                         ('vertical', [(-0.1, 20, 2), (0.1, 20, 2), (-0.1, 20.2, 2)])]:
        add('cull_' + name, 'cull', points=points, vertices=64, faces=[(0, 1, 2)]*96, repeats=64)
    add('cull_degenerate', 'cull', points=[screen_point(x, 100) for x in [100, 110, 120]],
        faces=[(0, 1, 2)]*96, repeats=64)
    for size in [0.5, 1, 2, 8, 32]:
        points = [screen_point(x, y) for x, y in [(100.25, 80.25), (100.25+size, 80.25), (100.25, 80.25+size)]]
        add('raster_size_' + str(size).replace('.', '_'), 'raster', points=points,
            faces=[(0, 1, 2)]*96, repeats=8 if size <= 8 else 1)
    add('raster_thin_diagonal', 'raster', points=[screen_point(x, y) for x, y in
        [(30.25, 30.25), (360.25, 210.25), (360.25, 211.25)]], repeats=4)
    add('raster_oblique_medium', 'raster', points=[screen_point(x, y) for x, y in
        [(50, 20), (330, 180), (150, 220)]], repeats=2)

    # Separate a cache-resident interior from one-axis and corner boundaries.
    center = 67.5 / 256
    for filt, name in [(1, 'bilinear'), (2, 'trilinear')]:
        for phase, value in [('interior', (center, center)), ('x_edge', (.25, center)),
                             ('y_edge', (center, .25))]:
            add('sample_' + name + '_' + phase, 'sampling', points=square, rgb=False,
                tex='medium', filter=filt, uv=[value]*3, lod_bias=8)
    # A finite 1/128-texel/pixel derivative plus positive bias gives useful
    # mip blends while the entire viewport remains inside each mip's tile.
    origin = 65.5 / 256
    interior_uv = [(origin, origin), (origin + 896/(128*256), origin),
                   (origin, origin + 576/(128*256))]
    add('fill_mip_id_bilinear', 'fill', rgb=False, tex='mip_ids', filter=1,
        uv=interior_uv, lod_bias=120)
    for bias in [116, 120, 124]:
        add('fill_mip_id_trilinear_bias_' + str(bias), 'fill', rgb=False,
            tex='mip_ids', filter=2, uv=interior_uv, lod_bias=bias)
    for filt, name in [(1, 'bilinear'), (2, 'trilinear')]:
        add('fill_half_texel_' + name, 'fill', rgb=False, tex='wide_mip', filter=filt,
            uv=[(.125, .125), (1, .125), (.125, .6875)], lod_bias=24)

    finish(output, arena, cases, layout)


def finish(output, arena, cases, layout):
    arena.data += bytes((-len(arena.data)) % 128)
    validate_asset_range(arena.base, len(arena.data))
    blob = output / 'bench.bin'
    blob.write_bytes(arena.data)
    token = int.from_bytes(hashlib.sha256(arena.data).digest()[:2], 'little')
    table = []
    for c in cases:
        table += [c['setup_base']//128, c['setup_instructions']-1,
                  c['work_base']//128, c['work_instructions']-1]
    constants = ('use crate::dsl_rt::*;\n'
                 f'pub const BENCH_CASES:u16={len(cases)};\n'
                 f'pub const BENCH_TOKEN:u16={token};\n'
                 f'pub const BENCH_WARMUP:u16={WARMUP};\n'
                 f'pub const BENCH_SAMPLES:u16={SAMPLES};\n'
                 f'pub const BENCH_PREVIEW_SECTOR:u16={layout["GPU_COLOR_B_SECTOR"]};\n'
                 f'pub static BENCH_TABLE:Buf<u16,{len(table)}>=Buf::new([' + ','.join(map(str, table)) + ']);\n')
    (output / 'bench_assets.rs').write_text(constants, encoding='utf8')
    manifest = dict(version=1, clock_hz=54000000, width=WIDTH, height=HEIGHT, token=token,
                    warmup=WARMUP, samples=SAMPLES, cases=cases,
                    targets=dict(color_base=layout['GPU_COLOR_B_SECTOR']*128,
                                 depth_base=layout['GPU_DEPTH_SECTOR']*128),
                    timing='SEND to completed END/FLUSH; setup separate; headless before first present',
                    resources=[dict(file='bench.bin', address=arena.base, bytes=len(arena.data),
                                    sha256=hashlib.sha256(arena.data).hexdigest())])
    (output / 'assets.json').write_text(json.dumps(manifest, indent=2)+'\n', encoding='utf8')
    print(json.dumps(dict(cases=len(cases), bytes=len(arena.data), token=token)))


def generate_full_scene(output):
    """Reuse the complete tracked demo geometry, camera and native record writer."""
    import importlib.util
    producer = SYSTEM/'examples/gpu_demo/tools/assets.py'
    spec = importlib.util.spec_from_file_location('classic_demo_assets', producer)
    demo = importlib.util.module_from_spec(spec); spec.loader.exec_module(demo)
    source = SYSTEM/'examples/gpu_demo/assets/scene.json'
    scene = json.loads(source.read_text())
    meshes, camera = scene['meshes'], scene['cameras'][0]
    assert len(meshes) == 24 and sum(len(m['triangles']) for m in meshes) == 1141
    output.mkdir(parents=True, exist_ok=True)
    layout = sectors(); arena = Arena(layout['GPU_ASSET_FIRST_SECTOR']*128)
    tex = arena.put(demo.texture())
    records = [(arena.put(record), len(record)) for record in map(demo.native_record, meshes)]
    light = camera['light']
    glob = [199 << 16, u32(light[0] & 65535 | (light[1] & 65535) << 16),
            (light[2] & 65535) | (57344 << 16), 60621 | (8192 << 16),
            64 | (192 << 16), 3 | (12 << 2)] + [0]*10
    global_at = arena.put(struct.pack('<16I', *glob))
    render_at = {}
    for unlit in [False, True]:
        fields = [tex//128 | 6 << 16 | 1 << 20 | 2 << 21, 0xa0a0a0,
                  1 | 1 << 3 | int(unlit) << 5, 0] + [0]*4
        render_at[unlit] = arena.put(struct.pack('<8I', *fields))
    target = (layout['GPU_COLOR_B_SECTOR'] >> 2) | (layout['GPU_DEPTH_SECTOR'] << 12)
    target_at = arena.put(struct.pack('<8I', target, *([0]*7)))
    instance_at = arena.put(struct.pack('<32I', *map(u32, camera['matrix']), 65536, 0xffffffff, *([0]*9)))

    def bindings(unlit):
        return [0x40000003, 0x70000000, dma(global_at, 264, 64), 0x40000001, 0x50000042,
                dma(render_at[unlit], 272, 32), 0x40000001, 0x60000044,
                dma(target_at, 276, 32), 0x40000001, 0xa0000045,
                dma(instance_at, 320, 128), 0x40000001]

    cases = []
    workloads = [('clear_color_plane', [], False), ('clear_depth_plane', [], False),
                 ('box_rgb_no_clear', list(range(7)), False),
                 ('rabbit_no_clear', list(range(7, 24)), False),
                 ('full_no_clear', list(range(24)), False),
                 ('full_with_clear', list(range(24)), True)]
    for name, selected, clear in workloads:
        unlit = meshes[selected[0]]['unlit'] if selected else True
        setup = bindings(unlit) + [0x90000000, 0x80000000, 0x8001ffff,
                                   0x40000003, 0x70000000, 0xf0000000]
        work = bindings(unlit)
        if name.startswith('clear_'):
            work += [0x8001ffff if name == 'clear_depth_plane' else 0x80000000]
        else:
            if clear:
                work += [0x80000000, 0x8001ffff]
            first, size = records[selected[0]]
            work += [0x20000000 | (first//128) << 12 | (size//128-2) << 1, 0x40000001]
            for i, index in enumerate(selected):
                if i+1 < len(selected):
                    address, size = records[selected[i+1]]
                    work += [0x20000000 | (address//128) << 12 | (size//128-2) << 1 | ((i+1) % 2)]
                work += [0x30000000 | (i % 2)]
                if i+1 < len(selected):
                    work += [0x40000001]
                    next_unlit = meshes[selected[i+1]]['unlit']
                    if next_unlit != unlit:
                        work += [0x40000003, dma(render_at[next_unlit], 272, 32), 0x40000001, 0x60000044]
                        unlit = next_unlit
        work += [0x40000003, 0x70000000, 0xf0000000]
        setup_at, setup_count = arena.commands(setup); work_at, work_count = arena.commands(work)
        cases.append(dict(id=len(cases), name=name, family='clear' if not selected else 'scene',
            setup_base=setup_at, setup_instructions=setup_count, work_base=work_at, work_instructions=work_count,
            vertices=sum(len(meshes[i]['vertices']) for i in selected),
            triangles=sum(len(meshes[i]['triangles']) for i in selected),
            reference_pixels=0, reference_quads=0, meshlets=len(selected),
            options=dict(camera=0, filter='trilinear', includes_clear=clear,
                         setup='both planes reset and texture cache invalidated before each sample',
                         source_sha256=hashlib.sha256(source.read_bytes()).hexdigest()),
            commands=work))
    finish(output, arena, cases, layout)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--output', required=True, type=Path)
    generate_selected = generate_full_scene if os.environ.get('GPU_BENCH_SCENE') == 'full' else generate
    generate_selected(parser.parse_args().output)
