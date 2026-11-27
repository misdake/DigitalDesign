"""Conservative cell/yaw admission as bounded, directly executable marked ranges."""
import heapq
from itertools import groupby
import math
import struct


# With these budgets, the ENGLAND route uses five pages at three groups per
# unit (three opaque, two cutout). Some offline plans touch all five pages, so
# range-limit 4 fails and 6 and 8 fit. At four groups per unit, the coarse-sphere
# route layout used four pages.
MAX_GROUPS_PER_PREPARED_UNIT = 3

# The asset packer estimates usage ahead of target recording. Each page has
# 8 KiB for 2048 32-bit commands, the maximum span of a CP list segment.
# Leave 168 commands of headroom for the real encoding and alignment.
PREPARED_PAGE_COMMAND_BUDGET = 1880
# The recorder reserves 511 128-byte push slots per page.
PREPARED_PAGE_PUSH_BUDGET = 500


def cover(required, pages, weights, limit):
    """Keep the most expensive gaps; never merge across physical pages."""
    intervals, gaps = [], []
    for page in range(max(pages, default=-1) + 1):
        ids = [i for i in required if pages[i] == page]
        if not ids:
            continue
        first = prior = ids[0]
        for item in ids[1:]:
            if item != prior + 1:
                gaps.append((sum(weights[prior+1:item]), prior+1, item))
            prior = item
        intervals.append((first, prior+1))
    if len(intervals) > limit:
        raise ValueError('physical page count exceeds the range budget')
    cuts = {(a,b) for _,a,b in sorted(gaps, reverse=True)[:limit-len(intervals)]}
    result = []
    for first,end in intervals:
        for a,b in sorted(cuts):
            if first < a < end:
                result.append((first,a))
                first = b
        result.append((first,end))
    assert len(result) <= limit
    assert set(required) <= {i for a,b in result for i in range(a,b)}
    return tuple(result)


def plan_ranges(required, units, pages, weights, limit):
    if not required:
        return (65535,)
    seed = required[0]
    # The dynamic prefix draws the first group. A singleton consumes no link.
    if units[seed]['count'] == 1:
        required = required[1:]
    return (seed,) + tuple(v for span in cover(required, pages, weights, limit) for v in span)


def order_groups(rows, identities, priority, shift):
    """Preserve intersecting native-order surfaces, including billboard envelopes."""
    rank = {identity:i for i,identity in enumerate(priority)}
    bounds = {}
    margin = 2 * (1 << shift) / 65536 + 1/256
    for identity in identities:
        row = rows[identity*18:identity*18+18]
        values = struct.unpack('<7i',struct.pack('<14H',*row[:14]))
        center = [v/65536 for v in values[:3]]
        extent = [v/65536 for v in values[4:7]]
        if row[17]:
            extent[0] = extent[2] = values[3]/65536
        bounds[identity] = [(c-e-margin,c+e+margin) for c,e in zip(center,extent)]
    successors = {i:[] for i in identities}
    incoming = {i:0 for i in identities}
    for at,before in enumerate(identities):
        for after in identities[at+1:]:
            if all(a[0]<=b[1] and b[0]<=a[1] for a,b in zip(bounds[before],bounds[after])):
                successors[before].append(after)
                incoming[after] += 1
    ready = [(rank[i],i) for i in identities if not incoming[i]]
    heapq.heapify(ready)
    result = []
    while ready:
        _,identity = heapq.heappop(ready)
        result.append(identity)
        for after in successors[identity]:
            incoming[after] -= 1
            if not incoming[after]:
                heapq.heappush(ready,(rank[after],after))
    return result


