"""Confirm fresh-player qualification checks model-derived and default fields."""
import argparse
import copy
import json
from pathlib import Path
import shutil
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('checker', type=Path)
    parser.add_argument('reference', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    data = json.loads((args.reference / 'vehicle-startup.json').read_text())
    args.output.mkdir(parents=True, exist_ok=False)
    faults = [(0, 0, 'spawn'), (1, 48, 'wobble-body'), (0, 316, 'shadow-hull'),
              (0, 361, 'spring-corner'), (0, 469, 'contact-node'),
              (0, 787, 'alternate-input'), (0, 1661, 'pose-config'),
              (35, None, 'short-shadow'), (36, None, 'pose-error')]
    results = []
    for index, (source, word, label) in enumerate(faults):
        case = copy.deepcopy(data['cases'][source])
        if word is None:
            case['status'] -= 1
            expected = 'status'
        else:
            case['state'][word] ^= 1
            expected = f'call 0 word {word}:'
        folder = args.output / label
        folder.mkdir()
        for asset in ['sine.i16.bin', 'angle.i16.bin']:
            shutil.copyfile(args.reference / asset, folder / asset)
        (folder / 'vehicle-startup.json').write_text(json.dumps(dict(cases=[case], scope=data['scope'])))
        process = subprocess.run([str(args.checker.resolve()), str(folder / 'check'),
                                  str(folder), '0', 'vehicle-startup'],
                                 capture_output=True, text=True, timeout=90)
        log = process.stdout + process.stderr
        (folder / 'check.log').write_text(log)
        if process.returncode == 0 or expected not in log:
            raise RuntimeError(f'{label}: expected {expected}: {log[-1500:]}')
        results.append(dict(label=label, case=source, word=word, rejected=True))
    (args.output / 'report.json').write_text(json.dumps(dict(passed=True, faults=results), indent=2) + '\n')


if __name__ == '__main__':
    main()
