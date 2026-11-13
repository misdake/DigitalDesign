"""Run Bloxorz's deterministic CPU/cache profile with a functional GPU."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
from datetime import datetime, timezone

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[5]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--level', choices=('1', '29', 'all'), default='all')
    parser.add_argument('--label', default='local')
    parser.add_argument('--no-auto-inline', action='store_true', help='Keep explicit hints, disable automatic inlining')
    parser.add_argument('--out', type=Path, help='New or empty output directory, relative to the repository')
    args = parser.parse_args()
    if not re.fullmatch(r'[A-Za-z0-9_-]+', args.label):
        parser.error('label must contain letters, digits, underscores or hyphens')
    output = args.out or Path('target/bloxorz-cpu') / datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%S%fZ')
    if not output.is_absolute():
        output = ROOT / output
    if output.exists() and (not output.is_dir() or any(output.iterdir())):
        parser.error('output directory must be new or empty')
    output.mkdir(parents=True, exist_ok=True)
    temporary = ROOT / 'target/tmp'
    temporary.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env.update(TEMP=str(temporary), TMP=str(temporary), BLOXORZ_SIM_OUTPUT_ROOT=str(output),
               BLOXORZ_CPU_ARM=args.label, BLOXORZ_START_PHASE='play',
               BLOXORZ_CPU_AUTO_INLINE='0' if args.no_auto_inline else '1')
    levels = (1, 29) if args.level == 'all' else (int(args.level),)
    command = ['cargo', 'test', '--release', '-p', 'cpu-v3-tang-nano-20k', '--test', 'host',
               'temporary_reuse_cpu_matrix', '--', '--ignored', '--nocapture', '--test-threads=1']
    for level in levels:
        env['BLOXORZ_START_LEVEL'] = str(level)
        for mode in ('compile', 'run'):
            env['BLOXORZ_CPU_MATRIX_MODE'] = mode
            print(f'Level {level}: {mode}', flush=True)
            with (output / f'level{level}-{mode}.log').open('wb') as log:
                completed = subprocess.run(command, cwd=ROOT, env=env, stdout=log,
                                           stderr=subprocess.STDOUT, timeout=900)
            if completed.returncode:
                raise SystemExit(f'Profile failed; see {output / f"level{level}-{mode}.log"}')
        result = json.loads((output / f'matrix-{args.label}-level{level}/cpu-profile.json').read_text())
        first = result['frames'][0]['end'] / 54000
        warm = sum(frame['cpu_ms'] for frame in result['frames'][2:6]) / 4
        print(f'Level {level}: init {first:.3f} ms; warm idle {warm:.3f} ms')
    print(f'CPU at 54 MHz; functional GPU, no physical GPU/scanout timing. Results: {output}')


if __name__ == '__main__':
    main()
