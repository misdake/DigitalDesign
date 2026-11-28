"""Verify ordinary scene retirement with bounded instruction or native execution."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'tools'))
from application_artifact import Artifact


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('artifact', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--runtime', type=Path, required=True)
    parser.add_argument('--backend', choices=['instruction', 'native'], default='instruction')
    parser.add_argument('--budget', type=int, default=50000000)
    parser.add_argument('--seconds', type=int, default=300)
    args = parser.parse_args()
    if args.output.exists() or args.budget <= 0 or args.seconds <= 0:
        raise ValueError('Fresh output and positive execution limits required')
    artifact = Artifact(args.artifact)
    data = json.loads((args.artifact / 'tankracer-retirement-assets/assets.json').read_text())['metadata']
    args.output.mkdir(parents=True)
    runtime = args.output.resolve() / args.runtime.name
    shutil.copyfile(args.runtime, runtime)
    budget_flag = '--max-cpu-steps' if args.backend == 'instruction' else '--max-cpu-cycles'
    command = [str(runtime), '--artifact', str(args.artifact.resolve()), '--package',
               str((args.artifact / artifact.manifest['package']).resolve()), '--cpu-mode', args.backend,
               budget_flag, str(args.budget), '--frames', '4096', '--out-dir', str(args.output / 'sim')]
    with (args.output / 'run.log').open('w') as log:
        subprocess.run(command, check=True, timeout=args.seconds, stdout=log, stderr=subprocess.STDOUT)
    name = 'summary-instruction.json' if args.backend == 'instruction' else 'summary-native.json'
    result = json.loads((args.output / 'sim' / name).read_text())
    diagnostic = json.loads((args.output / 'sim/diagnostic.json').read_text())
    if diagnostic['signal'] != 0x600d or result['cpu_fault'] or result['gpu_error']:
        raise ValueError(f'Retirement failed: {diagnostic}')
    report = dict(passed=True, backend=args.backend, cases=data['cases'], checked_i32_values=data['checked_i32_values'],
                  artifact_id=artifact.manifest['artifact_id'], scope=data['scope'], runtime_summary=result)
    (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(dict(passed=True, backend=args.backend, cases=len(data['cases']), checked_i32_values=data['checked_i32_values'])))


if __name__ == '__main__':
    main()
