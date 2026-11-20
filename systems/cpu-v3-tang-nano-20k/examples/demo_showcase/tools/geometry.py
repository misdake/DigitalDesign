"""Gallery meshes using the shared native encoders; no physical addresses."""
from native_asset_source import Vertex, PositionEncoding
from gpu_texture_layout import pack_levels
from dataclasses import replace
import math


def add_geometry(assets):
    # Face-local vertices preserve hard normals and UV seams.
    faces = [
        ((0, 0, 511), [(0,0,512),(512,0,512),(512,512,512),(0,512,512)]),
        ((0, 0, -511), [(512,0,0),(0,0,0),(0,512,0),(512,512,0)]),
        ((511, 0, 0), [(512,0,512),(512,0,0),(512,512,0),(512,512,512)]),
        ((-511, 0, 0), [(0,0,0),(0,0,512),(0,512,512),(0,512,0)]),
        ((0, 511, 0), [(0,512,512),(512,512,512),(512,512,0),(0,512,0)]),
        ((0, -511, 0), [(0,0,0),(512,0,0),(512,0,512),(0,0,512)]),
    ]
    vertices, triangles = [], []
    for normal, corners in faces:
        first = len(vertices)
        for (x,y,z), (u,v) in zip(corners, [(0,0),(65535,0),(65535,65535),(0,65535)]):
            vertices.append(Vertex(tuple(value / 256 - 1 for value in (x,y,z)),
                                   normal=normal, uv=(u / 65535,v / 65535)))
        triangles.extend([[first,first+1,first+2],[first,first+2,first+3]])
    assets.geometry('GEOMETRY_CUBE', vertices, triangles)
    add_rgb_geometry(assets, 'LAB_RGB_CUBE', vertices, triangles)
    levels = {}
    for exponent in range(5):
        side = 1 << exponent
        step = max(0, exponent-2)
        levels[exponent] = [0xffb0 if ((x >> step) ^ (y >> step)) & 1 else 0x2d9f
                            for y in range(side) for x in range(side)]
    assets.texture('GEOMETRY_CHECKER', pack_levels(levels), 16, 'Rgb565', mipmaps=True)
    corners = [(0,0),(512,0),(512,512),(0,512)]
    uvs = [(0,0),(65535,0),(65535,65535),(0,65535)]
    colors = [0x4040ff,0x40ff40,0xff4040,0xffffff]
    for symbol, rgb in [('LAB_PLANE', False), ('LAB_RGB_PLANE', True)]:
        vertices = []
        for i, ((x,y),(u,v)) in enumerate(zip(corners, uvs)):
            color = tuple((colors[i] >> (8 * axis)) & 255 for axis in range(3)) if rgb else None
            vertices.append(Vertex((x / 256 - 1,y / 256 - 1,0), uv=(u / 65535,v / 65535), color=color))
        assets.geometry(symbol, vertices, [[0,1,2],[0,2,3]])
    add_light_arrow(assets)
    add_curved_geometry(assets)
    add_axes(assets)
    assets.texture_pixels('LAB_WHITE', 1, [(255, 255, 255, 255)])


def add_rgb_geometry(assets, symbol, vertices, triangles):
    # Position-based RGB is independent of the analytic shading normals.
    colored = [replace(v, color=tuple(round(96 + 159 * (p + 1) / 2)
                                      for p in v.position)) for v in vertices]
    assets.geometry(symbol, colored, triangles)


def add_curved_geometry(assets):
    # Duplicate UV seams; omit collapsed pole triangles. All normals are analytic.
    for symbol, rings, sectors in [('SPHERE', 16, 32), ('TORUS', 12, 32)]:
        vertices, triangles = [], []
        for j in range(rings + 1):
            v = j / rings
            for i in range(sectors + 1):
                u = i / sectors
                a = u * math.tau
                if symbol == 'SPHERE':
                    b = v * math.pi
                    normal = (math.sin(b) * math.cos(a), math.cos(b),
                              math.sin(b) * math.sin(a))
                    point = normal
                else:
                    b = v * math.tau
                    radius = 0.65 + 0.3 * math.cos(b)
                    point = (radius * math.cos(a), radius * math.sin(a), 0.3 * math.sin(b))
                    normal = (math.cos(b) * math.cos(a), math.cos(b) * math.sin(a), math.sin(b))
                vertices.append(Vertex(point, normal=normal, uv=(u, v)))
        for j in range(rings):
            for i in range(sectors):
                a = j * (sectors + 1) + i
                b = a + sectors + 1
                if symbol != 'SPHERE' or j != 0:
                    triangles.append([a, a + 1, b])
                if symbol != 'SPHERE' or j != rings - 1:
                    triangles.append([a + 1, b + 1, b])
        assets.geometry('LAB_' + symbol, vertices, triangles)
        add_rgb_geometry(assets, 'LAB_RGB_' + symbol, vertices, triangles)


def add_axes(assets):
    vertices, triangles = [], []
    for axis, color in enumerate([(255, 80, 80), (80, 255, 112), (96, 160, 255)]):
        first = len(vertices)
        # A square shaft and a wider pyramidal tip, pointing along the positive axis.
        points = [(x, y, z) for z in (0, 0.58)
                  for x, y in [(-0.018,-0.018),(0.018,-0.018),(0.018,0.018),(-0.018,0.018)]]
        points += [(-0.06,-0.06,0.58),(0.06,-0.06,0.58),
                   (0.06,0.06,0.58),(-0.06,0.06,0.58),(0,0,0.78)]
        for x, y, z in points:
            position = (z, x, y) if axis == 0 else (y, z, x) if axis == 1 else (x, y, z)
            vertices.append(Vertex(position, color=color))
        faces = [[0,2,1],[0,3,2],[0,1,5],[0,5,4],[1,2,6],[1,6,5],
                 [2,3,7],[2,7,6],[3,0,4],[3,4,7],
                 [8,9,12],[9,10,12],[10,11,12],[11,8,12],[8,10,9],[8,11,10]]
        triangles.extend([[first + k for k in face] for face in faces])
    assets.geometry('LAB_AXES', vertices, triangles)


def add_light_arrow(assets):
    # A small +Z arrow: cyan shaft and gold pyramid, independent of lighting.
    points = [(x, y, z) for z in (0, 352)
              for x, y in ((112,112), (144,112), (144,144), (112,144))]
    points += [(64,64,352), (192,64,352), (192,192,352), (64,192,352), (128,128,512)]
    vertices = [Vertex(((x-128) / 512, (y-128) / 512, z / 512),
                       color=(128,255,255) if i < 8 else (255,208,64))
                for i,(x,y,z) in enumerate(points)]
    triangles = [[0,2,1], [0,3,2], [4,5,6], [4,6,7],
                 [0,1,5], [0,5,4], [1,2,6], [1,6,5],
                 [2,3,7], [2,7,6], [3,0,4], [3,4,7],
                 [8,9,12], [9,10,12], [10,11,12], [11,8,12], [8,10,9], [8,11,10]]
    assets.geometry('LAB_LIGHT_ARROW', vertices, triangles,
                    encoding=PositionEncoding(origin=(-0.25,-0.25,0), shift=7))
