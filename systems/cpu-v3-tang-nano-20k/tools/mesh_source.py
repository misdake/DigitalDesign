"""Source-world vertices to native meshlets, directory and relocation placeholders."""
from dataclasses import dataclass
import math
import struct

from gpu_mesh_layout import native_record
from mesh_inspect import inspect_mesh
from mesh_partition import partition


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


def _explicit_grid(encoding):
    if type(encoding.shift) is not int or not 0 <= encoding.shift <= 31:
        raise ValueError('position shift must be an integer in 0..31')
    return _fixed(encoding.origin), encoding.shift


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


def _partitions(vertices, triangles):
    # Preserve input order for a single record, including intentionally unused vertices.
    if len(vertices) <= 64 and len(triangles) <= 96:
        return [(list(range(len(vertices))), triangles)]
    positions = [tuple(v / 65536 for v in _fixed(vertex.position)) for vertex in vertices]
    return partition(positions, triangles)


def _snap(position, anchor, shift):
    step = 1 << shift
    return tuple(start + round((v - start) / step) * step for v, start in zip(position, anchor))


def _fit_shift(positions, anchor, minimum=0, required=None):
    for shift in range(minimum, 32):
        rounded = [_snap(p, anchor, max(shift, required.get(p, 0) if required else 0)) for p in positions]
        if all(max(p[axis] for p in rounded) - min(p[axis] for p in rounded) <= 1023 * (1 << shift)
               for axis in range(3)):
            return shift
    raise ValueError('meshlet positions cannot fit a native quantization level')


def _position_grids(meshlets, encoding):
    groups = [[_fixed(v.position) for v in points] for points, _ in meshlets]
    if encoding is not None:
        for positions in groups:
            origin, shift = _explicit_grid(encoding)
            yield [_snap(p, origin, shift) for p in positions], origin, shift
        return
    # Equal source Q16 positions share a canonical value, even across UV seams.
    # Only their positions merge; normals and attributes retain source identity.
    anchor = tuple(min(p[axis] for group in groups for p in group) for axis in range(3))
    shifts = [_fit_shift(group, anchor) for group in groups]
    # Levels only increase. Recompute from source positions, never from an
    # already rounded value, so ties-to-even cannot accumulate double rounding.
    for _ in range(31 * len(groups) + 1):
        required = {}
        for group, shift in zip(groups, shifts):
            for p in group:
                required[p] = max(required.get(p, 0), shift)
        next_shifts = [_fit_shift(group, anchor, shift, required) for group, shift in zip(groups, shifts)]
        if shifts == next_shifts:
            break
        shifts = next_shifts
    else:
        raise ValueError('canonical position constraints did not converge')
    canonical = {p: _snap(p, anchor, shift) for p, shift in required.items()}
    if any(not -(1 << 31) <= v < (1 << 31) for p in canonical.values() for v in p):
        raise ValueError('quantized position exceeds signed Q16.16 range')
    for group, shift in zip(groups, shifts):
        positions = [canonical[p] for p in group]
        origin = tuple(min(p[axis] for p in positions) for axis in range(3))
        yield positions, origin, shift


def encode_mesh(vertices, triangles, encoding=None, *, audit=None):
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
    records, canonical = [], {}
    parts = _partitions(vertices, triangles)
    source_maps = [indices for indices, _ in parts]
    meshlets = [([vertices[index] for index in indices], faces) for indices, faces in parts]
    for (points, faces), (positions, origin, shift) in zip(meshlets, _position_grids(meshlets, encoding)):
        packed = [_vertex(vertex, position, origin, shift, rgb)
                  for vertex, position in zip(points, positions)]
        canonical.update((_fixed(v.position), p) for v, p in zip(points, positions))
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
    report = inspect_mesh(payload, len(records), vertices, triangles, canonical, source_maps=source_maps)
    if audit is not None:
        audit.update(report, source_maps=source_maps)
    return payload, len(records), relocations
