"""Exercise selected spray, mark FIFO and light-pool answer differences."""
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
    args = parser.parse_args()
    data = json.loads((args.reference / 'track-effects.json').read_text())
    args.output.mkdir(parents=True, exist_ok=False)
    faults = [
        ('tracks-0', 0, 176, 'last-free-particle'),
        ('tracks-4', 0, 0, 'invalid-live-kind'),
        ('tracks-8', 0, 4, 'side-error-prefix'),
        ('tracks-20', 0, 184, 'narrow-gain'),
        ('tracks-34', 0, 210, 'mark-fifo'),
        ('tracks-36', 0, 5526, 'allocation-before-tail-error'),
        ('tracks-63', 1, 5494, 'exhausted-light-alias'),
        ('tracks-68', 1, 413, 'retained-light-color'),
        ('tracks-72', 2, 210, 'shared-pool-sequence'),
        ('tracks-65', 0, 0, 'invalid-light-handle'),
    ]
    results = []
    for index, (tag, frame, word, kind) in enumerate(faults):
        case = copy.deepcopy(next(c for c in data['cases'] if c['tag'] == tag))
        case['frames'][frame][word] ^= 1
        folder = args.output / str(index)
        folder.mkdir()
        (folder / 'track-effects.json').write_text(json.dumps(dict(cases=[case], scope=data['scope'])))
        process = subprocess.run(
            [str(args.checker.resolve()), str(folder / 'check'), str(folder), '0', 'track-effects'],
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
