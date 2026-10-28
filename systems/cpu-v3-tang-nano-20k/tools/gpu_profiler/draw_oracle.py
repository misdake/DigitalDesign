"""Independent, deliberately restricted native DRAW color/depth reference.

Uses exact cross products and rational perspective weights, not the GPU's
coefficient reduction, reciprocal table, raster walker or interpolation code.
Unsupported shading/clipping fails explicitly rather than silently approximating
it. This is an offline mathematical oracle, not a bit-exact hardware model.
"""
from dataclasses import dataclass
import struct

from layout import header_layout


def signed(value, bits):
    value &= (1 << bits) - 1
    return value - (1 << bits) if value >> (bits - 1) else value


def cross(a, b):
    return (a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2],
            a[0]*b[1]-a[1]*b[0])


def dot(a, b):
    return sum(x*y for x, y in zip(a, b))


def rgb565(channels):
    r, g, b = channels
    # ROP nearest UNORM8 -> UNORM5/6, not a truncating bit slice.
    return ((r*31+127)//255 << 11) | ((g*63+127)//255 << 5) | ((b*31+127)//255)


def expand565(value):
    r, g, b = value >> 11, (value >> 5) & 63, value & 31
    return (r*8+r//4, g*4+g//16, b*8+b//4)


def depth_pass(func, source, destination):
    return (False, source < destination, source == destination,
            source <= destination, source > destination, source != destination,
            source >= destination, True)[func]


@dataclass
class Result:
    planes: dict
    stable: list
    clear: dict
    caller: dict

    def compare(self, actual, *, depth_tolerance=2, color_tolerance=0):
        """Compare stable drawn pixels with declared budgets; background is exact."""
        if not 0 <= depth_tolerance <= 2 or not 0 <= color_tolerance <= 1:
            raise ValueError('Oracle budgets exceed the supported small-fixture bounds')
        report = dict(stable_pixels=sum(self.stable), edge_pixels=96000-sum(self.stable),
                      stable_drawn_pixels=sum(self.stable[i] for i in self.caller['drawn_pixels']),
                      stable_background_pixels=sum(self.stable[i] and i not in self.caller['drawn_pixels']
                                                   for i in range(96000)),
                      depth_tolerance=depth_tolerance, color_channel_code_tolerance=color_tolerance,
                      mismatches={}, maximum_error={})
        for role in ('color', 'depth'):
            if len(actual[role]) != 96000:
                raise ValueError('Expected exactly 96000 logical pixels per plane')
            failures = []; maximum = 0; count = 0
            for i, (want, got) in enumerate(zip(self.planes[role], actual[role])):
                if not self.stable[i]:
                    continue
                drawn = i in self.caller['drawn_pixels']
                if role == 'depth':
                    error = abs(want-got)
                    budget = depth_tolerance if drawn else 0
                else:
                    error = max(
                        abs(((want >> shift) & mask)-((got >> shift) & mask))
                        for shift, mask in ((11, 31), (5, 63), (0, 31)))
                    budget = color_tolerance if drawn else 0
                maximum = max(maximum, error)
                if error > budget:
                    count += 1
                    if len(failures) < 8:
                        failures.append(dict(x=i % 400, y=i//400, expected=want, actual=got))
            report['mismatches'][role] = dict(count=count, examples=failures)
            report['maximum_error'][role] = maximum
        report['pass'] = all(v['count'] == 0 for v in report['mismatches'].values())
        return report

    def description(self):
        return {k: v for k, v in self.caller.items() if k != 'drawn_pixels'}


def evaluate(capture):
    """Interpret one frozen CB. All geometry must be in front of the camera.

    Supports explicit DMA, global/render/target, opaque unlit DRAW, CLEAR and
    synchronization. UV mode requires a uniform no-mip texture; RGB attributes
    may vary. UI and lighting deliberately remain separate qualification paths.
    """
    h = capture.header; layout = header_layout(h)
    planes = {role: layout.words(capture.read(h[role+'_base'], layout.plane_bytes))
              for role in ('color', 'depth')}
    stable = [True]*96000; scratch = bytearray(4096)
    pages = {}; global_words = None; render = None; target = False
    clear = {}; commands = []; drawn = set(); draws = 0; triangles = 0; culled = 0

    def local(at, size):
        if at < 0 or at+size > len(scratch):
            raise ValueError('Local object outside scratchpad')
        return bytes(scratch[at:at+size])

    def triangle(points, colors, cull, tint, constant):
        nonlocal triangles, culled
        triangles += 1
        if triangles > 8:
            raise ValueError('Independent small-fixture oracle is limited to eight triangles')
        if any(p[2] <= 0 for p in points):
            raise ValueError('DRAW oracle requires positive-Z vertices; no clipping qualification')
        edges = [cross(points[1], points[2]), cross(points[2], points[0]), cross(points[0], points[1])]
        determinant = dot(points[0], edges[0])
        if determinant == 0 or (cull and determinant < 0):
            culled += 1
            return
        if determinant < 0:
            edges = [tuple(-v for v in n) for n in edges]
            determinant = -determinant
        equations = [(2*n[0], 2*n[1], -399*n[0]-239*n[1]+512*n[2]) for n in edges]
        far = global_words[0] >> 8
        func, write = render[2] & 7, bool(render[2] & 8)
        for y in range(240):
            for x in range(400):
                i = y*400+x
                values = [a*x+b*y+c for a, b, c in equations]
                # One-pixel L-infinity band around each ideal edge. Publish its
                # count explicitly; never spend this budget on interior errors.
                if any(abs(v) <= abs(a)+abs(b) for v, (a, b, _) in zip(values, equations)):
                    stable[i] = False
                if not all(v > 0 or (v == 0 and (a > 0 or (a == 0 and b > 0)))
                           for v, (a, b, _) in zip(values, equations)):
                    continue
                denominator = sum(values)
                if denominator <= 0:
                    raise ValueError('Unexpected nonpositive perspective denominator')
                # P is F16; viewport rays have z=512. Thus ideal z*256 is
                # determinant*512*256/(denominator*65536) = 2*det/denominator.
                z = 2*determinant//denominator
                if abs(z-far) <= 2:
                    raise ValueError('Fixture is too close to far for an independent oracle')
                if z > 65535 or z > far:
                    continue
                if planes['depth'][i] != 65535 and abs(z-planes['depth'][i]) <= 2:
                    raise ValueError('Overlapping fixture depths are too close for the oracle budget')
                if not depth_pass(func, z, planes['depth'][i]):
                    continue
                if constant is None:
                    rgb = [sum(values[j]*colors[j][channel] for j in range(3))
                           for channel in range(3)]
                    rgb = [(value+denominator//2)//denominator for value in rgb]
                else:
                    rgb = constant
                rgb = [((p+128)+((p+128) >> 8)) >> 8 for p in (a*b for a, b in zip(rgb, tint))]
                planes['color'][i] = rgb565(rgb)
                if write:
                    planes['depth'][i] = z
                drawn.add(i)

    if not 0 < h['cb_bytes'] <= 1024 or h['cb_bytes'] % 4:
        raise ValueError('Independent small-fixture oracle needs 1..256 whole commands')
    command_bytes = capture.read(h['cb_base'], h['cb_bytes'])
    if struct.unpack_from('<I', command_bytes, len(command_bytes)-4)[0] != 0xf0000000:
        raise ValueError('Missing final END')
    for pc, (word,) in enumerate(struct.iter_unpack('<I', command_bytes)):
        opcode = word >> 28
        commands.append(dict(pc=pc*4, word=f'{word:08x}', opcode=opcode))
        if opcode in (0, 4, 7, 9):
            continue  # Cold mathematical execution has no timing/cache state.
        if opcode == 15:
            if pc*4+4 != h['cb_bytes']:
                raise ValueError('END is not the last command')
        elif opcode == 1:
            code = (word >> 1) & 3
            if code == 3:
                raise ValueError('Unsupported local DMA length')
            length = 32 << code; destination = ((word >> 3) & 127)*32
            local(destination, length)
            scratch[destination:destination+length] = capture.read(((word >> 10) & 0x3ffff)*32, length)
        elif opcode == 2:
            address = ((word >> 12) & 65535)*128
            pages[word & 1] = (capture.read(address, 32), capture.read(address+128, (((word >> 1) & 7)+1)*128))
        elif opcode in (5, 6, 10):
            at = (word & 127)*32
            if opcode == 5:
                global_words = struct.unpack('<16I', local(at, 64))
                if not 0 < global_words[0] < 200*65536:
                    raise ValueError('Fixture far must be positive and below 200')
            elif opcode == 6:
                render = struct.unpack('<8I', local(at, 32))
                if not render[2] & 32 or render[2] & 16:
                    raise ValueError('DRAW oracle requires unlit and blending disabled')
                if not render[2] & 8 or render[2] & 7 not in (1, 7):
                    raise ValueError('Small-fixture oracle requires depth write with LESS or ALWAYS')
            else:
                value, = struct.unpack_from('<I', local(at, 32))
                if (value & 0x3fff)*512 != h['color_base'] or ((value >> 14) & 0x3fff)*512 != h['depth_base']:
                    raise ValueError('Target does not match frozen header')
                target = True
        elif opcode == 8:
            if not target:
                raise ValueError('CLEAR before target')
            if draws:
                raise ValueError('Small-fixture oracle requires CLEAR before every DRAW')
            role = 'depth' if word & 65536 else 'color'
            clear[role] = word & 65535
            planes[role] = [word & 65535]*96000
        elif opcode == 3:
            if not target or global_words is None or render is None or (word & 1) not in pages:
                raise ValueError('DRAW before complete bindings')
            if set(clear) != {'color', 'depth'}:
                raise ValueError('Small-fixture oracle needs both explicit plane clears')
            draws += 1
            meta, payload = pages[word & 1]
            q0, q1, q2, q3 = struct.unpack('<4Q', meta)
            vertices, count = q0 & 127, (q0 >> 8) & 127
            if not 1 <= vertices <= 64 or not 1 <= count <= 96:
                raise ValueError('Invalid meshlet counts')
            matrix = struct.unpack('<32i', local(((q0 >> 16) & 63)*128, 128))
            if matrix[21] != 65536:
                raise ValueError('Missing homogeneous constant')
            tint = [(matrix[22] >> shift) & 255 for shift in (0, 8, 16)]
            if (matrix[22] >> 24) & 255 != 255:
                raise ValueError('DRAW oracle requires opaque mesh uniform')
            origin = [signed(q2, 32), signed(q2 >> 32, 32), signed(q3, 32)]
            points = []; colors = []; constant = None
            for v in range(vertices):
                position, _, attr = struct.unpack_from('<3I', payload, v*12)
                p = [signed(origin[k]+(((position >> (10*k)) & 1023) << ((q3 >> 32) & 31)), 32) for k in range(3)]
                points.append(tuple(signed((sum(p[k]*matrix[4*j+k] for k in range(3))+(matrix[4*j+3] << 16)) >> 16, 32) for j in range(3)))
                if any(not -(1 << 25) <= c < (1 << 25) for c in points[-1]):
                    raise ValueError('Transformed position outside the production signed26 contract')
                colors.append(tuple((attr >> shift) & 255 for shift in (0, 8, 16)))
            if not q1 & 16:
                if render[0] & (1 << 20):
                    raise ValueError('Constant-texture fixture must not use mips')
                base = (render[0] & 65535)*128
                resources = [r for r in capture.ranges if r.address <= base < r.address+r.length]
                if not resources:
                    raise ValueError('Texture base was not captured')
                region = min(resources, key=lambda r: r.length)
                texture = capture.read(base, region.address+region.length-base)
                texels = [v[0] for v in struct.iter_unpack('<H', texture)]
                if not texels or any(t != texels[0] for t in texels):
                    raise ValueError('UV reference supports only a captured constant texture allocation')
                constant = expand565(texels[0])
                filtering = (render[0] >> 21) & 3
                size = (render[0] >> 16) & 15
                attributes = [struct.unpack_from('<I', payload, v*12+8)[0] for v in range(vertices)]
                if filtering > 2 or (filtering and not (size == 0 and texels[0] == 65535 and not any(attributes))):
                    raise ValueError('UV oracle needs nearest constant texture, or white 1x1 at zero UV')
            for t in range(count):
                indices, = struct.unpack_from('<Q', payload, ((q0 >> 24) & 127)*8+(t//3)*8)
                indices = (indices >> ((t % 3)*18)) & ((1 << 18)-1)
                index = [(indices >> shift) & 63 for shift in (0, 6, 12)]
                if max(index) >= vertices:
                    raise ValueError('Index outside vertex count')
                triangle([points[j] for j in index], [colors[j] for j in index], bool(q0 & (1 << 31)), tint, constant)
        else:
            raise ValueError('Unsupported oracle opcode '+str(opcode))
    if draws == 0:
        raise ValueError('No real DRAW command in fixture')
    caller = dict(commands=commands, draw_commands=draws, triangles=triangles, culled_triangles=culled,
                  drawn_pixels=drawn, drawn_pixel_count=len(drawn),
                  reference='Exact integer cross products and rational perspective; no hardware coefficient/reciprocal model')
    return Result(planes, stable, clear, caller)
