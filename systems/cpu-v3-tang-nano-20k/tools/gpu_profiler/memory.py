"""Actual payload accounting at the three 64-bit GPU master interfaces."""
from collections import defaultdict, deque
import math

LINKS = ('ro', 'fb_read', 'fb_write')
SOURCES = ('unknown', 'command-fetch', 'meshlet-dma', 'local-dma', 'texture', 'framebuffer-cache', 'clear')
WINDOW = 256


def overlap(a, b, start, end):
    return max(0, min(b, end) - max(a, start))


def union_length(spans):
    result, tail = 0, None
    for a, b in sorted(spans):
        if b > a:
            result += max(0, b - max(a, tail if tail is not None else a))
            tail = max(b, tail if tail is not None else b)
    return result


def peak(beats, start, end, clock):
    if end - start < WINDOW:
        return None
    window, total, maximum = deque(), 0, 0
    for b in beats:
        while window and window[0]['cycle'] < b['cycle'] - WINDOW + 1:
            total -= window.popleft()['wire_bytes']
        window.append(b)
        total += b['wire_bytes']
        maximum = max(maximum, total)
    return maximum * clock / WINDOW


def summarize(transactions, beats, start, end, clock, ports=(0, 1, 2), tracks=None):
    """Counts refer to this half-open interval, including straddling transactions."""
    tx = [t for t in transactions if t['port'] in ports]
    data = [b for b in beats if b['port'] in ports and start <= b['cycle'] < end]
    duration = end - start
    if duration <= 0:
        raise ValueError('Memory interval needs positive duration')
    wire = sum(b['wire_bytes'] for b in data)
    effective = sum(b['effective_bytes'] for b in data)
    reads = [b for b in data if b['port'] != 2]
    writes = [b for b in data if b['port'] == 2]
    waits, response_waits = 0, 0
    for port in ports:
        live = [(max(start, t['start']), min(end, t['end'] if t['end'] is not None else end)) for t in tx if t['port'] == port]
        active = union_length(live)
        slots = len({b['cycle'] for b in data if b['port'] == port})
        waits += active - slots
        if port == 2:
            response_waits += union_length([(max(start, t['last_data'] + 1), min(end, t['end'] if t['end'] is not None else end))
                                            for t in tx if t['port'] == port and t['last_data'] is not None and t['beats'] == t['expected_beats']])
    stalls = None
    if tracks is not None:
        stalls = 0
        for port in ports:
            changes = tracks.get(('ro_wait', 'fb_read_wait', 'fb_write_wait')[port], [])
            for i, (a, value) in enumerate(changes):
                b = changes[i + 1][0] if i + 1 < len(changes) else end
                if value:
                    stalls += overlap(a, b, start, end)
    return dict(start=start, end=end, cycles=duration,
                issued_transactions=sum(start <= t['start'] < end for t in tx),
                completed_transactions=sum(t['end'] is not None and start < t['end'] <= end for t in tx),
                failed_transactions=sum(t['error'] and t['end'] is not None and start < t['end'] <= end for t in tx),
                data_beats=len(data), read_wire_bytes=sum(b['wire_bytes'] for b in reads),
                write_wire_bytes=sum(b['wire_bytes'] for b in writes), wire_bytes=wire,
                effective_bytes=effective, average_wire_bytes_per_second=wire * clock / duration,
                average_read_bytes_per_second=sum(b['wire_bytes'] for b in reads) * clock / duration,
                average_write_bytes_per_second=sum(b['wire_bytes'] for b in writes) * clock / duration,
                average_effective_bytes_per_second=effective * clock / duration,
                peak_wire_bytes_per_second=peak(data, start, end, clock), peak_window_cycles=WINDOW,
                peak_read_bytes_per_second=peak(reads, start, end, clock),
                peak_write_bytes_per_second=peak(writes, start, end, clock),
                link_capacity_bytes_per_cycle=8 * len(ports), utilization=wire / (8 * len(ports) * duration),
                outstanding_without_data_link_cycles=waits,
                write_terminal_wait_link_cycles=response_waits, request_backpressure_link_cycles=stalls)


