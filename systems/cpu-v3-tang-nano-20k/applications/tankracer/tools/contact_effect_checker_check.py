"""Check publication timing, rejected slots, RNG and terrain-page observations."""
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
    data = json.loads((args.reference / 'contact-effect.json').read_text())
    args.output.mkdir(parents=True, exist_ok=False)
    results = []
    for source, word, label in [(2, 212, 'late-error-tail'), (3, 208, 'early-error-seed'),
                                (4, 0, 'invalid-slot'), (2, 166, 'partial-effect-state'),
                                (0, 1, 'terrain-page')]:
        case = copy.deepcopy(data['cases'][source])
        case['frames'][0][word] ^= 1
        folder = args.output / label
        folder.mkdir()
        name = f"contact-effect-cells-{case['variant']}.bin"
        shutil.copyfile(args.reference / name, folder / name)
        (folder / 'contact-effect.json').write_text(json.dumps(dict(cases=[case], scope=data['scope'])))
        process = subprocess.run([str(args.checker.resolve()), str(folder / 'check'),
                                  str(folder), '0', 'contact-effect'],
                                 capture_output=True, text=True, timeout=90)
        log = process.stdout + process.stderr
        (folder / 'check.log').write_text(log)
        expected = f'tick 0 word {word}:'
        if process.returncode == 0 or expected not in log:
            raise RuntimeError(f'{label}: expected {expected}: {log[-1500:]}')
        results.append(dict(label=label, source_case=source, word=word, rejected=True))
    (args.output / 'report.json').write_text(json.dumps(dict(passed=True, faults=results), indent=2) + '\n')


if __name__ == '__main__':
    main()
