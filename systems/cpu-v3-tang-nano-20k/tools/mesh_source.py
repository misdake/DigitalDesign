"""Source-world vertices to native meshlets, directory and relocation placeholders."""
from dataclasses import dataclass
import math
import struct

from gpu_mesh_layout import native_record


@dataclass(frozen=True)
class Vertex:
    position: tuple
    normal: tuple = (0, 0, 1)
    uv: tuple = (0, 0)
    color: tuple | None = None


@dataclass(frozen=True)
class PositionEncoding:
    """Optional source-world origin and native binary step; default is automatic."""
    origin: tuple
    shift: int


def _finite(values, count, label):
    if len(values) != count or not all(math.isfinite(v) for v in values):
        raise ValueError(f'{label} needs {count} finite components')
    return values


def _fixed(position):
    raw = tuple(round(v * 65536) for v in _finite(position, 3, 'position'))
    if any(not -(1 << 31) <= v < (1 << 31) for v in raw):
        raise ValueError('position exceeds signed Q16.16 range')
    return raw


def _position_encoding(positions, encoding):
    if encoding is not None:
        if type(encoding.shift) is not int or not 0 <= encoding.shift <= 31:
            raise ValueError('position shift must be an integer in 0..31')
        return _fixed(encoding.origin), encoding.shift
    origin = tuple(min(p[axis] for p in positions) for axis in range(3))
    span = max(max(p[axis] for p in positions) - origin[axis] for axis in range(3))
    shift = max(0, span.bit_length() - 10)
    while round(span / (1 << shift)) > 1023:
        shift += 1
    return origin, shift


def _vertex(vertex, position, origin, shift, rgb):
    point = [round((v - start) / (1 << shift)) for v, start in zip(position, origin)]
    if any(not 0 <= v <= 1023 for v in point):
        raise ValueError('position does not fit the selected origin and shift')
    normal = _finite(vertex.normal, 3, 'normal')
    length = math.hypot(*normal)
    if length == 0:
        raise ValueError('normal must be nonzero')
    packed_normal = sum((round(v / length * 511) & 1023) << (10 * axis)
                        for axis, v in enumerate(normal))
    if rgb:
        color = _finite(vertex.color, 3, 'RGB color')
        if any(type(v) is not int or not 0 <= v <= 255 for v in color):
            raise ValueError('RGB color components must be integers in 0..255')
        attribute = sum(v << (8 * axis) for axis, v in enumerate(color))
    else:
        uv = _finite(vertex.uv, 2, 'UV')
        if any(not 0 <= v <= 1 for v in uv):
            raise ValueError('native normalized UV components must be in 0..1')
        attribute = round(uv[0] * 65535) | (round(uv[1] * 65535) << 16)
    return [sum(v << (10 * axis) for axis, v in enumerate(point)), packed_normal, attribute]


def _meshlets(vertices, triangles):
    # Preserve input order for a single record, including intentionally unused vertices.
    if len(vertices) <= 64 and len(triangles) <= 96:
        yield vertices, triangles
        return
    indices, local, faces = [], {}, []
    for triangle in triangles:
        additions = set(triangle) - local.keys()
        if faces and (len(local) + len(additions) > 64 or len(faces) == 96):
            yield [vertices[index] for index in indices], faces
            indices, local, faces = [], {}, []
        face = []
        for index in triangle:
            if index not in local:
                local[index] = len(indices)
                indices.append(index)
            face.append(local[index])
        faces.append(face)
    if faces:
        yield [vertices[index] for index in indices], faces


def encode_mesh(vertices, triangles, encoding=None):
    """Return payload, meshlet count and sector-pointer relocations in draw order."""
    vertices, triangles = list(vertices), [list(face) for face in triangles]
    if not vertices or not triangles:
        raise ValueError('mesh needs vertices and triangles')
    if any(len(face) != 3 or any(type(i) is not int or not 0 <= i < len(vertices) for i in face)
           for face in triangles):
        raise ValueError('triangles need three in-range vertex indices')
    rgb = vertices[0].color is not None
    if any((vertex.color is not None) != rgb for vertex in vertices):
        raise ValueError('all vertices must use the same RGB or UV attribute mode')
    records = []
    for points, faces in _meshlets(vertices, triangles):
        positions = [_fixed(vertex.position) for vertex in points]
        origin, shift = _position_encoding(positions, encoding)
        packed = [_vertex(vertex, position, origin, shift, rgb)
                  for vertex, position in zip(points, positions)]
        records.append(native_record(dict(vertices=packed, triangles=faces,
                                          config=[*origin, shift], color_mode=int(rgb))))
    if len(records) > 5460:
        raise ValueError('mesh exceeds the native directory capacity')
    payload = bytearray((len(records) * 4 + 127) // 128 * 128)
    relocations = []
    for index, record in enumerate(records):
        relocations.append((index * 4, len(payload)))
        struct.pack_into('<HH', payload, index * 4, 0, len(record) // 128 - 1)
        payload.extend(record)
    return payload, len(records), relocations
