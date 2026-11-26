"""Pack a complete static scene and matching camera views with the native SDK."""

import argparse
import hashlib
import heapq
import io
import json
import math
import os
from pathlib import Path, PurePosixPath
import struct
import zipfile

from export_route import pack_route, read_json, write_json_atomic
import export_route
import route_ring

from gpu_mesh_layout import native_record
from mesh_partition import partition
from mesh_source import PositionEncoding, Vertex, encode_mesh
from native_asset_source import Sources

POLICIES = {"full": 0, "cull": 1, "lod": 2}
GROUP_WORDS = 13
CLUSTER_WORDS = 16
CLUSTER_GROUPS = 32
MAX_MESHLETS = 5460


def vector(values, length, name):
    if len(values) != length or any(type(v) not in (int, float) or not math.isfinite(v)
                                    for v in values):
        raise ValueError(f"{name} needs {length} finite numbers")
    return tuple(values)


def q16(value):
    raw = round(value * 65536)
    if not -(1 << 31) <= raw < 1 << 31:
        raise ValueError("value exceeds signed Q16.16")
    return raw & 65535, (raw >> 16) & 65535


def transform(point, origin, scale):
    return tuple((p - start) * scale for p, start in zip(point, origin))


def normalize(values):
    size = math.hypot(*values)
    if size < 1e-8:
        raise ValueError("degenerate camera basis")
    return tuple(v / size for v in values)


def cross(a, b):
    x, y, z = a
    u, v, w = b
    return y * w - z * v, z * u - x * w, x * v - y * u


def view_words(camera, origin, scale):
    eye = vector(camera["eye"], 3, "camera eye")
    target = vector(camera["target"], 3, "camera target")
    up = vector(camera.get("up", (0, 1, 0)), 3, "camera up")
    back = normalize(tuple(p - q for p, q in zip(eye, target)))
    right = normalize(cross(up, back))
    eye = transform(eye, origin, scale)
    return [word for axis in (right, cross(back, right), back)
            for value in (*axis, -sum(a * b for a, b in zip(axis, eye)))
            for word in q16(value)]


def word_table(name, words):
    lines = ["    " + ", ".join(map(str, words[i:i + 16])) + ","
             for i in range(0, len(words), 16)]
    body = "\n".join(lines)
    return f"pub static {name}: Buf<u16, {len(words)}> = Buf::new([\n{body}\n]);\n"


def faces_of(geometry, count):
    faces = geometry["triangles"]
    if not faces or any(len(face) != 3 or len(set(face)) != 3
                        or any(type(i) is not int or not 0 <= i < count for i in face)
                        for face in faces):
        raise ValueError("triangles need three distinct in-range indices")
    return faces


def read_geometry(geometry, textured, billboard, origin, scale, shift, exact_anchor=False):
    points, attributes, offsets = [], [], []
    step = (1 << shift) / 65536
    anchor = None
    for source in geometry["vertices"]:
        width = 6 if billboard or not textured else 5
        values = vector(source, width, "vertex")
        point = transform(values[:3], origin, scale)
        snapped = tuple(round(axis / step) * step for axis in point)
        if billboard and exact_anchor:
            # X/Z use the native Q16.16 origin; only Y uses the shared absolute grid.
            center = (values[0], values[2])
            if anchor is not None and center != anchor:
                raise ValueError("exact billboard anchor requires a common X/Z centerline")
            anchor = center
            snapped = (round(point[0] * 65536) / 65536, snapped[1],
                       round(point[2] * 65536) / 65536)
        for axis in snapped:
            q16(axis)
        points.append(snapped)
        attr = values[4:] if billboard else values[3:]
        if textured and any(not 0 <= v <= 1 for v in attr):
            raise ValueError("UV coordinates must be normalized")
        if not textured and any(type(v) is not int or not 0 <= v <= 255 for v in attr):
            raise ValueError("RGB components must be integer bytes")
        if billboard:
            offset = round(values[3] * scale * 512)
            if not -512 <= offset < 512:
                raise ValueError("billboard offset exceeds signed 10-bit range at render scale")
            offsets.append(offset)
        attributes.append(attr)
    faces = faces_of(geometry, len(points))
    parts = partition(points, faces) if len(points) > 64 or len(faces) > 96 else [(list(range(len(points))), faces)]
    records = []
    for ids, local_faces in parts:
        local = [points[i] for i in ids]
        origin_at = tuple(min(coordinates) for coordinates in zip(*local))
        if any(round((axis - base) / step) > 1023 for point in local
               for axis, base in zip(point, origin_at)):
            raise ValueError("meshlet exceeds delta10 span. Split the spatial group or increase position_shift")
        if billboard:
            packed = []
            for i, point in zip(ids, local):
                delta = [round((axis - base) / step) for axis, base in zip(point, origin_at)]
                u, v = (round(value * 65535) for value in attributes[i])
                packed.append([sum(value << (10*k) for k, value in enumerate(delta)),
                               offsets[i] & 1023, u | (v << 16)])
            records.append(native_record(dict(
                config=[*(round(v * 65536) for v in origin_at), shift],
                vertices=packed, triangles=local_faces, billboard=True, cull_negative=False,
            )))
        else:
            vertices = [Vertex(points[i], uv=attributes[i]) if textured
                        else Vertex(points[i], color=attributes[i]) for i in ids]
            payload, count, relocations = encode_mesh(
                vertices, local_faces, PositionEncoding(origin_at, shift),
            )
            assert count == 1
            records.append(payload[relocations[0][1]:])
    if billboard:
        # Horizontal facing varies per view. Enclose both endpoint directions.
        points = [(x + d, y, z + d) for (x, y, z), offset in zip(points, offsets)
                  for d in (-abs(offset) / 512, abs(offset) / 512)]
    return records, points, len(faces)


