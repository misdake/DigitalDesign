"""Exercise complete motion publication and ordered error prefixes."""
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
    data = json.loads((args.reference / 'car-motion.json').read_text())
    args.output.mkdir(parents=True, exist_ok=False)
    faults = [
        ('car-motion-0', 0, 2, 'integrated-position'),
        ('car-motion-0', 0, 210, 'previous-position'),
        ('car-motion-1', 0, 0, 'invalid-time'),
        ('car-motion-15', 0, 2, 'position-before-attitude-failure'),
        ('car-motion-15', 0, 32, 'attitude-transaction'),
        ('car-motion-7', 0, 343, 'gun-write-before-late-error'),
        ('car-motion-8', 0, 238, 'articulation-before-first-vertex-error'),
        ('car-motion-16', 0, 0, 'pose-failure'),
        ('car-motion-16', 0, 26, 'pose-prefix'),
        ('car-motion-6', 1, 238, 'absent-gun-articulation'),
        ('car-motion-4', 1, 246, 'mode-gated-lean'),
        ('car-motion-29', 0, 331, 'successful-ready-marker'),
        ('car-motion-25', 0, 330, 'contact-hull-radius'),
        ('car-motion-27', 0, 333, 'wrapped-rate-rescaling'),
        ('car-motion-43', 1, 32, 'orthogonal-policy'),
    ]
    results = []
    for index, (tag, frame, word, kind) in enumerate(faults):
        case = copy.deepcopy(next(c for c in data['cases'] if c['tag'] == tag))
        case['frames'][frame][word] ^= 1
        folder = args.output / str(index)
        folder.mkdir()
        for asset in ['sine.i16.bin', 'angle.i16.bin']:
            shutil.copyfile(args.reference / asset, folder / asset)
        (folder / 'car-motion.json').write_text(json.dumps(dict(cases=[case], scope=data['scope'])))
        process = subprocess.run(
            [str(args.checker.resolve()), str(folder / 'check'), str(folder), '0', 'car-motion'],
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
