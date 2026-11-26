"""Exercise complete car effects ordering, shared aliases and error prefixes."""
import argparse
import copy
import json
from pathlib import Path
import subprocess
import shutil


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('checker', type=Path)
    parser.add_argument('reference', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    data = json.loads((args.reference / 'car-effects.json').read_text())
    args.output.mkdir(parents=True, exist_ok=False)
    faults = [
        ('car-effects-0', 1, 1556, 'whole-effects-rng'),
        ('car-effects-0', 1, 1558, 'impact-cooldown'),
        ('car-effects-0', 0, 1556, 'smoke-debris-shared-rng-order'),
        ('car-effects-1', 1, 12928, 'temporary-state'),
        ('car-effects-2', 0, 8270, 'special-player-spray-prefix'),
        ('car-effects-2', 0, 852, 'shield-position'),
        ('car-effects-3', 0, 0, 'spray-failure'),
        ('car-effects-4', 0, 0, 'mark-failure'),
        ('car-effects-5', 0, 1558, 'impact-before-socket-error'),
        ('car-effects-6', 0, 1556, 'second-socket-prefix'),
        ('car-effects-7', 0, 0, 'light-failure'),
        ('car-effects-8', 0, 0, 'temporary-slot-failure'),
        ('car-effects-12', 0, 740, 'debris-owner-failure-prefix'),
        ('car-effects-20', 1, 8650, 'physical-position-light-anchor'),
        ('car-effects-31', 1, 13010, 'base-position-temporary-anchor'),
        ('car-effects-32', 0, 0, 'owner-capacity-after-smoke'),
        ('car-effects-33', 0, 0, 'impact-pool-failure'),
        ('car-effects-36', 0, 1558, 'pickup-failure-before-impacts'),
        ('car-effects-37', 0, 0, 'pickup-retirement-failure'),
    ]
    results = []
    for index, (tag, frame, word, kind) in enumerate(faults):
        case = copy.deepcopy(next(c for c in data['cases'] if c['tag'] == tag))
        case['frames'][frame][word] ^= 1
        folder = args.output / str(index)
        folder.mkdir()
        for asset in ['sine.i16.bin', 'angle.i16.bin']:
            shutil.copyfile(args.reference / asset, folder / asset)
        (folder / 'car-effects.json').write_text(json.dumps(dict(cases=[case], scope=data['scope'])))
        process = subprocess.run(
            [str(args.checker.resolve()), str(folder / 'check'), str(folder), '0', 'car-effects'],
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
