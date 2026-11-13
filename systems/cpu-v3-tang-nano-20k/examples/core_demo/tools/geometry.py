"""Small gallery cube using the shared native encoders; no physical addresses."""
import struct
from gpu_mesh_layout import native_record
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
        packed_normal = sum((n & 1023) << (10*i) for i, n in enumerate(normal))
        for (x,y,z), (u,v) in zip(corners, [(0,0),(65535,0),(65535,65535),(0,65535)]):
            vertices.append([x | (y << 10) | (z << 20), packed_normal, u | (v << 16)])
        triangles.extend([[first,first+1,first+2],[first,first+2,first+3]])
    record = native_record(dict(vertices=vertices, triangles=triangles, config=[-65536]*3+[8], color_mode=0))
    payload = bytearray(128)+record
    struct.pack_into('<H', payload, 2, len(record)//128-1)
    assets.mesh('GEOMETRY_CUBE', payload, 0, 1, [(0,128)])
    levels = {}
    for exponent in range(5):
        side = 1 << exponent
        step = max(0, exponent-2)
        levels[exponent] = [0xffb0 if ((x >> step) ^ (y >> step)) & 1 else 0x2d9f
                            for y in range(side) for x in range(side)]
    assets.texture('GEOMETRY_CHECKER', pack_levels(levels), 16, 'Rgb565', mipmaps=True)
