"""Actual payload accounting at the three 64-bit GPU master interfaces."""
from collections import deque
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


def build(events, origin, stop, clock, complete, commands, tracks):
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
