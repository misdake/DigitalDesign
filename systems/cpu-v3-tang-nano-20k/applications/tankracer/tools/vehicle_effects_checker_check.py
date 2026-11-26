"""Exercise smoke, synchronous impacts, debris ownership and shield answer differences."""
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
    data = json.loads((args.reference / 'vehicle-effects.json').read_text())
    args.output.mkdir(parents=True, exist_ok=False)
    faults = [
        ('vehicle-effects-2', 0, 6, 'penetration-damping'),
        ('vehicle-effects-16', 0, 34, 'narrow-particle-life'),
        ('vehicle-effects-9', 0, 18, 'damage-timer'),
        ('vehicle-effects-11', 0, 1556, 'rng-before-socket-error'),
        ('vehicle-effects-12', 0, 0, 'second-socket-failure'),
        ('vehicle-effects-5', 0, 1570, 'synchronous-impact-ring'),
        ('vehicle-effects-22', 0, 0, 'invalid-corner-prefix'),
        ('vehicle-effects-38', 0, 0, 'invalid-angle-prefix'),
        ('vehicle-effects-46', 0, 726, 'stolen-bank-owner'),
        ('vehicle-effects-47', 0, 740, 'impact-before-owner-error'),
        ('vehicle-effects-44', 0, 744, 'sound-draw-before-material-error'),
        ('vehicle-effects-40', 0, 546, 'debris-color-and-discarded-rng'),
        ('vehicle-effects-61', 0, 712, 'selective-bank-clear'),
        ('vehicle-effects-76', 0, 1556, 'shield-bootstrap-rng'),
        ('vehicle-effects-76', 0, 764, 'preserved-orbit-position'),
        ('vehicle-effects-88', 1, 852, 'shield-without-external-basis'),
        ('vehicle-effects-95', 1, 848, 'shield-growth-byte'),
        ('vehicle-effects-85', 0, 0, 'invalid-bank'),
        ('vehicle-effects-100', 2, 1556, 'shared-effects-rng-order'),
    ]
    results = []
    for index, (tag, frame, word, kind) in enumerate(faults):
        case = copy.deepcopy(next(c for c in data['cases'] if c['tag'] == tag))
        case['frames'][frame][word] ^= 1
        folder = args.output / str(index)
        folder.mkdir()
        for asset in ['sine.i16.bin', 'angle.i16.bin']:
            shutil.copyfile(args.reference / asset, folder / asset)
        (folder / 'vehicle-effects.json').write_text(json.dumps(dict(cases=[case], scope=data['scope'])))
        process = subprocess.run(
            [str(args.checker.resolve()), str(folder / 'check'), str(folder), '0', 'vehicle-effects'],
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
