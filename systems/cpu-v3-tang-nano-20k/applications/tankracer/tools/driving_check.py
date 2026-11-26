"""Run a source-verified driving artifact with bounded instruction or native CPU execution."""
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
    parser.add_argument('--budget', type=int, default=300000000)
    parser.add_argument('--seconds', type=int, default=300)
    args = parser.parse_args()
    if args.output.exists() or args.budget <= 0 or args.seconds <= 0:
        raise ValueError('fresh output and positive execution limits required')
    artifact = Artifact(args.artifact)
    assets = json.loads((args.artifact / 'tankracer-driving-assets/assets.json').read_text())
    steps = artifact.constants['DRIVING_STEPS']['value']
    if steps != assets['steps']:
        raise ValueError('driving source constant and oracle step count disagree')
    args.output.mkdir(parents=True)
    runtime = args.output.resolve() / args.runtime.name
    shutil.copyfile(args.runtime, runtime)
    budget_flag = '--max-cpu-steps' if args.backend == 'instruction' else '--max-cpu-cycles'
    command = [str(runtime), '--artifact', str(args.artifact.resolve()), '--package',
               str((args.artifact / artifact.manifest['package']).resolve()), '--cpu-mode', args.backend,
               budget_flag, str(args.budget), '--frames', '4096', '--out-dir', str(args.output / 'sim')]
    with (args.output / 'run.log').open('w') as log:
        subprocess.run(command, check=True, timeout=args.seconds, stdout=log, stderr=subprocess.STDOUT)
    summary_name = 'summary-instruction.json' if args.backend == 'instruction' else 'summary-native.json'
    result = json.loads((args.output / 'sim' / summary_name).read_text())
    diagnostic = json.loads((args.output / 'sim/diagnostic.json').read_text())
    if diagnostic['signal'] != 0x600d or result['cpu_fault'] or result['gpu_error']:
        raise ValueError(f'driving failed: {diagnostic}')
    report = dict(passed=True, backend=args.backend, cases=assets['cases'], steps_per_case=steps,
                  checked_i32_values=assets['updates'] * assets['checked_i32_per_frame'] + assets.get('checked_surface_i32', 0), startup_noise_words=16,
                  resource_version_words=assets.get('resource_version_words', 0),
                  checked_parent_words=assets.get('checked_parent_words', 0),
                  checked_player_words=assets.get('checked_player_words', 0),
                  checked_complete_words=assets.get('checked_complete_words',0), checked_pickup_words=assets.get('checked_pickup_words',0), checked_debris_words=assets.get('checked_debris_words', 0), checked_prefix_words=assets.get('checked_prefix_words',0), qualification_scope=assets.get('qualification_scope','full'),
                  scenery_frames=assets.get('scenery_frames', 0), checked_scenery_words=assets.get('checked_scenery_words', 0),
                  artifact_id=artifact.manifest['artifact_id'], scope=assets['scope'], runtime_summary=result)
    (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({k: report[k] for k in ['passed', 'backend', 'steps_per_case', 'checked_i32_values']}))


if __name__ == '__main__':
    main()