def temporal_data(transactions, beats, end, tracks, sample_interval, data_observed):
    """Exact event bytes and sampled states; never allocate burst bytes over time."""
    if not data_observed:
        return dict(available=False, reason='No MEM_DATA events saved; exact transfer timing is unavailable')
    slots = defaultdict(lambda: [0, 0])
    payload = set()
    changes = defaultdict(lambda: [0, 0, 0, 0])
    boundaries = {0, end}
    for beat in beats:
        t, port = beat['cycle'], beat['port']
        slots[t][int(port == 2)] += beat['wire_bytes']
        payload.add((t, port))
        boundaries.update((t, min(t + 1, end)))
    for tx in transactions:
        changes[tx['start']][tx['port']] += 1
        boundaries.add(tx['start'])
        if tx['end'] is not None:
            changes[tx['end']][tx['port']] -= 1
            boundaries.add(tx['end'])
        if tx['port'] == 2 and tx['last_data'] is not None and tx['beats'] == tx['expected_beats']:
            a = tx['last_data'] + 1
            changes[a][3] += 1
            boundaries.add(a)
            if tx['end'] is not None:
                changes[tx['end']][3] -= 1
    states = dict(outstanding=[], waiting=[], terminal_wait=[])
    live = [0, 0, 0, 0]
    for t in sorted(boundaries):
        live = [a + b for a, b in zip(live, changes[t])]
        if t >= end:
            continue
        values = dict(outstanding=sum(live[:3]),
                      waiting=sum(live[p] > 0 and (t, p) not in payload for p in range(3)),
                      terminal_wait=int(live[3] > 0))
        for key, value in values.items():
            if not states[key] or states[key][-1][1] != value:
                states[key].append([t, value])
    backpressure = dict(available=False, reason='Request valid/ready samples are not saved')
    names = ('ro_wait', 'fb_read_wait', 'fb_write_wait')
    if sample_interval is not None and all(tracks.get(k) for k in names):
        a, b = max(0, sample_interval[0]), min(end, sample_interval[1])
        if a < b:
            delta = defaultdict(int)
            for name in names:
                before = 0
                for t, value in tracks[name]:
                    delta[t] += value - before
                    before = value
            value, points = 0, []
            for t in sorted(set(delta) | {a}):
                value += delta[t]
                if a <= t < b and (not points or points[-1][1] != value):
                    points.append([t, value])
            backpressure = dict(available=True, start=a, end=b, track=points)
    return dict(available=True, version=1, start=0, end=end,
                data=[[t, *values] for t, values in sorted(slots.items())],
                states=states, backpressure=backpressure)


def build(events, origin, stop, clock, complete, commands, tracks, sample_interval=None):
    if not events.get('MEM_REQUEST'):
        return dict(available=False, reason='Capture has no master-interface events')
    timeline = []
    for name, priority in [('MEM_COMPLETE', 0), ('MEM_REQUEST', 1), ('MEM_DATA', 2)]:
        timeline += [(row[0], priority, name, row[1:]) for row in events.get(name, [])]
    live = [deque() for _ in LINKS]
    data_owners = [deque() for _ in LINKS]
    transactions, beats, seen = [], [], set()
    for cycle, _, name, args in sorted(timeline):
        t = cycle - origin
        port = args[0]
        if port not in range(3):
            raise ValueError('Unknown GPU master')
        key = (name, cycle, port)
        if key in seen:
            raise ValueError('Duplicate master event in one clock slot')
        seen.add(key)
        if name == 'MEM_REQUEST':
            _, address, count, source = args
            if count not in (0, 1, 3) or source not in range(len(SOURCES)) or not 0 <= t < stop-origin:
                raise ValueError('Invalid master request')
            item = dict(id=len(transactions)+1, port=port, source=SOURCES[source], address_halfwords=address,
                        expected_beats=4*(count+1), start=t, end=None, last_data=None, beats=0, error=False)
            transactions.append(item)
            live[port].append(item)
            data_owners[port].append(item)
        elif name == 'MEM_DATA':
            _, mask, error = args
            if not data_owners[port] or not 0 <= mask <= 255 or error not in (0, 1) or not 0 <= t < stop-origin:
                raise ValueError('Unowned/invalid master data beat')
            owner = data_owners[port][0]
            owner['beats'] += 1
            owner['last_data'] = t
            beats.append(dict(cycle=t, port=port, source=owner['source'], transaction=owner['id'],
                              wire_bytes=8, effective_bytes=0 if error else mask.bit_count()))
            if owner['beats'] == owner['expected_beats'] or error:
                data_owners[port].popleft()
        else:
            _, error = args
            if not live[port] or error not in (0, 1) or not 0 < t <= stop-origin:
                raise ValueError('Unowned/invalid master completion')
            owner = live[port].popleft()
            if not error and owner['beats'] != owner['expected_beats']:
                raise ValueError('Master LAST without all payload beats')
            owner.update(end=t, error=bool(error))
            if data_owners[port] and data_owners[port][0] is owner:
                data_owners[port].popleft()
    if complete and any(live):
        raise ValueError('Complete capture contains outstanding memory requests')
    end = stop-origin
    def scope(start, finish):
        summary = summarize(transactions, beats, start, finish, clock, tracks=tracks)
        summary['links'] = [dict(name=name, **summarize(transactions, beats, start, finish, clock, (port,), tracks))
                            for port, name in enumerate(LINKS)]
        summary['sources'] = [dict(name=source, **summarize([t for t in transactions if t['source'] == source],
                                [b for b in beats if b['source'] == source], start, finish, clock))
                              for source in SOURCES if any(t['source'] == source for t in transactions)]
        return summary
    return dict(available=True, boundary='Three full-byte 64-bit GPU master links; not physical SDRAM pins',
                clock_hz=clock, clock_domain='GPU logic', byte_enable='No byte-enable field; full eight-byte payload slots',
                utilization_denominator='8 bytes/link/logic-cycle; aggregate sums three independent link capacities',
                complete=complete, transactions=transactions, summary=scope(0, end),
                timeline=temporal_data(transactions, beats, end, tracks, sample_interval,
                                       bool(events.get('MEM_DATA'))),
                commands={str(c['id']): scope(c['start'], c.get('complete_cycle') or end) for c in commands})


