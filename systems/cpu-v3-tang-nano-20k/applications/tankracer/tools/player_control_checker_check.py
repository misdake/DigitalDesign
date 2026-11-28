"""Check that control-prefix qualification detects selected state differences."""
import argparse
import copy
import json
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('checker', type=Path)
    parser.add_argument('reference', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--routes', action='store_true')
    args = parser.parse_args()
    mode = 'player-route' if args.routes else 'player-control'
    filename = mode + '.json'
    data = json.loads((args.reference / filename).read_text())
    args.output.mkdir(parents=True, exist_ok=False)
    faults = [
        ('keys-4', 0, 2, 'throttle'),
        ('countdown-2-0', 2, 18, 'fast-start'),
        ('analog-1-1', 1, 6, 'analog-latch'),
        ('turret-3', 0, 24, 'turret-velocity'),
        ('brake-3', 0, 10, 'brake-override'),
        ('brake-11', 1, 12, 'unsigned-slide'),
        ('restore-complete', 0, 46, 'restored-yaw-lock'),
        ('collision-0', 0, 50, 'collision-marker'),
        ('prefix-0', 2, 0, 'prefix-outcome'),
    ]
    if args.routes:
        faults = [
            ('route-0', 0, 4, 'checkpoint'),
            ('route-48', 0, 10, 'lap-record'),
            ('route-49', 0, 6, 'failed-record-prefix'),
            ('route-52', 0, 26, 'lap-wrap'),
            ('route-53', 1, 0, 'work-limit'),
            ('route-59', 1, 2, 'route-fallback'),
            ('route-61', 1, 0, 'unresolved-score-link'),
            ('route-63', 2, 50, 'nearest-tie'),
            ('route-69', 0, 22, 'wrong-way-timer'),
            ('route-80', 0, 42, 'countdown'),
            ('route-85', 0, 24, 'elapsed'),
            ('route-91', 3, 52, 'physics-dt'),
        ]
    results = []
    for index, (tag, frame, word, kind) in enumerate(faults):
        case = copy.deepcopy(next(c for c in data['cases'] if c['tag'] == tag))
        case['frames'][frame][word] ^= 1
        folder = args.output / str(index)
        folder.mkdir()
        (folder / filename).write_text(json.dumps(dict(cases=[case], scope=data['scope'])))
        process = subprocess.run(
            [str(args.checker.resolve()), str(folder / 'check'), str(folder), '0', mode],
            capture_output=True, text=True, timeout=90,
        )
        log = process.stdout + process.stderr
        (folder / 'check.log').write_text(log)
        expected = f'call {frame} word {word}:'
        if process.returncode == 0 or expected not in log:
            raise RuntimeError(f'{kind}: qualification failed to identify {expected}: {log[-1200:]}')
        results.append(dict(kind=kind, tag=tag, frame=frame, word=word, rejected=True))
    (args.output / 'report.json').write_text(json.dumps(dict(passed=True, faults=results), indent=2) + '\n')


if __name__ == '__main__':
    main()
