"""Gallery meshes using the shared native encoders; no physical addresses."""
from native_asset_source import Vertex, PositionEncoding
from gpu_texture_layout import pack_levels


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
