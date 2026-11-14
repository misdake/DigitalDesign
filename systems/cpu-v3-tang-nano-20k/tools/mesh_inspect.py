"""Audit written native mesh bytes against source identity and canonical positions.

This decoder deliberately does not import the producer, partitioner or encoder.
"""
import math
import struct
from collections import Counter, defaultdict


def _signed32(value):
    return value - (1 << 32) if value & (1 << 31) else value


def _signature(vertex, rgb):
    length = math.hypot(*vertex.normal)
    normal = sum((round(v / length * 511) & 1023) << (10 * axis)
                 for axis, v in enumerate(vertex.normal))
    if rgb:
        attribute = sum(v << (8 * axis) for axis, v in enumerate(vertex.color))
    else:
        attribute = round(vertex.uv[0] * 65535) | (round(vertex.uv[1] * 65535) << 16)
    return normal, attribute


def inspect_mesh(payload, meshlet_count, vertices, triangles, canonical_positions=None, *, source_maps=None):
    """Decode unrelocated records, checking source identity, attributes and seams.

    canonical_positions maps original Q16 coordinate tuples to intended encoded
    positions. Without it, cross-record position/edge consistency is still audited.
    UV/hard-normal seam copies keep their own source attribute signatures.
    Optimized partitions supply source_maps and are checked as an oriented
    triangle multiset; omitting maps retains the original-order input contract.
    """
    def require(condition, message):
        if not condition:
            raise ValueError('native mesh audit: ' + message)

    vertices, triangles = list(vertices), list(triangles)
    require(0 < meshlet_count <= 5460, 'invalid directory count')
    offset = (meshlet_count * 4 + 127) // 128 * 128
    require(offset <= len(payload), 'truncated directory')
    source_positions = [tuple(round(v * 65536) for v in p.position) for p in vertices]
    def oriented(triangle):
        a, b, c = triangle
        return min((a,b,c), (b,c,a), (c,a,b))
    remaining = Counter(oriented(t) for t in triangles)
    if source_maps is not None:
        require(len(source_maps) == meshlet_count, 'source map count differs from directory')
    source_records, position_records, edge_records = defaultdict(set), defaultdict(set), defaultdict(set)
    decoded_positions, shifts = {}, Counter()
    triangle_at = 0
    for record in range(meshlet_count):
        pointer, sectors = struct.unpack_from('<HH', payload, record * 4)
        require(pointer == 0, 'expected unrelocated directory pointer')
        end = offset + (sectors + 1) * 128
        require(offset + 128 <= end <= len(payload), 'record extent exceeds payload')
        h0, h1, h2, h3 = struct.unpack_from('<4Q', payload, offset)
        require(h1 & ~16 == 0, 'unsupported source attribute flags')
        require(h3 >> 37 == 0, 'unsupported source normal mode or reserved metadata')
        nv, nt, triangle_offset = h0 & 127, (h0 >> 8) & 127, (h0 >> 24) & 127
        require(0 < nv <= 64 and 0 < nt <= 96, 'record counts outside native capacity')
        require(triangle_offset >= (3 * nv + 1) // 2, 'triangle area overlaps vertices')
        require(offset + 128 + (triangle_offset + (nt + 2) // 3) * 8 <= end, 'truncated triangle area')
        rgb = bool(h1 & 16)
        origin = (_signed32(h2 & 0xffffffff), _signed32(h2 >> 32), _signed32(h3 & 0xffffffff))
        shift = (h3 >> 32) & 31
        shifts[shift] += 1
        decoded = []
        for local in range(nv):
            p, n, a = struct.unpack_from('<3I', payload, offset + 128 + local * 12)
            require(p >> 30 == 0 and n >> 30 == 0, 'reserved vertex bits are nonzero')
            world = tuple(start + ((p >> (axis * 10)) & 1023) * (1 << shift)
                          for axis, start in enumerate(origin))
            require(all(-(1 << 31) <= v < (1 << 31) for v in world), 'decoded position outside Q16.16')
            decoded.append((world, n, a))
        mapping, reverse = {}, {}

        def check_vertex(local, source):
            require(0 <= local < nv and 0 <= source < len(vertices), 'invalid vertex reference')
            require(local not in mapping or mapping[local] == source, 'local/source identity mismatch')
            require(source not in reverse or reverse[source] == local, 'source has duplicate local IDs')
            if local in mapping:
                return
            mapping[local], reverse[source] = source, local
            vertex = vertices[source]
            require((vertex.color is not None) == rgb, 'RGB/UV mode differs from source')
            world, normal, attribute = decoded[local]
            require((normal, attribute) == _signature(vertex, rgb), 'normal/attribute differs from source')
            key = source_positions[source]
            if canonical_positions is not None:
                require(key in canonical_positions and world == canonical_positions[key], 'position differs from canonical')
            require(key not in decoded_positions or decoded_positions[key] == world, 'shared position mismatch')
            decoded_positions[key] = world
            source_records[source].add(record)
            position_records[key].add(record)

        if source_maps is not None:
            require(len(source_maps[record]) == nv, 'source map length differs from record')
            for local, source in enumerate(source_maps[record]):
                check_vertex(local, source)

        for triangle in range(nt):
            require(triangle_at < len(triangles), 'more triangles than source')
            word = struct.unpack_from('<Q', payload, offset + 128 + (triangle_offset + triangle // 3) * 8)[0]
            require(word >> 54 == 0, 'reserved triangle bits are nonzero')
            packed = (word >> (18 * (triangle % 3))) & 0x3ffff
            local_triangle = [(packed >> (lane * 6)) & 63 for lane in range(3)]
            require(all(i < nv for i in local_triangle), 'triangle index exceeds vertex count')
            source_triangle = ([mapping[i] for i in local_triangle] if source_maps is not None else triangles[triangle_at])
            require(len(source_triangle) == 3, 'invalid source triangle')
            for lane, source in enumerate(source_triangle):
                check_vertex(local_triangle[lane], source)
            key = oriented(source_triangle)
            require(remaining[key] > 0, 'triangle missing, duplicated or winding changed')
            remaining[key] -= 1
            keys = [source_positions[i] for i in source_triangle]
            for a, b in zip(keys, keys[1:] + keys[:1]):
                edge_records[tuple(sorted((a, b)))].add(record)
            triangle_at += 1
        if source_maps is None and meshlet_count == 1 and nv == len(vertices):
            # The producer's single-record contract preserves unused vertices.
            for local in range(nv):
                check_vertex(local, local)
        else:
            require(len(mapping) == nv, 'unreferenced record vertex has no source identity')
        offset = end
    require(triangle_at == len(triangles) and offset == len(payload), 'triangle count or payload tail mismatch')
    require(not any(remaining.values()), 'source triangle multiset was not preserved')
    return dict(meshlets=meshlet_count, triangles=triangle_at, shifts=dict(shifts),
                shared_source_vertices=sum(len(v) > 1 for v in source_records.values()),
                shared_position_groups=sum(len(v) > 1 for v in position_records.values()),
                shared_edges=sum(len(v) > 1 for v in edge_records.values()))
