"""Prove weapon snapshot checks reject RNG, pose, trail and error-prefix faults."""
import argparse
import copy
import json
from pathlib import Path
import shutil
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('checker', type=Path)
    parser.add_argument('fixture', type=Path)
    parser.add_argument('output', type=Path)
    scope = parser.add_mutually_exclusive_group()
    scope.add_argument('--explosion', action='store_true', help='check explosion method snapshots')
    scope.add_argument('--explosion-blasts', action='store_true', help='check shared blast snapshots')
    scope.add_argument('--projectile-marks', action='store_true', help='check terrain mark queue snapshots')
    args = parser.parse_args()
    checker = args.checker.resolve()
    fixture = args.fixture.resolve()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    results = []
    faults = [
        ('projectile', 'kind-2-branch-0', 0, 731, 'launch RNG'),
        ('projectile', 'kind-0-branch-5', 0, 143, 'trail active state'),
        ('projectile', 'reflect-0-0-4', 0, 707, 'reflection pose'),
        ('mine', 'impulse-0-0-false', 0, 57, 'angular write before missing-basis error'),
        ('tank-hit', 'contact-0-true-0-0-false', 1, 57, 'private body revision after repeated hit'),
        ('tank-hit', 'contact-5-true-0-0-true', 0, 'warp-seed', 'Warp shared effect RNG'),
        ('temporary', 'temporary-9', 0, 278, 'RNG before invalid line update'),
    ]
    if args.explosion:
        faults = [
            ('explosion', 'animation-0-false', 0, 678, 'initialization RNG'),
            ('explosion', 'contact-0-true-0-false', 2, 28, 'normalized attachment before missing body'),
            ('explosion', 'contact-1-false-0-true', 2, 414, 'concussion fragment velocity'),
            ('explosion', 'animation-3-true', 0, 402, 'attached smoke retains existing lift'),
            ('explosion', 'animation-10-false', 0, 86, 'smoke byte color'),
            ('explosion', 'animation-1-false', 0, 0, 'early retirement status'),
            ('explosion', 'animation-6-false', 0, 4, 'timer write before missing shared blast'),
        ]
    if args.explosion_blasts:
        faults = [
            ('explosion-blasts', 'blasts-0', 2, 5, 'evicted owner identity'),
            ('explosion-blasts', 'blasts-1', 3, 7158, 'live ring RNG'),
            ('explosion-blasts', 'blasts-4', 0, 42, 'retained handles after partial allocation'),
            ('explosion-blasts', 'blasts-10', 0, 384, 'quad released before invalid band handle'),
            ('explosion-blasts', 'blasts-11', 1, 384, 'quad released before invalid ring handle'),
            ('explosion-blasts', 'blasts-13', 0, 12, 'cross-kind release before invalid next slot'),
            ('explosion-blasts', 'blasts-23', 0, 7162, 'projected Q12 geometry'),
            ('explosion-blasts', 'blasts-0', 0, 7188, 'projected additive blend and color'),
            ('explosion-blasts', 'blasts-2', 0, 24, 'committed allocation count on pool exhaustion'),
        ]
    if args.projectile_marks:
        faults = [
            ('projectile-marks','marks-0',24,4,'oldest mark queue wrap'),
            ('projectile-marks','marks-1',0,56,'quantized mark position'),
            ('projectile-marks','marks-0',0,64,'averaged terrain height'),
            ('projectile-marks','marks-0',0,82,'material blend and color'),
            ('projectile-marks','marks-0',0,84,'crater UV'),
            ('projectile-marks','marks-2',0,2,'exhaustion retains count'),
            ('projectile-marks','marks-5',0,0,'invalid oldest handle status'),
            ('projectile-marks','marks-8',0,5200,'quad overflow line alias'),
        ]
    for index, (kind, tag, frame, word, label) in enumerate(faults):
        folder = output / str(index)
        folder.mkdir(exist_ok=False)
        data = json.loads((fixture / f'{kind}.json').read_text())
        case = copy.deepcopy(next(c for c in data['cases'] if c['tag'] == tag))
        if word == 'warp-seed':
            word = case['input'][13] - 16 + 2 + 274
        case['frames'][frame][word] ^= 1
        data['cases'] = [case]
        (folder / f'{kind}.json').write_text(json.dumps(data))
        for name in ['projectile-cells.bin', 'sine.i16.bin', 'angle.i16.bin']:
            shutil.copyfile(fixture / name, folder / name)
        run = subprocess.run([str(checker), str(folder / 'check'), str(folder), '0', kind],
                             capture_output=True, text=True, timeout=120)
        message = run.stdout + run.stderr
        if run.returncode == 0 or f'word {word}:' not in message:
            raise RuntimeError(f'{label} was not rejected at the changed word: {message[-2000:]}')
        results.append(dict(kind=kind, tag=tag, frame=frame, word=word, fault=label, rejected=True))
    (output / 'report.json').write_text(json.dumps(dict(passed=True, faults=results), indent=2) + '\n')


if __name__ == '__main__':
    main()