def build(rows, packed, inventory, opaque_count, materials, ring, location,
          eligible, scale, shift, display, limit, layout='route', cell_world=16,
          layout_only=False, shared_cutouts=False):
    if (limit not in ((30,) if layout_only else (4,6,8))
            or layout not in ('route','lanes','stable')):
        raise ValueError('unsupported bounded range configuration')
    block = MAX_GROUPS_PER_PREPARED_UNIT
    if block <= 0:
        raise ValueError('MAX_GROUPS_PER_PREPARED_UNIT must be positive')
    if any(materials[rows[i*18+16]*4+1]&8 == 0 for i in range(opaque_count,len(inventory))):
        raise ValueError('bounded static ranges require fixed-order cutouts; use ordinary selection for SrcOver')
    compact = {packed[at+10]:packed[at:at+11] for at in range(0,len(packed),11)}
    progress = {identity:i for i,identity in enumerate(ring[2]['order'])}
    side = {}
    for identity in range(len(inventory)):
        record = compact[identity]
        point = [(v-65536 if v&32768 else v)/128 for v in (record[0],record[2])]
        best = None
        for segment in ring[2]['segments']:
            a,b = segment['segment_start'],segment['segment_end']
            dx,dz = b[0]-a[0],b[1]-a[1]
            t = max(0,min(1,((point[0]-a[0])*dx+(point[1]-a[1])*dz)/(dx*dx+dz*dz)))
            distance = (point[0]-a[0]-t*dx)**2+(point[1]-a[1]-t*dz)**2
            lateral = (dx*(point[1]-a[1])-dz*(point[0]-a[0]))/math.hypot(dx,dz)
            if best is None or distance<best[0]:
                best = distance,lateral
        side[identity] = 0 if best[1]<-2 else 2 if best[1]>2 else 1
    opaque = list(range(opaque_count))
    opaque_priority = sorted(opaque,key=lambda i:((side[i],) if layout=='lanes' else ())+ (progress[i],i))
    opaque = order_groups(rows,opaque,opaque_priority,shift)
    cutout = sorted(range(opaque_count,len(inventory)),key=lambda i:(materials[rows[i*18+16]*4],i))
    if layout!='stable':
        priority = sorted(cutout,key=lambda i:(materials[rows[i*18+16]*4],)+
            ((side[i],) if layout=='lanes' else ())+(progress[i],i))
        cutout = order_groups(rows,cutout,priority,shift)
    # Filtered alpha edges are order-sensitive even for disjoint surfaces.
    # Spatial candidates deliberately accept that approximation; stable is the
    # full-order control. No runtime ordering or repair is introduced.
    shared_opaque = bool(opaque) and len({tuple(materials[rows[i*18+16]*4:rows[i*18+16]*4+2]) for i in opaque})==1
    def signature(identity):
        row = rows[identity*18:identity*18+18]
        return tuple(materials[row[16]*4:row[16]*4+4])+(row[17],)
    def blocks(identities):
        return [identities[at:at+block] for at in range(0,len(identities),block)]
    cutout_blocks = blocks(cutout)
    cutout_streams = [[(chunk, index) for index, chunk in enumerate(cutout_blocks)]]
    if shared_cutouts:
        # A unit that straddles two material groups needs two command ranges.
        # Keep both fragments tied to the original visibility decision.
        fragments = [(list(part), index) for index, chunk in enumerate(cutout_blocks)
                     for _, part in groupby(chunk, lambda i: signature(i)[:2])]
        cutout_streams = [list(parts) for _, parts in
                          groupby(fragments, lambda part: signature(part[0][0])[:2])]
    streams = [[(chunk, -1) for chunk in blocks(opaque)], *cutout_streams]
    units,draws,pages = [],[],[]
    current_page = 0
    used = uploads = 0
    fragments_by_block = {}
    for group,chunks in enumerate(streams):
        kind = int(group != 0)
        if kind and chunks and used and (shared_opaque or shared_cutouts):
            current_page += 1
            used = uploads = 0
        for chunk, block_index in chunks:
            meshlets = sum(rows[i*18+15] for i in chunk)
            changes = sum(signature(a)!=signature(b) for a,b in zip(chunk,chunk[1:]))
            render_changes = sum(signature(a)[:2]!=signature(b)[:2] for a,b in zip(chunk,chunk[1:]))
            mode_changes = sum(signature(a)[-1]!=signature(b)[-1] for a,b in zip(chunk,chunk[1:]))
            cost = (4 if shared_opaque and not kind else 8)+3*meshlets+2*changes+6*render_changes+2*mode_changes
            push = 2+changes
            if cost > PREPARED_PAGE_COMMAND_BUDGET or push > PREPARED_PAGE_PUSH_BUDGET:
                raise ValueError('one marked unit exceeds page capacity; reduce MAX_GROUPS_PER_PREPARED_UNIT or partition its groups')
            if (used + cost > PREPARED_PAGE_COMMAND_BUDGET
                    or uploads + push > PREPARED_PAGE_PUSH_BUDGET):
                current_page += 1
                used = uploads = 0
            first = len(draws)//4
            for identity in chunk:
                row = rows[identity*18:identity*18+18]
                draws.extend((row[14],row[15],row[16]|(row[17]<<15),identity))
            if kind and shared_cutouts:
                fragments_by_block.setdefault(block_index, []).append(len(units))
            units.append(dict(first=first,count=len(chunk),page=current_page,kind=kind,group=group,ids=chunk,
                              weight=sum(inventory[i]['triangles']+8*inventory[i]['meshlets'] for i in chunk)))
            used += cost
            uploads += push
    pages = [u['page'] for u in units]
    if max(pages)+1>8:
        raise ValueError('layout exceeds the eight-page static owner array')
    if layout_only:
        # One dynamic segment per active group and one final segment.
        limit = min(limit, 31-len(streams)) if shared_cutouts else limit
        if max(pages)+1 > limit:
            raise ValueError('physical page count exceeds the dynamic range budget')
        return bytes(128), dict(bytes=128, limit=limit, layout=layout, block=block,
            columns=1, rows=1, origin_q16=[0,0], inverse_q16=65536,
            heading_first_sector=0, plan_first_sector=0, plan_words=64,
            pages=max(pages)+1, units=len(units), packed_ranges=len(units)<256,
            shared_opaque=shared_opaque, material_groups=len(streams),
            split_blocks=[ids for ids in fragments_by_block.values() if len(ids)>1],
            selection='coarse-sphere-shared' if shared_cutouts else 'coarse-sphere'), units, draws
    step = cell_world*scale
    old_step = location['step_q16']/65536
    columns = math.ceil((location['end_q16'][0]-location['origin_q16'][0])/65536/step)
    rows_count = math.ceil((location['end_q16'][1]-location['origin_q16'][1])/65536/step)
    yaw_count = 32
    if columns*rows_count*yaw_count>65535:
        raise ValueError('range key exceeds u16')
    origin = [v/65536 for v in location['origin_q16']]
    half = math.asin(math.sin(math.atan(math.hypot(201/256,121/256)))/math.cos(math.radians(20)))
    half += math.pi/yaw_count+math.radians(1)
    support = []
    diagonal_slope = math.hypot(201/256,121/256)
    deviation = math.acos(math.cos(math.radians(20))*math.cos(math.pi/yaw_count+math.radians(1)))
    far_factor = math.cos(deviation)+diagonal_slope*math.sin(deviation)+1e-5
    for yaw in range(yaw_count):
        pair = []
        for sign in (-1,1):
            a = yaw*2*math.pi/yaw_count+sign*(math.pi/2-half)
            nx,nz = -math.sin(a),-math.cos(a)
            pair.append((nx,nz,[nx*((r[0]-65536 if r[0]&32768 else r[0])/128)+
                nz*((r[2]-65536 if r[2]&32768 else r[2])/128)+
                (abs(nx)+abs(nz))*2*r[6]/128 for r in compact.values()]))
        # Project the old box test's far envelope onto the bin's horizontal
        # axis. Eliminating its vertical coordinate permits arbitrary camera
        # height/roll without the very loose omnidirectional radius alone.
        nx,nz = math.sin(yaw*2*math.pi/yaw_count),math.cos(yaw*2*math.pi/yaw_count)
        far = []
        for r in compact.values():
            radius = max(r[6]/128,math.hypot(*(v/128 for v in r[3:6])))+1/64
            extent = radius*(far_factor+(diagonal_slope+math.sqrt(2))*math.sin(deviation))
            far.append(nx*((r[0]-65536 if r[0]&32768 else r[0])/128)+
                       nz*((r[2]-65536 if r[2]&32768 else r[2])/128)+display*far_factor+extent)
        pair.append((nx,nz,far))
        support.append(pair)
    ids = list(compact)
    plans = [()]
    plan_ids = {():0}
    keys = []
    active = 0
    weights = [u['weight'] for u in units]
    for z in range(rows_count):
        for x in range(columns):
            old_x = min(location['columns']-1,int(x*step/old_step))
            old_z = min(location['rows']-1,int(z*step/old_step))
            if not eligible[old_z*location['columns']+old_x]:
                keys.extend([65535]*yaw_count)
                continue
            active += 1
            lo = [origin[0]+x*step-scale,origin[1]+z*step-scale]
            hi = [v+step+2*scale for v in lo]
            nearby = set()
            for identity,r in compact.items():
                center = [(v-65536 if v&32768 else v)/128 for v in (r[0],r[2])]
                distance = sum(max(a-c,0,c-b)**2 for c,a,b in zip(center,lo,hi))
                # Also preserve the old independent-axis box test's loose
                # candidates, not only points inside the geometric far plane.
                radius = display*math.sqrt(1+(201/256)**2+(121/256)**2)+4*r[6]/128
                if distance<=radius*radius:
                    nearby.add(identity)
            for pair in support:
                bounds = [nx*(lo[0] if nx>=0 else hi[0])+nz*(lo[1] if nz>=0 else hi[1]) for nx,nz,_ in pair]
                visible = {identity for j,identity in enumerate(ids) if identity in nearby and
                           all(plane[2][j]>=bound-1e-8 for plane,bound in zip(pair,bounds))}
                required = [i for i,u in enumerate(units) if any(identity in visible for identity in u['ids'])]
                plan = plan_ranges(required, units, pages, weights, limit)
                if plan not in plan_ids:
                    plan_ids[plan] = len(plans)
                    plans.append(plan)
                keys.append(plan_ids[plan])
    if len(plans)>65535:
        raise ValueError('too many range plans')
    headings,heading_ids,cells = [],{},[]
    for at in range(0,len(keys),yaw_count):
        row = tuple(keys[at:at+yaw_count])
        if row not in heading_ids:
            heading_ids[row] = len(headings)
            headings.append(row)
        cells.append(heading_ids[row])
    payload = bytearray(struct.pack(f'<{len(cells)}H',*cells))
    payload.extend(bytes(-len(payload)%128))
    heading_first = len(payload)//128
    for heading in headings:
        payload.extend(struct.pack('<32H',*heading))
    payload.extend(bytes(-len(payload)%128))
    plan_first = len(payload)//128
    packed_ranges = len(units)<256
    required_words = 2+limit*(1 if packed_ranges else 2)
    plan_words = 1 << (required_words-1).bit_length()
    for plan in plans:
        endpoints = list(plan[1:])
        if packed_ranges:
            endpoints = [a|(b<<8) for a,b in zip(endpoints[::2],endpoints[1::2])]
        data = [len(plan)//2,plan[0] if plan else 65535,*endpoints]
        payload.extend(struct.pack(f'<{plan_words}H',*data,*([0]*(plan_words-len(data)))))
    payload.extend(bytes(-len(payload)%128))
    info = dict(bytes=len(payload),limit=limit,layout=layout,block=block,cell_world=cell_world,
        columns=columns,rows=rows_count,origin_q16=location['origin_q16'],step_q16=round(step*65536),
        inverse_q16=round(65536/step),active_cells=active,yaw_sectors=yaw_count,
        heading_first_sector=heading_first,plan_first_sector=plan_first,plan_words=plan_words,
        plans=len(plans),heading_rows=len(headings),pages=max(pages)+1,units=len(units),packed_ranges=packed_ranges,
        shared_opaque=shared_opaque,
        scope='whole-cell/heading conservative XZ cone and distance envelope; no recorded camera input')
    return bytes(payload),info,units,draws


def validate_cameras(words, info, data):
    """Reject unsupported static playback before producing an executable."""
    if len(words)%24:
        raise ValueError('invalid range camera matrix layout')
    inverse = info['inverse_q16']
    checked = 0
    for at in range(0,len(words),24):
        values = struct.unpack('<12i',struct.pack('<24H',*words[at:at+24]))
        rows = [values[i:i+4] for i in (0,4,8)]
        if abs(rows[2][1])>21336:
            raise ValueError('camera pitch outside bounded plans; use --range-limit 0')
        t = [r[3] for r in rows]
        position = [-(sum(r[axis]*v for r,v in zip(rows,t))>>16) for axis in (0,2)]
        cell = [((p-o)*inverse)>>32 for p,o in zip(position,info['origin_q16'])]
        if not 0<=cell[0]<info['columns'] or not 0<=cell[1]<info['rows']:
            raise ValueError('camera outside bounded grid; use --range-limit 0')
        index = cell[1]*info['columns']+cell[0]
        heading_row = struct.unpack_from('<H',data,index*2)[0]
        refs = struct.unpack_from('<32H',data,info['heading_first_sector']*128+heading_row*64)
        if 65535 in refs:
            raise ValueError('camera outside bounded plan domain; use --range-limit 0')
        checked += 1
    return checked
