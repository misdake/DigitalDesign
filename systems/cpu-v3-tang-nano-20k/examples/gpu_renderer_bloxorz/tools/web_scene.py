"""Portable geometry and low atlas for the first two Bloxorz Web stages."""
import math
import colorsys
from collections import defaultdict

from PIL import Image


def camera(width, depth):
    direction = [math.sin(math.pi / 12) * .85, .8,
                 math.cos(math.pi / 12) * .85]
    length = math.sqrt(sum(v * v for v in direction))
    ux, uy, uz = [v / length for v in direction]
    xn = math.hypot(uz, ux)
    xx, xz = uz / xn, -ux / xn
    yx, yy, yz = uy * xz, uz * xx - ux * xz, -uy * xx
    tan_v = math.tan(math.pi / 18)
    required = 8
    for x in [-width / 2, width / 2]:
        for z in [-depth / 2, depth / 2]:
            for height in [0, 2]:
                y = height - .6
                dot = x * ux + y * uy + z * uz
                required = max(required, abs(x * xx + z * xz) / (tan_v * 5 / 3) + dot,
                               abs(x * yx + y * yy + z * yz) / tan_v + dot)
    distance = max(8, min(130, required * 1.03))
    return dict(eye=[ux * distance, .6 + uy * distance, uz * distance],
                target=[0, .6, 0], focal=120 / tan_v, distance=distance)


