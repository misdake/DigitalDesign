"""Conservative XZ grid/direction candidates, independent of recorded cameras."""
import math
import struct

YAW_SECTORS = 32
BAKED_PITCH_DEGREES = 20
RUNTIME_PITCH_DEGREES = 19
BUDGET_BYTES = 2*1024*1024


def build(metadata, grid, scale, eligible_cells=None):
    count = len(metadata)//16
    mask_words = max(2, (count+31)//32*2)
    stride_sectors = (mask_words+63)//64
    disabled = (bytes(128), dict(enabled=False, mask_words=mask_words, stride_sectors=1, first_mask_sector=0))
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
                    masks.append(raw.ljust(stride_sectors*128, b'\0'))
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
                    masks.append(raw.ljust(stride_sectors*128, b'\0'))
                keys.append(interned[raw])
    header_sectors = (len(keys)*2+127)//128
    payload = struct.pack(f'<{len(keys)}H', *keys).ljust(header_sectors*128, b'\0')+b''.join(masks)
    if len(payload) > BUDGET_BYTES:
        return disabled
    return payload, dict(enabled=True, mask_words=mask_words, stride_sectors=stride_sectors,
                         first_mask_sector=header_sectors, bytes=len(payload), unique_masks=len(masks),
                         cells=columns*rows, yaw_sectors=YAW_SECTORS,
                         eligible_cells=sum(eligible_cells) if eligible_cells is not None else columns*rows,
                         baked_pitch_degrees=BAKED_PITCH_DEGREES, runtime_pitch_degrees=RUNTIME_PITCH_DEGREES,
                         projected_half_angle_degrees=math.degrees(half), camera_guard_world=1,
                         group_guard='2r XZ square preserves the existing independent-axis box test',
                         inputs='immutable compact bounds and route location grid; no camera fixture input')