def mesh_asset(assets, records):
    if not 0 < len(records) <= MAX_MESHLETS:
        raise ValueError(f"native map needs 1..{MAX_MESHLETS} meshlets in one package")
    payload = bytearray((4 * len(records) + 127) & ~127)
    relocs = []
    for index, record in enumerate(records):
        relocs.append((index * 4, len(payload)))
        struct.pack_into("<HH", payload, index * 4, 0, len(record) // 128 - 1)
        payload.extend(record)
    assets.mesh("TRACK_MESH", payload, 0, len(records), relocs)


def read_materials(assets, package, materials):
    if not 0 < len(materials) <= 16384:
        raise ValueError("expected at least one material")
    words = []
    textures = {}
    texture_indices = {}
    for index, material in enumerate(materials):
        texture = material.get("texture")
        clamp = material.get("clamp", False)
        blend = material.get("blend", False)
        rgba = material.get("rgba", blend)
        tint = vector(material.get("tint", [255]*4), 4, "material tint")
        if any(type(v) is not int or not 0 <= v <= 255 for v in tint):
            raise ValueError("material tint components must be integer bytes")
        grid = material.get("atlas_grid_log2", 0)
        if type(grid) is not int or not 0 <= grid <= 3 or (grid and (not clamp or not material.get('mipmaps'))):
            raise ValueError("atlas grid requires a two-bit cell exponent, full mips and clamp")
        words.extend((int(texture is not None), int(clamp) | (grid << 1), tint[0] | tint[1] << 8,
                      tint[2] | tint[3] << 8))
        if texture is None:
            continue
        path = PurePosixPath(texture)
        if path.is_absolute() or ".." in path.parts or ":" in texture or "\\" in texture:
            raise ValueError("texture paths must be relative ZIP entries")
        mipmaps = material.get("mipmaps", False)
        encoding = material.get("encoding", "image")
        key = (texture, rgba, mipmaps, encoding, material.get("size"))
        symbol = textures.get(key)
        if symbol is None:
            symbol = f"MAP_TEXTURE_{index}"
            payload = package.read(texture)
            pixel_format = "Rgba4444" if rgba else "Rgb565"
            if encoding == "gpu-column16":
                size = material.get("size")
                if type(size) is not int:
                    raise ValueError("native texture size must be an integer")
                assets.texture(symbol, payload, size, pixel_format, mipmaps)
            elif encoding == "image":
                from PIL import Image
                with Image.open(io.BytesIO(payload)) as image:
                    if image.width != image.height:
                        raise ValueError("map textures must be square")
                    assets.texture_pixels(symbol, image.width, image.convert("RGBA").getdata(),
                                          pixel_format, mipmaps=mipmaps)
            else:
                raise ValueError("unsupported map texture encoding")
            textures[key] = symbol
            texture_indices[key] = len(texture_indices)
        words[index * 4] = texture_indices[key] + 1
    extra = word_table("MAP_MATERIALS", words)
    if textures:
        extra += f"pub static MAP_TEXTURES: Buf<TextureDesc, {len(textures)}> = Buf::new([\n"
        extra += ",\n".join(textures.values()) + "\n]);\n"
        extra += "pub fn map_texture(id: u16) -> Texture {\n"
        extra += "    Texture::from_native(MAP_TEXTURES[id])\n}\n"
    else:
        extra += "pub fn map_texture(id: u16) -> Texture { halt(0x7a11) }\n"
    return extra


def group_words(points, padding):
    axes = list(zip(*points))
    lo, hi = [min(axis) for axis in axes], [max(axis) for axis in axes]
    center = [(a + b) / 2 for a, b in zip(lo, hi)]
    extent = [(b - a) / 2 + padding for a, b in zip(lo, hi)]
    return [word for value in (*center, math.hypot(*extent), *extent) for word in q16(value)]


def cluster_groups(rows):
    """Keep native ranges and stable IDs; stream compact records by spatial cell."""
    cells = {}
    for stable_id, at in enumerate(range(0, len(rows), 20)):
        words = rows[at:at + 20]
        fixed = struct.unpack('<7i', struct.pack('<14H', *words[:14]))
        center = [round(value / 512) for value in fixed[:3]]
        if any(not -32768 <= value <= 32767 for value in center):
            raise ValueError('group center exceeds signed Q9.7 range')
        extent = [math.ceil((size + abs(value - snapped * 512)) / 512)
                  for value, snapped, size in zip(fixed[:3], center, fixed[4:])]
        radius = math.ceil((fixed[3] + math.hypot(*(value - snapped * 512
                           for value, snapped in zip(fixed[:3], center)))) / 512)
        if any(not 0 <= value <= 65535 for value in [*extent, radius]):
            raise ValueError('group extent exceeds unsigned Q9.7 range')
        if words[18] >= 32768 or words[19] not in (0, 1):
            raise ValueError('group material or billboard flag exceeds compact record')
        record = [*(value & 65535 for value in center), *extent, radius,
                  *words[14:18], words[18] | words[19] << 15, stable_id]
        cell = center[0] // 2048, center[2] // 2048  # 16 render units.
        cells.setdefault(cell, []).append((record, center, extent))
    packed, clusters, counts = [], [], []
    for cell in sorted(cells):
        entries = cells[cell]
        for start in range(0, len(entries), CLUSTER_GROUPS):
            selected = entries[start:start + CLUSTER_GROUPS]
            points = []
            for _, center, extent in selected:
                points.extend(tuple((c + sign * e) / 128 for c, e in zip(center, extent))
                              for sign in (-1, 1))
            clusters.extend((*group_words(points, 0), len(packed), len(selected)))
            counts.append(len(selected))
            for record, _, _ in selected:
                packed.extend(record)
    if len(packed) > 65535:
        raise ValueError('compact group table exceeds the 16-bit word offset range')
    return packed, clusters, counts



def ring_chunks(metadata, maximum):
    """Fixed-size ring chunks enclose both compact boxes and group spheres."""
    if maximum <= 0 or len(metadata) % route_ring.RING_WORDS:
        raise ValueError("invalid ring chunk layout")
    directory = []
    total = len(metadata) // route_ring.RING_WORDS
    for first in range(0, total, maximum):
        count = min(maximum, total - first)
        points, spheres = [], []
        for index in range(first, first + count):
            record = metadata[index * route_ring.RING_WORDS:(index + 1) * route_ring.RING_WORDS]
            center = [route_ring.signed(v) / 128 for v in record[:3]]
            extent = [v / 128 for v in record[3:6]]
            points.extend(tuple(c + sign * e for c, e in zip(center, extent)) for sign in (-1, 1))
            spheres.append((center, record[6] / 128))
        header = group_words(points, 0)
        fixed = struct.unpack('<7i', struct.pack('<14H', *header))
        center = [v / 65536 for v in fixed[:3]]
        radius = max(fixed[3] / 65536, *(math.dist(center, c) + r for c, r in spheres))
        header[6:8] = q16(math.ceil(radius * 65536) / 65536)
        directory.extend([*header, first, count])
    return directory


def opaque_spans(rows, opaque_count, meshlet_count):
    """Index immutable draw spans by their first meshlet, in submission order."""
    spans = [0] * (((meshlet_count + 15) // 16) * 16 * 3)
    owners = {}
    for identity in range(opaque_count):
        at = identity * 20
        for offset in (14, 16):
            first, count = rows[at + offset:at + offset + 2]
            if not count or first + count > meshlet_count:
                raise ValueError('opaque span exceeds the meshlet directory')
            record = [first, count, rows[at + 18]]
            previous = spans[first * 3:first * 3 + 3]
            if first in owners and (owners[first] != identity or previous != record):
                raise ValueError('opaque spans need unique first meshlets')
            owners[first] = identity
            spans[first * 3:first * 3 + 3] = record
    return spans


def prepared_ring(rows, opaque_count, ring, position_shift=10, strict=True):
    """Two matching command rings (fine/coarse), in authored route order."""
    order = list(range(opaque_count)) if ring is None else [
        ring[0][at + 12] for at in range(0, len(ring[0]), route_ring.RING_WORDS)
        if ring[0][at + 12] < opaque_count]
    if sorted(order) != list(range(opaque_count)):
        raise ValueError('opaque command ring must contain each opaque group once')
    # Reorder disjoint groups spatially. Potentially intersecting quantized
    # surfaces retain native order, including LessEqual depth ties. Fine draws
    # precede coarse draws at runtime, as in the native meshlet directory.
    margin = 2 * (1 << position_shift) + 256
    bounds = []
    for identity in range(opaque_count):
        values = struct.unpack('<7i', struct.pack('<14H', *rows[identity * 20:identity * 20 + 14]))
        bounds.append([(values[axis] - values[axis + 4] - margin,
                        values[axis] + values[axis + 4] + margin) for axis in range(3)])
    rank = {identity: ordinal for ordinal, identity in enumerate(order)}
    successors, incoming = [[] for _ in order], [0] * opaque_count
    for before in range(opaque_count):
        for after in range(before + 1, opaque_count):
            if all(a[0] <= b[1] and b[0] <= a[1]
                   for a, b in zip(bounds[before], bounds[after])):
                successors[before].append(after)
                incoming[after] += 1
    ready = [(rank[identity], identity) for identity in order if not incoming[identity]]
    heapq.heapify(ready)
    order = []
    while ready:
        _, identity = heapq.heappop(ready)
        order.append(identity)
        for after in successors[identity]:
            incoming[after] -= 1
            if not incoming[after]:
                heapq.heappush(ready, (rank[after], after))
    directory, index, starts = [], [0] * max(1, opaque_count), [0]
    commands = [0, 0]
    groups = 0
    for ordinal, identity in enumerate(order):
        at = identity * 20
        spans = rows[at + 14:at + 18]
        cost = [4 + 3 * spans[1], 4 + 3 * spans[3]]
        if strict and max(cost) > 1960:
            raise ValueError('one opaque group exceeds the prepared command segment budget')
        if groups == 256 or max(a + b for a, b in zip(commands, cost)) > 1960:
            starts.append(ordinal)
            commands, groups = [0, 0], 0
        page = len(starts) - 1
        directory.extend([*spans, rows[at + 18], identity, page])
        index[identity] = ordinal
        commands = [a + b for a, b in zip(commands, cost)]
        groups += 1
    starts.append(opaque_count)
    return directory or [0] * 7, index, starts


def build(output, route_path=None):
    recording = os.getenv('TANKRACER_RECORDING', 'reuse')
    if recording not in ('always', 'reuse', 'prepared'):
        raise ValueError('recording must be always, reuse or prepared')
    map_path = os.getenv("TANKRACER_MAP_INPUT")
    camera_path = os.getenv("TANKRACER_CAMERA_INPUT")
    if not map_path or not camera_path:
        raise ValueError("the static map build requires --map and --camera")
    map_data = Path(map_path).read_bytes()
    camera_data = Path(camera_path).read_bytes()
    route_path = route_path or os.getenv("TANKRACER_ROUTE_INPUT")
    route_input = read_json(route_path) if route_path else None
    assets = Sources(output)
    with zipfile.ZipFile(io.BytesIO(map_data)) as package:
        source = json.loads(package.read("scene.json"))
        if source["version"] != 1:
            raise ValueError("unsupported static scene version")
        materials = source["materials"]
        material_code = read_materials(assets, package, materials)
    origin = vector(source.get("origin", (0, 0, 0)), 3, "world origin")
    scale = vector([source.get("scale", 0.125)], 1, "render scale")[0]
    shift = source.get("position_shift", 10)
    billboard_shift = source.get("billboard_position_shift", shift)
    exact_anchor = source.get("billboard_exact_anchor", False)
    if type(billboard_shift) is not int or not 0 <= billboard_shift <= 31:
        raise ValueError("billboard_position_shift must be an integer in 0..31")
    if type(exact_anchor) is not bool:
        raise ValueError("billboard_exact_anchor must be a bool")
    far = vector([source.get("far", 110)], 1, "render far")[0]
    display = vector([source.get("display_distance", far)], 1, "display distance")[0]
    lod = vector([source.get("lod_distance", 20)], 1, "LOD distance")[0]
    if scale <= 0 or type(shift) is not int or not 0 <= shift <= 31 or not 0 < lod < far < 200:
        raise ValueError("expected positive scale, position_shift in 0..31 and 0 < lod_distance < far < 200")
    if not 0 < display <= far:
        raise ValueError('display_distance must be positive and within the projection far plane')
    policy = POLICIES[os.getenv("TANKRACER_POLICY", "lod")]
    records, coarse_records, coarse_ranges, rows, inventory = [], [], [], [], []
    opaque_count = 0
    queued = {False: [], True: []}
    for group in source["groups"]:
        material = group["material"]
        if type(material) is not int or not 0 <= material < len(materials):
            raise ValueError("group has an invalid material index")
        queued[bool(materials[material].get("blend", False))].append(group)
    for blended, groups in queued.items():
        if not blended:
            # Preserve original order among blended sort units.
            groups.sort(key=lambda group: group["material"])
            opaque_count = len(groups)
        for group in groups:
            mat = group["material"]
            textured = materials[mat].get("texture") is not None and not materials[mat].get('vertex_color', False)
            billboard = bool(group.get("billboard", False))
            if billboard and (not textured or not blended):
                raise ValueError("vertical billboard needs a textured blended material")
            group_shift = billboard_shift if billboard else shift
            fine, points, triangles = read_geometry(
                group, textured, billboard, origin, scale, group_shift, exact_anchor,
            )
            if group.get("bounds_geometry") is not None:
                _, original_bounds, _ = read_geometry(
                    group["bounds_geometry"], textured, billboard, origin, scale, group_shift, exact_anchor,
                )
                points.extend(original_bounds)
            first = len(records)
            records.extend(fine)
            coarse = None
            reduced_triangles = 0
            if policy == 2 and group.get("lod") is not None:
                coarse, more, reduced_triangles = read_geometry(
                    group["lod"], textured, billboard, origin, scale, group_shift, exact_anchor,
                )
                points.extend(more)
            low_first = first
            if coarse:
                low_first = len(coarse_records)
                coarse_ranges.append((len(rows) + 16, low_first))
                coarse_records.extend(coarse)
            row = group_words(points, (1 << group_shift) / 65536)
            rows.extend((*row, first, len(fine), low_first, len(coarse or fine), mat, int(billboard)))
            inventory.append(dict(name=group.get("name"), material=mat, billboard=billboard,
                                  fine_triangles=triangles, fine_meshlets=len(fine),
                                  reduced_triangles=reduced_triangles,
                                  reduced_meshlets=len(coarse or ())))
    # Keep fine material runs contiguous; switching LOD only selects another span.
    for at, first in coarse_ranges:
        rows[at] = len(records) + first
    records.extend(coarse_records)
    spans = opaque_spans(rows, opaque_count, len(records))
    assets.bytes("MAP_OPAQUE_SPANS", struct.pack(f"<{len(spans)}H", *spans))
    packed, clusters, cluster_counts = cluster_groups(rows)
    ring = None
    if route_input is not None:
        ring = route_ring.build_ring(route_input[0], packed, opaque_count, origin, scale, display,
                                     include_blended=True)
        assets.bytes("MAP_RING_GROUP_DATA", struct.pack(f"<{len(ring[0])}H", *ring[0]))
    # Match the spatial cluster count and keep the runtime chunk division a shift.
    chunk_groups = CLUSTER_GROUPS
    scratch_words = max(CLUSTER_GROUPS * GROUP_WORDS, chunk_groups * route_ring.RING_WORDS)
    chunk_directory = [0] if ring is None else ring_chunks(ring[0], chunk_groups)
    if ring is not None:
        ring[2].update(coarse_chunk_groups=chunk_groups,
                       coarse_chunk_count=len(chunk_directory) // CLUSTER_WORDS,
                       coarse_chunk_bytes=len(chunk_directory) * 2,
                       scope="all static candidate metadata; no command reordering")
    location_cells, location_lists, location = route_ring.build_location_grid(
        [] if ring is None else ring[1], scale, 0 if ring is None else ring[2]['camera_corridor'])
    if ring is not None:
        ring[2]['location_grid'] = location
    command_ring, command_index, command_pages = prepared_ring(
        rows, opaque_count, ring, shift, strict=recording == 'prepared')
    assets.bytes('MAP_OPAQUE_RING', struct.pack(f'<{len(command_ring)}H', *command_ring))
    mesh_asset(assets, records)
    assets.bytes("MAP_GROUP_DATA", struct.pack(f"<{len(packed)}H", *packed))
    cameras = json.loads(camera_data)
    if not isinstance(cameras, list) or not 1 <= len(cameras) <= 192:
        raise ValueError("expected 1..192 camera samples in the supplied sequence")
    camera_words = [w for camera in cameras for w in view_words(camera, origin, scale)]
    assets.bytes("MAP_CAMERA_DATA", struct.pack(f"<{len(camera_words)}H", *camera_words))
    extra = "use crate::dsl_rt::*;\n"
    candidates = os.getenv('TANKRACER_CANDIDATES', 'route')
    if candidates not in ('clusters', 'route'):
        raise ValueError('candidates must be clusters or route')
    safe_corridor = 0 if ring is None else max(0, ring[2]['camera_corridor'] - scale)
    constants = dict(
        TANKRACER_ROUTE_CANDIDATES=int(candidates == 'route'),
        RING_GROUP_WORDS=route_ring.RING_WORDS, RING_SEGMENT_WORDS=route_ring.SEGMENT_WORDS,
        RING_CHUNK_GROUPS=chunk_groups,
        ROUTE_LOCATION_GRID=int(location['enabled']),
        ROUTE_LOCATION_COLUMNS=location['columns'], ROUTE_LOCATION_ROWS=location['rows'],
        ROUTE_LOCATION_ORIGIN_X_LOW=location['origin_q16'][0] & 65535,
        ROUTE_LOCATION_ORIGIN_X_HIGH=(location['origin_q16'][0] >> 16) & 65535,
        ROUTE_LOCATION_ORIGIN_Z_LOW=location['origin_q16'][1] & 65535,
        ROUTE_LOCATION_ORIGIN_Z_HIGH=(location['origin_q16'][1] >> 16) & 65535,
        ROUTE_LOCATION_END_X_LOW=location['end_q16'][0] & 65535,
        ROUTE_LOCATION_END_X_HIGH=(location['end_q16'][0] >> 16) & 65535,
        ROUTE_LOCATION_END_Z_LOW=location['end_q16'][1] & 65535,
        ROUTE_LOCATION_END_Z_HIGH=(location['end_q16'][1] >> 16) & 65535,
        ROUTE_LOCATION_INVERSE_LOW=location['inverse_step_q16'] & 65535,
        ROUTE_LOCATION_INVERSE_HIGH=(location['inverse_step_q16'] >> 16) & 65535,
        MAP_RING_ACCEPT_LOW=q16(safe_corridor)[0], MAP_RING_ACCEPT_HIGH=q16(safe_corridor)[1],
        MAP_RING_ACCEPT_SQUARED_LOW=q16(safe_corridor * safe_corridor)[0],
        MAP_RING_ACCEPT_SQUARED_HIGH=q16(safe_corridor * safe_corridor)[1],
        TANKRACER_REUSE=int(recording == 'reuse'),
        TANKRACER_PREPARED=int(recording == 'prepared'),
        MAP_SHARED_OPAQUE_STATE=int(len({(m.get('texture'), m.get('clamp', False),
                                           m.get('rgba', m.get('blend', False)), m.get('mipmaps', False), m.get('atlas_grid_log2', 0))
                                          for m in materials if not m.get('blend', False)}) == 1),
        OPAQUE_MASK_WORDS=(max(1, (opaque_count * 2 + 15) // 16) if recording == 'prepared'
                           else (len(records) + 15) // 16),
        OPAQUE_DRAW_WORDS=5 if recording == 'prepared' else 3,
        MAP_PREPARED_RING_PAGES=len(command_pages) - 1,
        TANKRACER_POLICY=policy, GROUP_COUNT=len(inventory), GROUP_WORDS=GROUP_WORDS,
        GROUP_SCRATCH_WORDS=scratch_words, OPAQUE_COUNT=opaque_count,
        CLUSTER_COUNT=len(cluster_counts), CLUSTER_WORDS=CLUSTER_WORDS,
        CAMERA_COUNT=len(cameras),
        RING_SEGMENT_COUNT=0 if ring is None else ring[2]["segment_count"],
        RING_GROUP_COUNT=0 if ring is None else ring[2]["group_count"],
        MAP_FAR_LOW=q16(far)[0], MAP_FAR_HIGH=q16(far)[1],
        MAP_DISPLAY_LOW=q16(display)[0], MAP_DISPLAY_HIGH=q16(display)[1],
        MAP_LOD_LOW=q16(lod)[0], MAP_LOD_HIGH=q16(lod)[1],
    )
    for name, value in constants.items():
        extra += f"pub const {name}:u16={value};\n"
    extra += f"pub const MAP_PREPARED_PAGES:usize={(len(command_pages) - 1) * 2};\n"
    extra += word_table('MAP_OPAQUE_RING_INDEX', command_index)
    extra += word_table('MAP_OPAQUE_RING_PAGE', command_ring[6::7] if recording == 'prepared' else [0])
    extra += word_table('MAP_PREPARED_PAGE_FIRST', command_pages)
    extra += word_table("MAP_CLUSTERS", clusters)
    extra += word_table("MAP_RING_SEGMENTS", [0] if ring is None else ring[1])
    extra += word_table("MAP_RING_CHUNKS", chunk_directory)
    extra += word_table("MAP_ROUTE_LOCATION_CELLS", location_cells)
    extra += word_table("MAP_ROUTE_LOCATION_LISTS", location_lists)
    if ring is None:
        extra += "pub const MAP_RING_GROUP_DATA:AssetBytes=MAP_GROUP_DATA;\n"
    extra += material_code
    manifest = assets.write_manifest("tankracer_static.rs", sources=[__file__, export_route.__file__, route_ring.__file__], extra=extra)
    manifest.update(candidate_source=candidates, map_sha256=hashlib.sha256(map_data).hexdigest(),
                    camera_sha256=hashlib.sha256(camera_data).hexdigest(),
                    origin=origin, render_scale=scale, position_shift=shift, display_distance=display,
                    selection=dict(group_record_bytes=GROUP_WORDS * 2,
                                   group_asset_bytes=len(packed) * 2,
                                   cluster_count=len(cluster_counts),
                                   cluster_directory_bytes=len(clusters) * 2,
                                   group_scratch_bytes=scratch_words * 2,
                                   cluster_groups=cluster_counts, camera_asset_bytes=len(camera_words) * 2),
                    groups=inventory, camera_cases=[c.get("label", str(i))
                                                   for i, c in enumerate(cameras, 1)])
    if "billboard_position_shift" in source or "billboard_exact_anchor" in source:
        manifest["billboard_encoding"] = dict(position_shift=billboard_shift,
                                               exact_anchor=exact_anchor)
    if route_input is not None:
        route, route_hash = route_input
        result = write_json_atomic(output / "route.json", pack_route(route, origin, scale))
        ring_result = write_json_atomic(output / "route-ring.json", ring[2])
        manifest["route_ring"] = dict(file="route-ring.json", group_count=ring[2]["group_count"],
                                      max_candidates=ring[2]["max_candidates"], **ring_result)
        manifest["route"] = dict(file="route.json", input_sha256=route_hash,
                                 points=len(route["points"]), main_loop=len(route["main_loop"]), **result)
    write_json_atomic(output / "assets.json", manifest, indent=2)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--route", type=Path, help="optional authored tankracer.route.v1 JSON")
    try:
        args = parser.parse_args()
        build(args.output, args.route)
    except (ValueError, TypeError, KeyError, OSError, zipfile.BadZipFile) as error:
        parser.exit(1, f"static map: {error}\n")