def uv_rect(which):
    if isinstance(which, int):
        return ((which % 2) * .25 + 16 / 1024,
                .5 + (which // 2) * .25 + 16 / 1024, 224 / 1024, 224 / 1024)
    return {'side-a': (.5 + 16 / 1024, .5 + 16 / 1024, 480 / 1024, 480 / 1024),
            'top': (16 / 1024, 16 / 1024, 480 / 1024, 480 / 1024),
            'side-b': (.5 + 16 / 1024, 16 / 1024, 480 / 1024, 480 / 1024)}[which]


def mesh(name, quads):
    # Native unsigned delta10, Q16 positions at 1/128 source-unit resolution.
    step = 512
    positions = [[round(v * 65536 / step) * step for v in point]
                 for points, _, _ in quads for point in points]
    origins = [min(p[i] for p in positions) for i in range(3)]
    vertices, triangles = [], []
    cursor = 0
    for points, normal, rect in quads:
        first = len(vertices)
        for uv in [(0, 1), (1, 1), (1, 0), (0, 0)]:
            xyz = positions[cursor]
            cursor += 1
            delta = [(xyz[i] - origins[i]) // step for i in range(3)]
            assert all(0 <= d < 1024 for d in delta)
            pos = sum(d << (10 * i) for i, d in enumerate(delta))
            n = sum((v & 1023) << (10 * i) for i, v in enumerate(normal))
            u = round((rect[0] + uv[0] * rect[2]) * 65535)
            v = round((1 - rect[1] - uv[1] * rect[3]) * 65535)
            vertices.append([pos, n, u | (v << 16)])
        triangles += [[first, first + 1, first + 2], [first, first + 2, first + 3]]
    assert len(vertices) <= 64 and len(triangles) <= 96
    return dict(name=name, group=name, vertices=vertices, triangles=triangles,
                config=origins + [9 | 256], color_mode=0, cull_negative=False)


def floor(level, cells, name, variant=None):
    quads = []
    for x, z in cells:
        cx, cz = 2 * x - (level['width'] - 1), 2 * z - (level['depth'] - 1)
        x0, x1, z0, z1 = cx - 1, cx + 1, cz - 1, cz + 1
        rect = uv_rect((x * 3 + z * 5) % 4 if variant is None else variant)
        quads += [([(x0, 0, z0), (x1, 0, z0), (x1, 0, z1), (x0, 0, z1)], (0, 511, 0), rect),
                  ([(x1, 0, z1), (x1, 0, z0), (x1, -.4, z0), (x1, -.4, z1)], (511, 0, 0), rect),
                  ([(x0, 0, z1), (x1, 0, z1), (x1, -.4, z1), (x0, -.4, z1)], (0, 0, 511), rect)]
    return mesh(name, quads)


def floors(level, name):
    buckets = defaultdict(list)
    for i, cell in enumerate(level['cells']):
        x, z = i % level['width'], i // level['width']
        if cell[0] not in (0, 3, 4, 5, 6) and [x, z] != level['start'][:2]:
            buckets[(x // 3, z // 3)].append((x, z))
    return [floor(level, cells[i:i + 5], name) for cells in buckets.values()
            for i in range(0, len(cells), 5)]


def block():
    faces = [(0, 1, 'side-a', [(1, 1, 1), (1, 1, -1), (1, -1, -1), (1, -1, 1)]),
             (0, -1, 'side-a', [(-1, 1, -1), (-1, 1, 1), (-1, -1, 1), (-1, -1, -1)]),
             (1, 1, 'top', [(-1, 1, -1), (1, 1, -1), (1, 1, 1), (-1, 1, 1)]),
             (1, -1, 'top', [(-1, -1, 1), (1, -1, 1), (1, -1, -1), (-1, -1, -1)]),
             (2, 1, 'side-b', [(-1, 1, 1), (1, 1, 1), (1, -1, 1), (-1, -1, 1)]),
             (2, -1, 'side-b', [(1, 1, -1), (-1, 1, -1), (-1, -1, -1), (1, -1, -1)])]
    quads = []
    for axis, sign, rect_name, face in faces:
        normal = [0, 0, 0]
        normal[axis] = sign * 511
        quads.append(([tuple(c * .49 for c in p) for p in face], normal, uv_rect(rect_name)))
    return mesh('block', quads)


def plain(name, quads):
    result = mesh(name, quads)
    result['color_mode'] = 1
    for vertex in result['vertices']:
        vertex[2] = 0xffffff
    return result


def box_quads(center, size):
    template = [
        (0, 1, [(1,1,1),(1,1,-1),(1,-1,-1),(1,-1,1)]),
        (0,-1, [(-1,1,-1),(-1,1,1),(-1,-1,1),(-1,-1,-1)]),
        (1, 1, [(-1,1,-1),(1,1,-1),(1,1,1),(-1,1,1)]),
        (1,-1, [(-1,-1,1),(1,-1,1),(1,-1,-1),(-1,-1,-1)]),
        (2, 1, [(-1,1,1),(1,1,1),(1,-1,1),(-1,-1,1)]),
        (2,-1, [(1,1,-1),(-1,1,-1),(-1,-1,-1),(1,-1,-1)])]
    result = []
    for axis, sign, points in template:
        normal = [0, 0, 0]
        normal[axis] = sign * 511
        result.append(([tuple(center[i] + p[i] * size[i] / 2 for i in range(3))
                        for p in points], normal, uv_rect(0)))
    return result


def well():
    quads = box_quads((0, -2.14, 0), (1, .12, 1))
    for width, height, y in [(.99, .195, -.1025), (1, 1.88, -1.14)]:
        offset = (width - .03) / 2
        for sign in [-1, 1]:
            quads += box_quads((0, y, sign * offset), (width, height, .03))
            quads += box_quads((sign * offset, y, 0), (.03, height, .94 if height < 1 else 1))
    return [plain('well', quads[i:i + 15]) for i in range(0, len(quads), 15)]


def soft_glyph():
    quads = []
    for i in range(8):
        a, b = i * math.pi / 4, (i + 1) * math.pi / 4
        points = [(math.cos(t) * r, 0, math.sin(t) * r)
                  for t, r in [(a, .15), (b, .15), (b, .25), (a, .25)]]
        quads.append((points, (0, 511, 0), uv_rect(0)))
    return plain('soft-glyph', quads)


def heavy_base():
    return plain('heavy-base', box_quads((0, 0, 0), (.44, .05, .44)))


def heavy_glyph():
    return plain('heavy-glyph', box_quads((0, 0, 0), (.26, .06, .26)))


def palette(hex_rgb):
    # Three.Color HSL operates on linear-sRGB, including palette multiplication.
    encoded = [(hex_rgb >> shift & 255) / 255 for shift in [16, 8, 0]]
    linear = [v / 12.92 if v <= .04045 else ((v + .055) / 1.055) ** 2.4 for v in encoded]
    h, lightness, saturation = colorsys.rgb_to_hls(*linear)
    base = colorsys.hls_to_rgb(h, min(1, lightness * 1.04), saturation * .62)
    variants = [[v * .5 for v in base], base,
                [v + (1 - v) * .45 for v in base], [v * .2 for v in base]]
    return [[round(255 * (v * 12.92 if v <= .0031308 else 1.055 * v ** (1/2.4) - .055))
             for v in color] for color in variants]


def texture(system):
    import sys
    sys.path.insert(0, str(system / 'tools'))
    from gpu_texture_layout import pack_levels
    atlas = Image.open(system / 'examples/bloxorz/assets/atlas-low.png').convert('RGB')
    assert atlas.size == (512, 512)
    levels = {}
    for exponent in range(10):
        image = atlas.resize((1 << exponent, 1 << exponent), Image.Resampling.BOX)
        levels[exponent] = [((r * 31 + 127) // 255 << 11) |
                            ((g * 63 + 127) // 255 << 5) | ((b * 31 + 127) // 255)
                            for r, g, b in image.getdata()]
    return pack_levels(levels)