def validate(report, clock, command_ids, end):
    if not isinstance(report, dict) or type(report.get('available')) is not bool:
        raise ValueError('Invalid memory report')
    if not report['available']:
        return
    if report['clock_hz'] != clock or set(report['commands']) != {str(i) for i in command_ids}:
        raise ValueError('Memory clock/command scope mismatch')
    counts = ('start', 'end', 'cycles', 'issued_transactions', 'completed_transactions', 'failed_transactions',
              'data_beats', 'wire_bytes', 'effective_bytes', 'read_wire_bytes', 'write_wire_bytes',
              'outstanding_without_data_link_cycles', 'write_terminal_wait_link_cycles', 'link_capacity_bytes_per_cycle')
    rates = ('average_wire_bytes_per_second', 'average_effective_bytes_per_second',
             'average_read_bytes_per_second', 'average_write_bytes_per_second', 'utilization')
    peaks = ('peak_wire_bytes_per_second', 'peak_read_bytes_per_second', 'peak_write_bytes_per_second')
    def sample(s):
        if any(type(s[k]) is not int or not 0 <= s[k] <= (1 << 53)-1 for k in counts):
            raise ValueError('Invalid memory count')
        if not 0 <= s['start'] < s['end'] <= end or s['cycles'] != s['end']-s['start']:
            raise ValueError('Invalid memory interval')
        if (s['wire_bytes'] != s['data_beats']*8 or s['wire_bytes'] != s['read_wire_bytes']+s['write_wire_bytes'] or
                s['effective_bytes'] > s['wire_bytes'] or s['utilization'] > 1 or s['peak_window_cycles'] != WINDOW):
            raise ValueError('Memory payload/capacity mismatch')
        values = [s[k] for k in rates] + [s[k] for k in peaks if s[k] is not None]
        if any(type(v) not in (int, float) or not math.isfinite(v) or v < 0 for v in values):
            raise ValueError('Invalid memory rate')
    for scope in [report['summary'], *report['commands'].values()]:
        sample(scope)
        for subset in [*scope['links'], *scope['sources']]:
            sample(subset)
        for field in ('wire_bytes', 'effective_bytes', 'data_beats', 'issued_transactions', 'completed_transactions'):
            if any(sum(s[field] for s in scope[group]) != scope[field] for group in ('links', 'sources')):
                raise ValueError('Memory totals do not conserve link/source payloads')
    timeline = report.get('timeline')
    if timeline is None:  # Previously saved profiles did not retain beat cycles.
        return
    if type(timeline.get('available')) is not bool:
        raise ValueError('Invalid memory timeline availability')
    if not timeline['available']:
        if not isinstance(timeline.get('reason'), str):
            raise ValueError('Unavailable memory timeline needs a reason')
        return
    if timeline.get('version') != 1 or timeline.get('start') != 0 or timeline.get('end') != report['summary']['end']:
        raise ValueError('Memory timeline range/version mismatch')
    previous, totals = -1, [0, 0]
    for row in timeline['data']:
        if (len(row) != 3 or any(type(v) is not int for v in row) or not previous < row[0] < timeline['end']
                or row[1] not in (0, 8, 16) or row[2] not in (0, 8) or row[1] + row[2] == 0):
            raise ValueError('Invalid memory data timeline')
        previous = row[0]
        totals = [totals[i] + row[i + 1] for i in range(2)]
    if totals != [report['summary']['read_wire_bytes'], report['summary']['write_wire_bytes']]:
        raise ValueError('Memory timeline bytes do not conserve summary totals')
    def state(points, a, b, maximum=None):
        previous = a - 1
        if not points or points[0][0] != a:
            raise ValueError('Memory state timeline needs its first sample')
        for t, value in points:
            if (type(t) is not int or type(value) is not int or not previous < t < b or value < 0
                    or (maximum is not None and value > maximum)):
                raise ValueError('Invalid memory state timeline')
            previous = t
        return sum(value * ((points[i + 1][0] if i + 1 < len(points) else b) - t)
                   for i, (t, value) in enumerate(points))
    for name, cap in [('outstanding', None), ('waiting', 3), ('terminal_wait', 1)]:
        integral = state(timeline['states'][name], 0, timeline['end'], cap)
        expected = (sum((t['end'] if t['end'] is not None else timeline['end']) - t['start']
                        for t in report['transactions']) if name == 'outstanding' else
                    report['summary']['outstanding_without_data_link_cycles'] if name == 'waiting' else
                    report['summary']['write_terminal_wait_link_cycles'])
        if integral != expected:
            raise ValueError('Memory state timeline does not conserve captured link/request cycles')
    bp = timeline['backpressure']
    if type(bp.get('available')) is not bool:
        raise ValueError('Invalid backpressure availability')
    if bp['available']:
        if not 0 <= bp['start'] < bp['end'] <= timeline['end']:
            raise ValueError('Invalid backpressure sample interval')
        state(bp['track'], bp['start'], bp['end'], 3)
