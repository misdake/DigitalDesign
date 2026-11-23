"""Conservative authored-route order and at most two static metadata spans."""
import math

from export_route import validate_route

RING_WORDS = 16
SEGMENT_WORDS = 12


def signed(word):
    return word - 65536 if word & 32768 else word


def fixed_words(value):
    raw = round(value * 65536)
    if not -(1 << 31) <= raw < 1 << 31:
        raise ValueError("route coefficient exceeds Q16.16")
    return [raw & 65535, (raw >> 16) & 65535]


def distance_squared(point, a, b):
    dx, dz = b[0] - a[0], b[1] - a[1]
    length = dx * dx + dz * dz
    if length == 0:
        raise ValueError("degenerate XZ route segment")
    t = max(0.0, min(1.0, ((point[0] - a[0]) * dx + (point[1] - a[1]) * dz) / length))
    return (point[0] - a[0] - t * dx) ** 2 + (point[1] - a[1] - t * dz) ** 2, t


def box_gap_squared(lo, hi, other_lo, other_hi):
    return sum(max(a - d, c - b, 0) ** 2 for a, b, c, d in zip(lo, hi, other_lo, other_hi))


def segment_box_distance_squared(a, b, lo, hi):
    """Exact XZ distance between a closed segment and a closed AABB."""
    near, far = 0.0, 1.0
    for axis in range(2):
        delta = b[axis] - a[axis]
        if delta == 0:
            if not lo[axis] <= a[axis] <= hi[axis]:
                break
        else:
            enter, leave = sorted(((lo[axis] - a[axis]) / delta,
                                   (hi[axis] - a[axis]) / delta))
            near, far = max(near, enter), min(far, leave)
            if near > far:
                break
    else:
        return 0.0
    corners = [(x, z) for x in (lo[0], hi[0]) for z in (lo[1], hi[1])]
    return min(box_gap_squared(a, a, lo, hi), box_gap_squared(b, b, lo, hi),
               *(distance_squared(corner, a, b)[0] for corner in corners))


def covering_span(required, total):
    """Remove the largest empty cyclic gap; deterministic ties keep lower start."""
    selected = sorted(set(required))
    if not selected:
        return 0, 0
    if selected[0] < 0 or selected[-1] >= total:
        raise ValueError("ring index outside directory")
    if len(selected) == total:
        return 0, total
    choices = []
    for index, current in enumerate(selected):
        following = selected[(index + 1) % len(selected)]
        gap = (following - current - 1) % total
        choices.append((-gap, following))
    negative_gap, start = min(choices)
    return start, total + negative_gap


def split_span(start, count, total):
    if not 0 <= start < total or not 0 <= count <= total:
        raise ValueError("invalid ring span")
    first = min(count, total - start)
    return [(start, first)] + ([(0, count - first)] if first < count else [])


