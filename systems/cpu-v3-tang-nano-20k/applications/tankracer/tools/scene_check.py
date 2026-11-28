"""Verify initial scene records and ordered queries through bounded target execution."""
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
    parser.add_argument('--budget', type=int, default=150000000)
    parser.add_argument('--seconds', type=int, default=600)
    args = parser.parse_args()
    if args.output.exists() or args.budget <= 0 or args.seconds <= 0:
        raise ValueError('Fresh output and positive execution limits required')
    artifact = Artifact(args.artifact)
    data = json.loads((args.artifact / 'tankracer-scene-assets/assets.json').read_text())['metadata']
    limit = artifact.constants['SCENE_QUERY_LIMIT']['value']
    count = min(limit or data['query_count'], data['query_count'])
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
        raise ValueError(f'Scene failed: {diagnostic}')
    report = dict(passed=True, backend=args.backend, objects=data['object_count'], queries=count,
                  record_checked_words=data['object_count'] * data['record_words'],
                  live_collider_checked_words=data['object_count'] * 60,
                  live_property_checked_words=data['object_count'] * 16,
                  live_membership_rows=64,
                  real_fall_contacts=data['hit_count'],
                  contact_checked_i32_values=data['hit_count'] * 286,
                  real_fall_ticks=data['fall_tick_count'],
                  fall_tick_checked_i32_values=data['fall_tick_count'] * 687,
                  real_bounce_updates=data['bounce_update_count'],
                  bounce_checked_i32_values=data['bounce_update_count'] * 1225,
                  query_checked_i32_values=count * 23, artifact_id=artifact.manifest['artifact_id'],
                  scope=data['scope'], runtime_summary=result)
    (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({k: report[k] for k in ['passed', 'backend', 'objects', 'queries',
                                           'real_fall_contacts', 'real_fall_ticks', 'real_bounce_updates', 'query_checked_i32_values',
                                           'contact_checked_i32_values', 'fall_tick_checked_i32_values']}))


if __name__ == '__main__':
    main()
