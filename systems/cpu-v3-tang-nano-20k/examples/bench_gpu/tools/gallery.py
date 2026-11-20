"""Associate HDMI stills with timestamped UART gallery events; no board mutation."""
import argparse
import bisect
import csv
import datetime
import json
import math
from pathlib import Path
import re
import subprocess
import sys

sys.dont_write_bytecode = True

import report


def utc_seconds(value):
    """Accept the shared CSV's UTC ISO strings, including seven decimal places."""
    # Python 3.10 only accepts up to microseconds; PowerShell emits 100 ns.
    value = re.sub(r'(\.\d{6})\d+(?=Z|[+-]\d{2}:\d{2}$)', r'\1', value)
    stamp = datetime.datetime.fromisoformat(value.replace('Z', '+00:00'))
    if stamp.tzinfo is None:
        raise ValueError('UART timing must include an explicit timezone')
    return stamp.timestamp()


def attach_timestamps(board, events, raw_bytes):
    """Read shared live CSV, or old JSON solely for archived evidence."""
    csv_path = board/'uart.bin.timing.csv'
    if csv_path.exists():
        with csv_path.open(encoding='utf-8-sig', newline='') as stream:
            reader = csv.DictReader(stream)
            if reader.fieldnames != ['offset', 'count', 'utc']:
                raise ValueError('Expected shared UART timing CSV: offset,count,utc')
            chunks = [(int(r['offset']), int(r['count']), utc_seconds(r['utc'])) for r in reader]
        source = csv_path.name
    else:
        path = board/'uart.bin.timing.json'
        timing = json.loads(path.read_text(encoding='utf-8-sig'))
        start = utc_seconds(timing['start_utc'])
        chunks = [(r['offset'], r['count'], start + r['seconds']) for r in timing['chunks']]
        source = path.name
    offset, previous, ends, times = 0, -math.inf, [], []
    for at, count, when in chunks:
        if at != offset or count <= 0 or not math.isfinite(when) or when < previous:
            raise ValueError('UART timing chunks must be contiguous and time ordered')
        offset += count
        ends.append(offset)
        times.append(when)
        previous = when
    if offset != raw_bytes:
        raise ValueError('UART timing does not cover the exact captured byte stream')
    for event in events:
        end = event['offset'] + 20
        index = bisect.bisect_left(ends, end)
        if event['offset'] < 0 or index >= len(chunks):
            raise ValueError('UART packet is outside timing coverage')
        event['utc'] = times[index]
    return source


def extract(board, manifest_path, ffmpeg, ffprobe):
    from PIL import Image, ImageDraw

    manifest = json.loads(manifest_path.read_text(encoding='utf8'))
    raw = (board/'uart.bin').read_bytes()
    events, _ = report.decode(raw, manifest['token'])
    timing_source = attach_timestamps(board, events, len(raw))
    _, epoch_end, complete = report.measurement_epoch(events)
    if not complete:
        raise ValueError('No completed measurement epoch to bind the gallery')
    report_manifest = json.loads((board/'report/report.json').read_text(encoding='utf8'))
    import hashlib
    if (not report_manifest['complete'] or
            report_manifest['manifest_sha256'] != hashlib.sha256(manifest_path.read_bytes()).hexdigest() or
            report_manifest['capture_sha256'] != hashlib.sha256(raw).hexdigest()):
        raise ValueError('Gallery requires a complete report matching these exact inputs')
    result = subprocess.run([ffprobe, '-v', 'error', '-select_streams', 'v:0', '-show_entries',
        'frame=best_effort_timestamp_time', '-of', 'json', str(board/'capture.mkv')],
        check=True, capture_output=True, timeout=40)
    frames = json.loads(result.stdout)['frames']
    times = [float(f['best_effort_timestamp_time']) for f in frames]
    selected = {}
    for event in events[epoch_end+1:]:
        if event['kind'] != 2 or event['case'] in selected:
            continue
        if event['case'] not in range(len(manifest['cases'])) or event['flags'] & 4 == 0:
            raise ValueError('Invalid Display-active preview packet')
        desired = event['utc'] + .9
        k = bisect.bisect_left(times, desired)
        if k < len(times) and abs(times[k]-desired) < .25:
            selected[event['case']] = dict(frame=k, utc=times[k], uart_utc=event['utc'])
    if len(selected) != len(manifest['cases']):
        raise ValueError('Video does not contain a settled preview for every case')
    output = board/'gallery-verified'; output.mkdir(parents=True, exist_ok=False)
    (output/'uart-events.json').write_text(json.dumps(events, indent=2)+'\n')
    indices = sorted({s['frame'] for s in selected.values()})
    selector = '+'.join(f'eq(n\\,{k})' for k in indices)
    command = [ffmpeg, '-hide_banner', '-loglevel', 'error', '-i', str(board/'capture.mkv'),
        '-vf', selector.join(['select=', ',scale=in_range=full:out_range=full:in_color_matrix=bt601,format=rgb24']),
        '-vsync', '0', str(output/'capture-%03d.png')]
    result = subprocess.run(command, capture_output=True, timeout=60)
    (output/'decode.log').write_bytes(result.stdout+result.stderr)
    result.check_returncode()
    by_frame = {frame: output/f'capture-{i:03d}.png' for i, frame in enumerate(indices, 1)}
    sheet = Image.new('RGB', (4*400, ((len(selected)+3)//4)*266), (24, 24, 24))
    draw = ImageDraw.Draw(sheet)
    records = []
    for case in manifest['cases']:
        info = selected[case['id']]
        with Image.open(by_frame[info['frame']]) as image:
            if image.size != (1600, 1200):
                raise ValueError('Pixel recovery only supports the calibrated 1600x1200 mode')
            # Center samples of the measured 4x5 capture-card scale. JPEG or
            # YUV conversion is not RGB565-bit-exact qualification.
            import numpy as np
            source = Image.fromarray(np.asarray(image.convert('RGB'))[2::5, 2::4])
        path = output/(f'{case["id"]:02d}-'+case['name']+'.png')
        source.save(path)
        x, y = (case['id'] % 4)*400, (case['id']//4)*266
        sheet.paste(source, (x, y+26))
        draw.text((x+5, y+6), f'{case["id"]:02d} {case["name"]}', fill='white')
        records.append(dict(case=case['id'], name=case['name'], image=path.name, **info))
    sheet.save(output/'contact-sheet.png')
    (output/'identity.json').write_text(json.dumps(dict(
        capture_mode='1600x1200 YUYV 5fps; full-range BT.601 decode; 4x5 center recovery',
        not_bit_exact_rgb565=True, association='After completed measurement epoch, UART event +0.9s, absolute video wall timestamps',
        uart_timing_source=timing_source,
        cases=records), indent=2)+'\n')
    print(json.dumps(dict(cases=len(records), contact_sheet=str(output/'contact-sheet.png'))))


if __name__ == '__main__':
    p = argparse.ArgumentParser(__doc__)
    p.add_argument('--board', type=Path, required=True)
    p.add_argument('--manifest', type=Path, required=True)
    p.add_argument('--ffmpeg', default='ffmpeg')
    p.add_argument('--ffprobe', default='ffprobe')
    a = p.parse_args()
    extract(a.board, a.manifest, a.ffmpeg, a.ffprobe)