def build_ring(route, packed, opaque_count, origin, scale, display_distance, corridor_world=24, include_blended=False):
    validate_route(route)
    if scale <= 0 or not math.isfinite(scale) or display_distance <= 0 or corridor_world < 0:
        raise ValueError("invalid route ring distance or scale")
    if not 0 < opaque_count <= len(packed) // 13 or len(packed) % 13:
        raise ValueError("invalid opaque compact-group table")
    points = {p["name"]: [(v - o) * scale for v, o in zip(p["position"], origin)] for p in route["points"]}
    circuit = [points[name] for name in route["main_loop"]]
    segments = []
    progress = 0.0
    for index, point in enumerate(circuit):
        other = circuit[(index + 1) % len(circuit)]
        a, b = [point[0], point[2]], [other[0], other[2]]
        length = math.dist(a, b)
        if length == 0:
            raise ValueError("degenerate XZ route segment")
        segments.append(dict(a=a, b=b, progress=progress, length=length,
                             lo=[min(x, y) for x, y in zip(a, b)],
                             hi=[max(x, y) for x, y in zip(a, b)]))
        progress += length
    groups = []
    seen = set()
    for at in range(0, len(packed), 13):
        record = packed[at:at + 13]
        identity = record[12]
        if identity in seen:
            raise ValueError("duplicate compact-group source ID")
        seen.add(identity)
        if identity >= opaque_count and not include_blended:
            continue
        center = [signed(record[axis]) / 128 for axis in [0, 2]]
        extent = [record[axis] / 128 for axis in [3, 5]]
        nearest = min(((*distance_squared(center, s["a"], s["b"]), i) for i, s in enumerate(segments)),
                      key=lambda item: (item[0], item[2]))
        _, t, index = nearest
        groups.append(dict(id=identity, packed_index=at // 13, record=record,
                           progress=segments[index]["progress"] + t * segments[index]["length"],
                           lo=[x - e for x, e in zip(center, extent)],
                           hi=[x + e for x, e in zip(center, extent)]))
    expected = len(packed) // 13 if include_blended else opaque_count
    if len(groups) != expected or {g["id"] for g in groups} != set(range(expected)):
        raise ValueError("selected source IDs do not cover the ring table")
    groups.sort(key=lambda g: (g["progress"], g["id"]))
    radius = display_distance + corridor_world * scale
    rows, directory = [], []
    for index, segment in enumerate(segments):
        required = [i for i, g in enumerate(groups)
                    if segment_box_distance_squared(segment["a"], segment["b"], g["lo"], g["hi"]) <= radius * radius]
        start, count = covering_span(required, len(groups))
        spans = split_span(start, count, len(groups))
        included = {i for first, length in spans for i in range(first, first + length)}
        if not set(required) <= included:
            raise ValueError("route interval omits a conservative candidate")
        a, b = segment["a"], segment["b"]
        length_squared = sum((x - y) ** 2 for x, y in zip(a, b))
        directory.extend([start, count, *fixed_words(a[0]), *fixed_words(a[1]),
                          *fixed_words(b[0] - a[0]), *fixed_words(b[1] - a[1]),
                          *fixed_words(1 / length_squared)])
        rows.append(dict(segment=index, start=start, count=count, spans=spans,
                         required_ids=[groups[i]["id"] for i in required],
                         segment_lo=segment["lo"], segment_hi=segment["hi"],
                         segment_start=segment["a"], segment_end=segment["b"]))
    metadata = [w for group in groups for w in [*group["record"], 0, 0, 0]]
    info = dict(format="tankracer.route-ring.v1", scope="static candidate metadata" if include_blended else "opaque metadata; no command reordering",
                route_start=route["start"], route_branch=0, group_words=RING_WORDS,
                segment_words=SEGMENT_WORDS, group_count=len(groups),
                segment_count=len(segments), display_distance=display_distance,
                camera_corridor=corridor_world * scale, coordinate_space="render",
                world_display_distance=display_distance / scale, world_camera_corridor=corridor_world,
                render_scale=scale, origin=list(origin),
                bound_metric="XZ distance from closed route segment to closed group AABB",
                bound_source="outward quantized union of fine, bounds_geometry and LOD positions",
                metadata_bytes=len(metadata) * 2, directory_bytes=len(directory) * 2,
                opaque_stable_id_range=[0, opaque_count],
                transparent_stable_id_range=[opaque_count, len(packed) // 13],
                lod_group_ids=[g["id"] for g in groups if g["record"][7:9] != g["record"][9:11]],
                order=[g["id"] for g in groups], packed_indices=[g["packed_index"] for g in groups],
                segments=rows, max_candidates=max(row["count"] for row in rows))
    return metadata, directory, info


def native_fixed(words, at):
    raw = words[at] | words[at + 1] << 16
    return raw - (1 << 32) if raw & (1 << 31) else raw


def native_segment_cell_within(a, b, lo, hi, radius):
    """Exact integer segment/AABB distance comparison in native Q16 units."""
    if any(all(low <= v <= high for v, low, high in zip(p, lo, hi)) for p in (a, b)):
        return True
    corners = [(lo[0], lo[1]), (hi[0], lo[1]), (hi[0], hi[1]), (lo[0], hi[1])]
    def cross(p, q, r):
        return (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
    for c, d in zip(corners, corners[1:] + corners[:1]):
        if all(max(min(a[k], b[k]), min(c[k], d[k])) <=
               min(max(a[k], b[k]), max(c[k], d[k])) for k in range(2)):
            if cross(a, b, c) * cross(a, b, d) <= 0 and cross(c, d, a) * cross(c, d, b) <= 0:
                return True
    squared = radius * radius
    for p in (a, b):
        if sum(max(low - v, v - high, 0) ** 2 for v, low, high in zip(p, lo, hi)) <= squared:
            return True
    dx, dz = b[0] - a[0], b[1] - a[1]
    length = dx * dx + dz * dz
    if length == 0:
        return False  # Point/AABB distance was handled above.
    for p in corners:
        rx, rz = p[0] - a[0], p[1] - a[1]
        projection = rx * dx + rz * dz
        if projection <= 0:
            distance = rx * rx + rz * rz
            if distance <= squared:
                return True
        elif projection >= length:
            if (p[0] - b[0]) ** 2 + (p[1] - b[1]) ** 2 <= squared:
                return True
        elif (rx * dz - rz * dx) ** 2 <= squared * length:
            return True
    return False


def build_location_grid(directory, scale, corridor):
    """Intern ascending native-segment lists; unsupported layouts retain a full scan."""
    if len(directory) % SEGMENT_WORDS or scale <= 0 or not math.isfinite(scale) or corridor < 0:
        raise ValueError("invalid native route location grid input")
    total = len(directory) // SEGMENT_WORDS
    fallback = dict(format="tankracer.route-location-grid.v1", enabled=False,
                    columns=0, rows=0, origin_q16=[0, 0], end_q16=[0, 0],
                    step_q16=65536, inverse_step_q16=65536, segment_count=total)
    def disabled(reason):
        return [0], [total, *range(total)], {**fallback, "fallback_reason": reason}
    if total == 0:
        return disabled("no route segments")
    exponent = round(math.log2(32 * scale))
    if not -14 <= exponent <= 14:
        return disabled("power-of-two render step or reciprocal outside signed Q16")
    step = 1 << (16 + exponent)
    radius = math.ceil(corridor * 65536)
    accepted = max(0, corridor - scale)
    limit = round(accepted * accepted * 65536)
    # Old DOTSTORE truncates squared distance; allow that plus <sqrt(2)
    # native units from the two projected-coordinate multiplies.
    if radius <= 2 or (radius - 2) ** 2 < (limit + 1) * 65536:
        return disabled("native acceptance quantization exceeds corridor margin")
    segments = []
    for at in range(0, len(directory), SEGMENT_WORDS):
        a = [native_fixed(directory, at + 2), native_fixed(directory, at + 4)]
        b = [a[0] + native_fixed(directory, at + 6), a[1] + native_fixed(directory, at + 8)]
        if any(not -(1 << 31) <= v < (1 << 31) for v in [*a, *b]):
            return disabled("native endpoint outside signed Q16")
        segments.append((a, b))
    origin = [((min(p[k] for a, b in segments for p in (a, b)) - radius) // step) * step
              for k in range(2)]
    end = [((max(p[k] for a, b in segments for p in (a, b)) + radius + step - 1) // step) * step
           for k in range(2)]
    if any(not -(1 << 31) <= v < (1 << 31) for v in origin + end):
        return disabled("grid bound outside signed Q16")
    if any(not 0 < high - low < (1 << 31) for low, high in zip(origin, end)):
        return disabled("grid difference outside signed Q16")
    columns, rows = [(high - low) // step for low, high in zip(origin, end)]
    if columns * rows > 2048 or total > 65535:
        return disabled("grid exceeds resident table or u16 indexing budget")
    cells, lists = [], [0]
    offsets = {(): 0}
    maximum = 0
    for z in range(rows):
        for x in range(columns):
            lo = [origin[0] + x * step, origin[1] + z * step]
            hi = [v + step for v in lo]
            ids = tuple(index for index, (a, b) in enumerate(segments)
                        if native_segment_cell_within(a, b, lo, hi, radius))
            maximum = max(maximum, len(ids))
            if ids not in offsets:
                offsets[ids] = len(lists)
                lists.extend([len(ids), *ids])
            cells.append(offsets[ids])
    if len(lists) > 65535 or (len(cells) + len(lists)) * 2 > 4096:
        return disabled("shared lists exceed resident table or u16 indexing budget")
    info = dict(format="tankracer.route-location-grid.v1", enabled=True,
                columns=columns, rows=rows, origin_q16=origin, end_q16=end,
                step_q16=step, inverse_step_q16=1 << (16 - exponent),
                cell_world=step / 65536 / scale, corridor_q16=radius,
                segment_count=total, unique_lists=len(offsets), max_segments=maximum,
                cell_table_bytes=len(cells) * 2, list_table_bytes=len(lists) * 2,
                endpoint_source="native quantized a and a+d from segment directory",
                bound_metric="exact integer closed segment to closed cell AABB distance")
    return cells, lists, info
