"""Conservative XZ grid/direction candidates, independent of recorded cameras."""
import math
import struct

YAW_SECTORS = 32
BAKED_PITCH_DEGREES = 20
RUNTIME_PITCH_DEGREES = 19
BUDGET_BYTES = 2*1024*1024


def build_coarse(records, location, scale, cells, lists, directory):
    """Eight headings and one bit per 32-group read; share direct 8-byte masks."""
    global YAW_SECTORS
    original = YAW_SECTORS
    try:
        YAW_SECTORS = 8
        payload, info = build(records, location, scale, [lists[i] != 0 for i in cells])
        if not info['enabled']:
            raise ValueError('coarse Prepared selection needs an enabled route PVS')
        domains = []
        groups = len(records)//16
        segments_count = len(directory)//12
        for at in cells:
            segments = set(lists[at+1:at+1+lists[at]])
            segments |= {(i+1)%segments_count for i in segments}
            chunks = set()
            for segment in segments:
                start, count = directory[segment*12:segment*12+2]
                chunks |= {((start+i)%groups)//32 for i in range(count)}
            domains.append(chunks)
        masks, mask_ids, keys = [], {}, []
        chunks = (groups+31)//32
        if chunks > 64:
            raise ValueError('coarse direct PVS exceeds its 64-bit mask')
        for key in range(info['cells']*8):
            bits = int.from_bytes(struct.pack(f'<{info["mask_words"]}H', *decode(payload, info, key)), 'little')
            coarse = sum(1 << i for i in domains[key//8] if (bits >> (i*32)) & 0xffffffff)
            raw = coarse.to_bytes(8, 'little')
            if raw not in mask_ids:
                mask_ids[raw] = len(masks)
                masks.append(raw)
            keys.append(mask_ids[raw])
        headings, heading_ids, refs = [], {}, []
        for at in range(0, len(keys), 8):
            row = tuple(keys[at:at+8])
            if row not in heading_ids:
                heading_ids[row] = len(headings)
                headings.append(row)
            refs.append(heading_ids[row])
        if max(len(headings), len(masks)) > 65535:
            raise ValueError('coarse direct PVS reference overflow')
        data = bytearray(struct.pack(f'<{len(refs)}H', *refs))
        data.extend(bytes(-len(data)%128))
        heading_first = len(data)//128
        for row in headings:
            data.extend(struct.pack('<8H', *row))
        data.extend(bytes(-len(data)%128))
        root_first = len(data)//128
        data.extend(b''.join(masks))
        data.extend(bytes(-len(data)%128))
        return bytes(data), dict(enabled=True, mask_words=4, block_count=1,
            cells=info['cells'], yaw_sectors=8, bytes=len(data),
            heading_first_sector=heading_first, root_first_sector=root_first,
            root_sector_shift=5, block_first_sector=0, shared_heading_rows=len(headings),
            unique_masks=len(masks), encoding='direct-chunk-masks')
    finally:
        YAW_SECTORS = original


def pack(keys, masks, mask_words):
    """Share heading rows and native 16-byte mask blocks; no dependency chains."""
    if len(keys) % YAW_SECTORS:
        raise ValueError('PVS keys must contain complete heading rows')
    block_count = (mask_words + 7) // 8
    blocks = {bytes(16): 0, b'\xff' * 16: 1}
    roots = []
    for raw in masks:
        if len(raw) != mask_words * 2:
            raise ValueError('PVS mask width')
        row = raw.ljust(block_count * 16, b'\0')
        refs = []
        for at in range(0, len(row), 16):
            block = row[at:at + 16]
            if block not in blocks:
                blocks[block] = len(blocks)
            refs.append(blocks[block])
        roots.append(refs)
    # 0xffff is the runtime cache's invalid reference.
    if len(blocks) > 65535:
        return None
    heading_ids, heading_rows, cell_rows = {}, [], []
    for at in range(0, len(keys), YAW_SECTORS):
        row = tuple(keys[at:at + YAW_SECTORS])
        if row not in heading_ids:
            heading_ids[row] = len(heading_rows)
            heading_rows.append(row)
        cell_rows.append(heading_ids[row])
    # Pack 20-byte roots tightly on this map. Larger layouts increase the
    # address unit while retaining a single u16 reference and simple shifts.
    for unit in (4, 8, 16, 32, 64, 128):
        root_stride = (block_count * 2 + unit - 1) // unit * unit
        units_per_root = root_stride // unit
        if (len(roots) - 1) * units_per_root >= 65535:
            continue
        payload = bytearray(struct.pack(f'<{len(cell_rows)}H', *cell_rows))
        payload.extend(bytes(-len(payload) % 128))
        heading_first = len(payload) // 128
        for row in heading_rows:
            payload.extend(struct.pack(f'<{len(row)}H', *(ref * units_per_root for ref in row)))
        payload.extend(bytes(-len(payload) % 128))
        root_first = len(payload) // 128
        for refs in roots:
            payload.extend(struct.pack(f'<{len(refs)}H', *refs))
            payload.extend(bytes(root_stride - len(refs) * 2))
        payload.extend(bytes(-len(payload) % 128))
        block_first = len(payload) // 128
        native_blocks = [None] * len(blocks)
        for block, ref in blocks.items():
            native_blocks[ref] = block
        payload.extend(b''.join(native_blocks))
        payload.extend(bytes(-len(payload) % 128))
        if len(payload) > BUDGET_BYTES:
            return None
        return bytes(payload), dict(
            encoding='shared-blocks-v1', block_words=8, block_count=block_count,
            heading_first_sector=heading_first, root_first_sector=root_first,
            block_first_sector=block_first, root_sector_shift=7 - (unit.bit_length() - 1),
            root_unit_bytes=unit, root_stride_bytes=root_stride,
            shared_heading_rows=len(heading_rows), shared_blocks=len(blocks))
    return None


def decode(data, info, key):
    """Independent host traversal used by format and camera conservatism checks."""
    cell, yaw = divmod(key, info['yaw_sectors'])
    heading = struct.unpack_from('<H', data, cell * 2)[0]
    row = info['heading_first_sector'] * 128 + heading * info['yaw_sectors'] * 2
    root = struct.unpack_from('<H', data, row + yaw * 2)[0]
    if info.get('encoding') == 'direct-chunk-masks':
        return struct.unpack_from('<4H', data, info['root_first_sector'] * 128 + root * 8)
    offset = info['root_first_sector'] * 128 + root * info['root_unit_bytes']
    refs = struct.unpack_from(f"<{info['block_count']}H", data, offset)
    first = info['block_first_sector'] * 128
    raw = b''.join(data[first + ref * 16:first + (ref + 1) * 16] for ref in refs)
    return struct.unpack_from(f"<{info['mask_words']}H", raw)


def build(metadata, grid, scale, eligible_cells=None):
    count = len(metadata)//16
    mask_words = max(2, (count+31)//32*2)
    disabled = (bytes(128), dict(enabled=False, mask_words=mask_words,
                               root_sector_shift=5, block_count=(mask_words+7)//8,
                               heading_first_sector=0, root_first_sector=0, block_first_sector=0))
    if not grid['enabled'] or count == 0:
        return disabled
    columns, rows = grid['columns'], grid['rows']
    if columns*rows*YAW_SECTORS > 65535:
        return disabled
    # The circular cone encloses all four corners of the guarded CPU frustum
    # and is invariant under camera roll. Its XZ projection has this half-angle
    # for every pitch up to the baked bound, with one degree of heading slack.
    diagonal = math.atan(math.hypot(201/256, 121/256))
    half = math.asin(math.sin(diagonal)/math.cos(math.radians(BAKED_PITCH_DEGREES)))
    half += math.pi/YAW_SECTORS+math.radians(1)
    supports = []
    for yaw in range(YAW_SECTORS):
        angle = yaw*2*math.pi/YAW_SECTORS
        pair = []
        for side in (-1, 1):
            a = angle+side*(math.pi/2-half)
            nx, nz = -math.sin(a), -math.cos(a)
            maxima = []
            for at in range(0, len(metadata), 16):
                sx = metadata[at]-65536 if metadata[at] & 32768 else metadata[at]
                sz = metadata[at+2]-65536 if metadata[at+2] & 32768 else metadata[at+2]
                radius = metadata[at+6]/128
                # Legacy box screening uses independent projected extents. A
                # passing center can be brought into its guarded cone by a
                # displacement no larger than sqrt(3)*radius. The 2r square
                # therefore preserves those loose candidates as well as geometry.
                maxima.append(nx*sx/128+nz*sz/128+(abs(nx)+abs(nz))*2*radius)
            pair.append((nx, nz, maxima))
        supports.append(pair)
    keys, masks, interned = [], [], {}
    step = grid['step_q16']/65536
    padding = scale  # Same one-world-unit reconstruction guard as route acceptance.
    for z in range(rows):
        for x in range(columns):
            if eligible_cells is not None and not eligible_cells[z*columns+x]:
                raw = struct.pack(f'<{mask_words}H', *([65535]*mask_words))
                if raw not in interned:
                    interned[raw] = len(masks)
                    masks.append(raw)
                keys.extend([interned[raw]]*YAW_SECTORS)
                continue
            lo = [grid['origin_q16'][0]/65536+x*step-padding,
                  grid['origin_q16'][1]/65536+z*step-padding]
            hi = [lo[k]+step+2*padding for k in (0, 1)]
            for pair in supports:
                bounds = [sum(n*(low if n >= 0 else high) for n, low, high in zip((nx, nz), lo, hi)) for nx, nz, _ in pair]
                words = [0]*mask_words
                for i, (a, b) in enumerate(zip(pair[0][2], pair[1][2])):
                    if a >= bounds[0]-1e-8 and b >= bounds[1]-1e-8:
                        words[i >> 4] |= 1 << (i & 15)
                raw = struct.pack(f'<{mask_words}H', *words)
                if raw not in interned:
                    interned[raw] = len(masks)
                    masks.append(raw)
                keys.append(interned[raw])
    packed = pack(keys, masks, mask_words)
    if packed is None:
        return disabled
    payload, encoding = packed
    return payload, dict(enabled=True, mask_words=mask_words, **encoding,
                         bytes=len(payload), unique_masks=len(masks),
                         cells=columns*rows, yaw_sectors=YAW_SECTORS,
                         eligible_cells=sum(eligible_cells) if eligible_cells is not None else columns*rows,
                         baked_pitch_degrees=BAKED_PITCH_DEGREES, runtime_pitch_degrees=RUNTIME_PITCH_DEGREES,
                         projected_half_angle_degrees=math.degrees(half), camera_guard_world=1,
                         group_guard='2r XZ square preserves the existing independent-axis box test',
                         inputs='immutable compact bounds and route location grid; no camera fixture input')
