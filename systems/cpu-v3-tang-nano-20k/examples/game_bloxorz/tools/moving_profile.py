"""Summarize a qualified moving-frame sidecar and the standard GPU profile."""
import argparse
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path
import re


def summarize(measurement, gpu_report, output):
    timing = json.loads((measurement / 'timing.json').read_text())
    profile = json.loads((gpu_report / 'profile.json').read_text())
    assert timing['complete'] and timing['right_read'] and timing['acknowledged'] and timing['released']
    assert not timing['open_cpu_intervals']
    assert profile['schema'] == 'digitaldesign.gpu-profile' and profile['version'] == 2
    digest = hashlib.sha256((measurement / 'cpu-v3-boot.bin').read_bytes()).hexdigest()
    assert profile['provenance']['report_inputs']['files']['program.bin'] == digest
    hz = timing['quantum_hz']
    assert hz == profile['system']['quantum_hz']
    listing = (measurement / 'application.listing').read_text()
    entries, calls = {}, {}
    for line in listing.splitlines():
        if ' @ 0x' in line:
            name, address = line.split(' @ 0x')
            entries[name] = int(address, 16)
        elif re.match(r'^\s+[0-9a-f]+:', line) and 'jalrel ' in line:
            words = line.split()
            at = words.index('jalrel')
            ret = int(words[0][:-1], 16) + at - 1
            calls[ret] = (ret + int(words[at + 1])) & 65535
    cpu = defaultdict(lambda: defaultdict(float))
    for interval in timing['cpu_intervals']:
        assert calls[interval['return_pc']] == entries[interval['function']], interval
        assert interval['end_tick'] > interval['begin_tick']
        cpu[interval['frame']][interval['function']] += (
            interval['end_tick'] - interval['begin_tick']) * 1000 / hz
    frames = {}
    for number in (2, 3):
        events = [e for e in timing['events'] if e.get('frame') == number]
        begin = next(e['tick'] for e in events if e['kind'] == 'frame_start')
        submits = [e for e in events if e['kind'] == 'cpu_submit']
        assert len(submits) == 2
        points = {'frame_start': begin}
        for role, submit in zip(('restore', 'dynamic'), submits):
            sid = submit['submission']
            done = next(e['tick'] for e in events if e['kind'] == 'gpu_done' and e['submission'] == sid)
            actual = next(s for s in profile['system']['submissions'] if s['id'] == sid)
            assert actual['start_tick'] == submit['tick'] and actual['end_tick'] == done
            points[f'cpu_{role}_submit'] = submit['tick']
            points[f'gpu_{role}_done'] = done
        relative = {key: (tick - begin) * 1000 / hz for key, tick in points.items()}
        frames[number] = dict(absolute_ticks=points, relative_ms=relative,
                              cpu_inclusive_ms=dict(cpu[number]),
                              submission_ids=[s['submission'] for s in submits])
    state = next(e['state'] for e in timing['events'] if e['kind'] == 'dynamic_state')
    assert state['phase'] == state['moves'] == state['orientation'] == 1
    assert state['x'] == 8 and state['z'] == 3 and state['mask'] == 25 and state['age_ms'] > 0
    assert cpu[3]['shadow_project'] > 0 and cpu[3]['shadow_hull'] > 0
    gpu = []
    axis = profile['system']['axis_origin_tick']
    for sid in frames[3]['submission_ids']:
        sub = next(s for s in profile['system']['submissions'] if s['id'] == sid)
        commands = [c for c in profile['commands'] if c['submission'] == sid]
        assert commands and all(c.get('complete_cycle') is not None for c in commands)
        lo, hi = [(sub[k] - axis) // 148 for k in ('start_tick', 'end_tick')]
        residency = defaultdict(float)
        for c in commands:
            residency[c['name']] += (c['end'] - c['start']) * 1000 / profile['clock_hz']
        activity = {}
        for definition in profile['metric_defs']:
            if definition['kind'] != 'binary':
                continue
            track = profile['tracks'][definition['key']]
            integral = sum(max(0, min(hi, b[0]) - max(lo, a[0])) * a[1]
                           for a, b in zip(track, track[1:] + [[profile['end'], 0]]))
            activity[definition['key']] = dict(active_ms=integral * 1000 / profile['clock_hz'],
                                                fraction=integral / (hi - lo))
        gpu.append(dict(submission=sid, duration_ms=(sub['end_tick'] - sub['start_tick']) * 1000 / hz,
                        commands=len(commands), command_types=dict(Counter(c['name'] for c in commands)),
                        cp_command_residency_ms=dict(residency), binary_stage_activity=activity))
    output.mkdir(parents=True, exist_ok=True)
    result = dict(state=state, frames=frames, gpu=gpu, partial=profile['partial'],
                  gpu_image_submissions=profile['images']['selected_submission_ids'],
                  cpu_return_sites_verified=len(timing['cpu_intervals']),
                  scope='CPU wall intervals are inclusive and overlap through nested calls; GPU stage activity overlaps. Frame-to-GPU-done excludes present. Same-run frame2 is ROLL age0 with the preceding shadow reusable; frame3 has a changed pose.')
    (output / 'summary.json').write_text(json.dumps(result, indent=2) + '\n')
    rows = ['| Point | Frame2 ms | Moving frame3 ms |', '| --- | ---: | ---: |']
    for key in frames[3]['relative_ms']:
        rows.append(f"| {key} | {frames[2]['relative_ms'][key]:.6f} | {frames[3]['relative_ms'][key]:.6f} |")
    rows += ['', 'CPU inclusive wall intervals; nested rows must not be added:', '',
             '| Function | Frame2 ms | Moving frame3 ms |', '| --- | ---: | ---: |']
    for name in ('prepare_block', 'draw_bridge_group', 'draw_switches', 'draw_pad_part',
                 'draw_shadow', 'shadow_project', 'shadow_hull', 'shadow_clip', 'shadow_batch_cell', 'shadow_batch_finish', 'draw_block'):
        rows.append(f"| {name} | {cpu[2].get(name, 0):.6f} | {cpu[3].get(name, 0):.6f} |")
    (output / 'summary.md').write_text('\n'.join(rows) + '\n')
    print(json.dumps(dict(state=state, moving_ms=frames[3]['relative_ms'], shadow_cpu_ms=cpu[3]['draw_shadow'],
                          gpu=[dict(submission=s['submission'], ms=s['duration_ms'], commands=s['commands']) for s in gpu]), indent=2))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('measurement', type=Path)
    parser.add_argument('gpu_report', type=Path)
    parser.add_argument('--out', required=True, type=Path)
    args = parser.parse_args()
    summarize(args.measurement, args.gpu_report, args.out)
