"""Check complete inactive-car answers, retained fields and failure prefixes."""
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
    data = json.loads((args.reference / 'vehicle-inactive.json').read_text())
    args.output.mkdir(parents=True, exist_ok=False)
    faults = [(0, 1, 4, 'physical-motion'), (0, 1, 76, 'track-pose'),
              (0, 0, 941, 'spray-clear'), (20, 0, 1329, 'retained-smoke'),
              (18, 0, 14318, 'restored-model-grip'),
              (12, 1, 4843, 'shared-spin-pulse'),
              (44, 0, 14198, 'shared-rng'),
              (36, 0, 0, 'fallback-failure'), (28, 0, 0, 'late-hull-failure')]
    results = []
    for source, tick, word, label in faults:
        case = copy.deepcopy(data['cases'][source])
        case['frames'][tick][word] ^= 1
        folder = args.output / label
        folder.mkdir()
        for name in ['sine.i16.bin', 'angle.i16.bin', 'projectile-cells.bin']:
            shutil.copyfile(args.reference / name, folder / name)
        (folder / 'vehicle-inactive.json').write_text(json.dumps(dict(cases=[case], scope=data['scope'])))
        process = subprocess.run([str(args.checker.resolve()), str(folder / 'check'),
                                  str(folder), '0', 'vehicle-inactive'],
                                 capture_output=True, text=True, timeout=90)
        log = process.stdout + process.stderr
        (folder / 'check.log').write_text(log)
        expected = f'call {tick} word {word}:'
        if process.returncode == 0 or expected not in log:
            raise RuntimeError(f'{label}: expected {expected}: {log[-1500:]}')
        results.append(dict(label=label, case=source, call=tick, word=word, rejected=True))
    (args.output / 'report.json').write_text(json.dumps(dict(passed=True, faults=results), indent=2) + '\n')


if __name__ == '__main__':
    main()
