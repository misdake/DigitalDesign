"""Confirm finish-driving comparisons observe state, error prefixes and RNG."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import shutil
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('fixtures', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--checker', type=Path, required=True)
    args = parser.parse_args()
    source = json.loads((args.fixtures / 'finish-drive.json').read_text())
    tests = [
        ('baseline', 'double-selector-lap-3', None),
        ('status', 'missing-ai-0', 0),
        ('untouched-ai-field', 'double-selector-lap-3', 1),
        ('look-position', 'checked-divide-overflow-after-distance', 5),
        ('distance-error-prefix', 'checked-divide-overflow-after-distance', 14),
        ('steering', 'double-selector-lap-3', 33),
        ('rng', 'double-selector-lap-3', 34),
        ('look-checkpoint-error-prefix', 'waypoint-look-published-before-error', 9),
        ('passed-wrap', 'waypoint-lap-3-completed-3', 37),
        ('lap-error-prefix', 'waypoint-lap--1-completed--1', 47),
        ('horn', 'controls-flags0-confusion0-anger5001-spacing0', 49),
        ('turn-timer', 'controls-flags0-confusion0-anger5001-spacing0', 15),
    ]
    args.output.mkdir(parents=True, exist_ok=False)
    results = []
    for label, tag, field in tests:
        folder = args.output / label
        fixtures = folder / 'fixtures'
        fixtures.mkdir(parents=True)
        case = copy.deepcopy(next(c for c in source['cases'] if c['tag'] == tag))
        if field is not None:
            case['frames'][0][field] ^= 1
        (fixtures / 'finish-drive.json').write_text(json.dumps({'cases': [case]}))
        shutil.copyfile(args.fixtures / 'angle.i16.bin', fixtures / 'angle.i16.bin')
        result = subprocess.run([str(args.checker.resolve()), str(folder / 'check'),
                                 str(fixtures), '0', 'finish-drive'],
                                capture_output=True, text=True, timeout=60)
        (folder / 'run.log').write_text(result.stdout + result.stderr)
        if field is None:
            if result.returncode != 0:
                raise ValueError(f'{label}: baseline failed: {result.stderr}')
        elif result.returncode == 0 or f'field {field}:' not in result.stderr:
            raise ValueError(f'{label}: comparison did not identify field {field}: {result.stderr}')
        results.append(dict(label=label, field=field, detected=field is not None))
    identity = hashlib.sha256(args.checker.read_bytes()).hexdigest()
    (args.output / 'report.json').write_text(json.dumps(dict(passed=True, tests=results,
        checker_sha256=identity), indent=2) + '\n')
    print(json.dumps(dict(passed=True, checks=len(results))))


if __name__ == '__main__':
    main()
