"""Portable shared Bloxorz geometry, fixed cameras and the retained low atlas."""
import math
import colorsys

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


def conservative_far(level):
    # Whole-block release: centre-to-pivot <=sqrt(1.25), |spin|<8.727,
    # |velocity component|<9.76. Fall notifies at600ms (tick <=50ms), then
    # a320ms fade reaches the presented reset; total fall time is <1 second.
    # Bounds include failed off-board rolls, all rotated extents, entry/goal
    # drops and wells. Extra vertical headroom also bounds the linear velocity
    # term without relying on gravity reducing the upper extremum.
    bounds=[(-level['width']/2-14,level['width']/2+14),(-60,14),
            (-level['depth']/2-14,level['depth']/2+14)]
    cam=camera(level['width'],level['depth'])
    back=[cam['eye'][i]-cam['target'][i] for i in range(3)]
    length=math.sqrt(sum(v*v for v in back));back=[v/length for v in back]
    maximum=sum((cam['eye'][i]-bounds[i][0 if back[i]>=0 else 1])*back[i] for i in range(3))
    return math.ceil(maximum)+4,bounds


def uv_rect(which):
    if isinstance(which, int):
        return ((which % 2) * .25 + 16 / 1024,
                .5 + (which // 2) * .25 + 16 / 1024, 224 / 1024, 224 / 1024)
    return {'side-a': (.5 + 16 / 1024, .5 + 16 / 1024, 480 / 1024, 480 / 1024),
            'top': (16 / 1024, 16 / 1024, 480 / 1024, 480 / 1024),
            'side-b': (.5 + 16 / 1024, 16 / 1024, 480 / 1024, 480 / 1024)}[which]


def mesh(name, quads, cull_negative=False):
    # Native unsigned delta10, Q16 positions at 1/128 source-unit resolution.
    step = 512
    positions = [[round(v * 65536 / step) * step for v in point]
                 for points, _, _ in quads for point in points]
    origins = [min(p[i] for p in positions) for i in range(3)]
    vertices, triangles = [], []
    cursor = 0
    for points, normal, rect in quads:
        if cull_negative:
            a=[points[1][i]-points[0][i] for i in range(3)]
            b=[points[2][i]-points[0][i] for i in range(3)]
            cross=[a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]]
            assert sum(cross[i]*normal[i] for i in range(3))<0
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
                config=origins + [9 | 256], color_mode=0, cull_negative=cull_negative)


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




def unit_floor_face(face, variant):
    # The same per-tile three visible quad definitions as floor(), plus the
    # opposite sides for exact per-camera visibility on larger boards.
    faces = [
        ([(-1,0,-1),(1,0,-1),(1,0,1),(-1,0,1)], (0,511,0)),
        ([(1,0,1),(1,0,-1),(1,-.4,-1),(1,-.4,1)], (511,0,0)),
        ([(-1,0,1),(1,0,1),(1,-.4,1),(-1,-.4,1)], (0,0,511)),
        ([(-1,0,-1),(-1,0,1),(-1,-.4,1),(-1,-.4,-1)], (-511,0,0)),
        ([(1,0,-1),(-1,0,-1),(-1,-.4,-1),(1,-.4,-1)], (0,0,-511))]
    points, normal = faces[face]
    return mesh(f'shared-floor-{variant}-{face}', [(points,normal,uv_rect(variant))])

def visible_floor_faces(level):
    width,depth=level['width'],level['depth'];eye=camera(width,depth)['eye']
    def fixed(x,z):
        return 0<=x<width and 0<=z<depth and level['cells'][z*width+x][0] in (1,7) and [x,z]!=level['start'][:2]
    result=[]
    for i,cell in enumerate(level['cells']):
        x,z=i%width,i//width
        if not fixed(x,z):continue
        variant=(x*3+z*5)%4
        faces=[0]
        # Dynamic bridges, switches and open goal/start cells never suppress a
        # static boundary face. A 1/Q16 margin conservatively keeps edge-on faces.
        for face,nx,nz,visible in [(1,x+1,z,eye[0]>=x+1-width/2-1/65536),(2,x,z+1,eye[2]>=z+1-depth/2-1/65536),(3,x-1,z,eye[0]<=x-width/2+1/65536),(4,x,z-1,eye[2]<=z-depth/2+1/65536)]:
            if not fixed(nx,nz) and visible:faces.append(face)
        result.extend(x|(z<<5)|(face<<10)|(variant<<13) for face in faces)
    return result


def unit_tile(variant,untextured=False):
    parts=[unit_floor_face(f,variant) for f in range(5)]
    # Build the same five quads as the shared faces in a single native meshlet.
    vertices=[];triangles=[]
    origin=[min(p['config'][i] for p in parts) for i in range(3)]
    for part in parts:
        first=len(vertices)
        for pos,normal,uv in part['vertices']:
            values=[((pos>>(10*i))&1023)+(part['config'][i]-origin[i])//512 for i in range(3)]
            vertices.append([sum(v<<(10*i) for i,v in enumerate(values)),normal,0xffffff if untextured else uv])
        triangles.extend([[first+t for t in tri] for tri in part['triangles']])
    return dict(name=f'unit-tile-{variant}',group='unit-tile',vertices=vertices,triangles=triangles,config=origin+[9|256],color_mode=int(untextured),cull_negative=False)




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
    return mesh('block', quads, cull_negative=True)


def plain(name, quads):
    result = mesh(name, quads, cull_negative=True)
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
    # A cavity owns its interior, not overlapping closed boxes and hidden caps.
    a=.47;top=-.005;bottom=-2.08;rect=uv_rect(0)
    quads=[
        ([(-a,bottom,-a),(a,bottom,-a),(a,bottom,a),(-a,bottom,a)],(0,511,0),rect),
        ([(-a,top,-a),(a,top,-a),(a,bottom,-a),(-a,bottom,-a)],(0,0,511),rect),
        ([(a,top,a),(-a,top,a),(-a,bottom,a),(a,bottom,a)],(0,0,-511),rect),
        ([(-a,top,a),(-a,top,-a),(-a,bottom,-a),(-a,bottom,a)],(511,0,0),rect),
        ([(a,top,-a),(a,top,a),(a,bottom,a),(a,bottom,-a)],(-511,0,0),rect),
    ]
    return [plain('well',quads)]


def heavy_base():
    return plain('heavy-base', box_quads((0, 0, 0), (.44, .05, .44)))




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
