"""Decode CRC-protected board samples; retain raw data and honest timing units."""
import argparse
import binascii
import csv
import hashlib
import json
from pathlib import Path
import random
import statistics
import struct


def decode(data, token):
    frames, corrupt, foreign, pos = [], 0, 0, 0
    while True:
        at = data.find(b'GBEN', pos)
        if at < 0 or at + 20 > len(data):
            break
        block = data[at:at+20]
        if block[4] != 1 or binascii.crc_hqx(block[:18], 0xffff) != int.from_bytes(block[18:], 'little'):
            corrupt += 1
            pos = at + 1
            continue
        kind, case, sample, us, ms, flags, identity = struct.unpack('<B6H', block[5:18])
        if identity != token:
            foreign += 1
        else:
            frames.append(dict(offset=at, kind=kind, case=case, sample=sample,
                               us=us, ms=ms, flags=flags, token=identity))
        pos = at + 20
    return frames, dict(corrupt_packets=corrupt, foreign_packets=foreign)


def percentile(values, fraction):
    values = sorted(values)
    at = (len(values) - 1) * fraction
    low, high = int(at), min(len(values)-1, int(at)+1)
    return values[low] + (values[high] - values[low]) * (at-low)


def stats(values):
    rng = random.Random(7351)
    bootstrap = [statistics.median(rng.choices(values, k=len(values))) for _ in range(2000)]
    median = statistics.median(values)
    return dict(n=len(values), min=min(values), median=median,
                p05=percentile(values, .05), p95=percentile(values, .95), max=max(values),
                mad=statistics.median(abs(v-median) for v in values),
                median_ci95=[percentile(bootstrap, .025), percentile(bootstrap, .975)],
                resolution_us=1,
                median_ci95_with_quantization=[max(0, percentile(bootstrap, .025)-1),
                                               percentile(bootstrap, .975)+1])


def measurement_epoch(frames):
    """Never mix an old gallery or reset's partial samples with a new run."""
    start = None
    end = None
    for i, frame in enumerate(frames):
        if frame['kind'] == 0:
            start = i
            end = None
        elif frame['kind'] == 3 and start is not None:
            if end is not None:
                raise ValueError('Repeated completion marker in one measurement epoch')
            end = i
    if end is not None:
        return frames[start:end+1], end, True
    return frames[start:] if start is not None else frames, None, False


