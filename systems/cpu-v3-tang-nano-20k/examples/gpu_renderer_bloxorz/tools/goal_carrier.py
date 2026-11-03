"""Read-only planar goal apertures and baked ordinary-floor material phases."""
import math
import struct


def prepare(w, system, output):
    from gpu_texture_layout import level_offset, pack_levels
    texture = (output / 'texture.bin').read_bytes()

    def expand(value):
        r, g, b = value >> 11, (value >> 5) & 63, value & 31
        return [(r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2)]

    def sample(uv):
        result = [0, 0, 0]
        for exponent, parent in [(6, 319), (5, 192)]:
            xy = [((int(v * 65535) & 65535) * (1 << exponent) >> 8) - 128 for v in uv]
            x, y = [v >> 8 for v in xy]
            fx, fy = [v & 255 for v in xy]
            bottom = parent * fy >> 8
            top = parent - bottom
            rt, rb = top * fx >> 8, bottom * fx >> 8
            for tap, weight in enumerate([top - rt, rt, bottom - rb, rb]):
                address = sum(2 * max(16, 1 << e) ** 2 for e in range(exponent))
                address += level_offset(exponent, (x + (tap & 1)) & ((1 << exponent) - 1), (y + (tap >> 1)) & ((1 << exponent) - 1))
                color = expand(struct.unpack_from('<H', texture, address)[0])
                for c in range(3):
                    result[c] += (color[c] * weight + 256) >> 9
        return result

    binary = bytearray()
    for phase in range(4):
        pixels = []
        for y in range(128):
            for x in range(128):
                wx, wz = [(min(123, max(0, v - 2)) / 123 * 3 - 1.5) for v in [x, y]]
                if abs(wx) <= .5 and abs(wz) <= .5:
                    rgb = [224, 88, 88]
                else:
                    dx, dz = [min(1, max(-1, math.floor(v + .5))) for v in [wx, wz]]
                    fx, fz = [min(1, max(0, v - d + .5)) for v, d in [(wx, dx), (wz, dz)]]
                    u, v, s, t = w.uv_rect((phase + dx * 3 + dz * 5) % 4)
                    rgb = sample([u + fx * s, 1 - v - t + fz * t])
                    for c, tint in enumerate([82, 100, 121]):
                        value = rgb[c] * tint + 128
                        rgb[c] = (((value + (value >> 8)) >> 8) * 232) >> 8
                channels = [(v * maximum + 127) // 255 for v, maximum in zip(rgb, [31, 63, 31])]
                pixels.append((channels[0] << 11) | (channels[1] << 5) | channels[2])
        binary.extend(pack_levels({7: pixels}))
    assert len(binary) == 4 * 32768
    (output / 'goal_carrier.bin').write_bytes(binary)

    carriers, hatches = [], []
    corners = [(-3, -3), (3, -3), (3, 3), (-3, 3)]
    for index in range(48):
        n = round(index * .02 * 128) / 128
        inside = [(-n, -n), (n, -n), (n, n), (-n, n)]
        if index == 0:
            faces = [corners]
        else:
            faces = [[corners[i], corners[(i + 1) % 4], inside[(i + 1) % 4], inside[i]] for i in range(4)]
        quads = [([(x, 0, z) for x, z in face], (0, 511, 0), (0, 0, 1, 1)) for face in faces]
        mesh = w.mesh('goal-carrier-' + str(index), quads, cull_negative=True)
        for vertex in mesh['vertices']:
            xyz = [mesh['config'][axis] + (((vertex[0] >> (axis * 10)) & 1023) << 9) for axis in range(3)]
            uv = [round((2.5 + (xyz[axis] / 65536 + 3) / 6 * 123) * 65535 / 128) for axis in [0, 2]]
            vertex[2] = uv[0] | (uv[1] << 16)
        carriers.append(mesh)
        # Index0 is never a visible hatch; retain a valid nondegenerate record.
        h = max(n, 1 / 128)
        u, v, s, t = w.uv_rect(0)
        f = h / .995
        crop = (1 - f) / 2
        hatches.append(w.mesh('goal-hatch-' + str(index), [([(-h, 0, -h), (h, 0, -h), (h, 0, h), (-h, 0, h)], (0, 511, 0), (u + crop * s, v + crop * t, s * f, t * f))], cull_negative=True))
    return [carriers, hatches]


def applicable(level):
    width, depth = level['width'], level['depth']
    goal = next(i for i, cell in enumerate(level['cells']) if cell[0] == 6)
    gx, gz = goal % width, goal // width
    return all(0 <= gx + dx < width and 0 <= gz + dz < depth and level['cells'][(gz + dz) * width + gx + dx][0] == 1
               for dz in [-1, 0, 1] for dx in [-1, 0, 1] if dx or dz)