def report(manifest_path, capture_path, out, allow_partial=False):
    if out.exists() and any(out.iterdir()):
        raise ValueError('Use a fresh report directory; existing evidence is never overwritten')
    manifest = json.loads(manifest_path.read_text(encoding='utf8'))
    cases = manifest['cases']
    if not cases or [c['id'] for c in cases] != list(range(len(cases))):
        raise ValueError('Manifest case IDs must be dense and unique')
    if not 1 <= manifest['samples'] <= 65535:
        raise ValueError('Manifest sample count is invalid')
    data = capture_path.read_bytes()
    frames, integrity = decode(data, manifest['token'])
    frames, _, has_complete_epoch = measurement_epoch(frames)
    if has_complete_epoch:
        for marker in (frames[0], frames[-1]):
            if (marker['case'], marker['sample'], marker['flags']) != (len(cases), manifest['samples'], 0):
                raise ValueError('Epoch marker does not match the case/sample manifest')
        if any(f['kind'] not in (0, 1, 3, 5, 128) for f in frames):
            raise ValueError('Preview or unknown packet inside the headless epoch')
    for frame in frames:
        if frame['kind'] in (1, 5):
            if frame['case'] not in range(len(cases)):
                raise ValueError('Measurement case is outside the manifest')
            if frame['flags'] & ~1:
                raise ValueError('Headless samples cannot carry Display-active or unknown flags')
    errors = [f for f in frames if f['kind'] == 128]
    table, incomplete, samples = [], [], []
    for case in cases:
        groups = {}
        for kind in (1, 5):
            rows = [f for f in frames if f['kind'] == kind and f['case'] == case['id']]
            by_id = {}
            for row in rows:
                if row['sample'] in by_id:
                    raise ValueError('Duplicate measurement: separate board runs before analysis')
                by_id[row['sample']] = row
            if set(by_id) != set(range(manifest['samples'])):
                incomplete.append(dict(case=case['id'], kind=kind, received=len(by_id)))
            groups[kind] = rows
            samples += [{**r, 'name': case['name']} for r in rows]
        rows = groups[1]
        if not rows:
            continue
        saturated = any(r['us'] == 65535 or r['flags'] & 1 for r in rows)
        setup_saturated = any(r['us'] == 65535 or r['flags'] & 1 for r in groups[5])
        row = dict(id=case['id'], name=case['name'], family=case['family'],
                   samples=len(rows), us=stats([r['us'] for r in rows]), saturated=saturated,
                   setup_us=stats([r['us'] for r in groups[5]]) if groups[5] else None,
                   setup_saturated=setup_saturated,
                   vertices=case['vertices'], triangles=case['triangles'],
                   reference_pixels=case['reference_pixels'], reference_quads=case['reference_quads'])
        if not saturated and row['us']['median'] > 0:
            seconds = row['us']['median'] / 1e6
            row['submission_hz'] = 1 / seconds
            row['nominal_cycles'] = seconds * manifest['clock_hz']
            for key in ('vertices', 'triangles', 'reference_pixels', 'reference_quads'):
                if row[key]:
                    row[key + '_per_second'] = row[key] / seconds
            if case['family'] == 'clear':
                row['bytes_per_second'] = 2 * manifest['width'] * manifest['height'] / seconds
            if case['reference_quads']:
                row['reference_quad_occupancy'] = case['reference_pixels'] / (4*case['reference_quads'])
                row['nominal_cycles_per_reference_quad'] = row['nominal_cycles'] / case['reference_quads']
            if case['reference_pixels']:
                row['nominal_cycles_per_reference_pixel'] = row['nominal_cycles'] / case['reference_pixels']
        table.append(row)
    complete = not incomplete and not errors and has_complete_epoch
    result = dict(version=1, complete=complete, integrity=integrity, errors=errors,
                  incomplete=incomplete, cases=table,
                  manifest_sha256=hashlib.sha256(manifest_path.read_bytes()).hexdigest(),
                  capture_sha256=hashlib.sha256(data).hexdigest(),
                  timing='1us board interval, timer-clear through completed END/FLUSH observation; CPU/transport overhead included',
                  boundary='Headless before first present. Reference coverage is not an internal hardware counter.')
    out.mkdir(parents=True, exist_ok=True)
    (out/'report.json').write_text(json.dumps(result, indent=2)+'\n', encoding='utf8')
    with (out/'samples.csv').open('w', newline='', encoding='utf8') as stream:
        if samples:
            writer = csv.DictWriter(stream, samples[0].keys()); writer.writeheader(); writer.writerows(samples)
    lines = ['# GPU board throughput', '', result['timing'], '', result['boundary'], '',
             '| Case | p50 us | p05 / p95 us | median CI95 us | cycles / reference pixel |',
             '| --- | ---: | ---: | --- | ---: |']
    for c in table:
        s = c['us']; cycle = c.get('nominal_cycles_per_reference_pixel')
        lines.append(f'| {c["name"]} | {s["median"]:g} | {s["p05"]:g} / {s["p95"]:g} | {s["median_ci95"]} | '
                     + ('saturated' if c['saturated'] else f'{cycle:.3f}' if cycle else '-') + ' |')
    (out/'README.md').write_text('\n'.join(lines)+'\n', encoding='utf8')
    print(json.dumps(dict(complete=complete, cases=len(table), incomplete=len(incomplete),
                         saturated=[c['name'] for c in table if c['saturated']], errors=errors, **integrity)))
    if not complete and not allow_partial:
        raise ValueError('Incomplete or failed benchmark; report retained for diagnosis')


if __name__ == '__main__':
    p = argparse.ArgumentParser(__doc__)
    p.add_argument('--manifest', required=True, type=Path)
    p.add_argument('--capture', required=True, type=Path)
    p.add_argument('--out', required=True, type=Path)
    p.add_argument('--allow-partial', action='store_true')
    args = p.parse_args()
    report(args.manifest, args.capture, args.out, args.allow_partial)
