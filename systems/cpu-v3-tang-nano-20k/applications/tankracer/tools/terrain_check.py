"""Verify a terrain artifact and run its bounded instruction or native qualification."""
import argparse
import hashlib
import json
import os
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
    parser.add_argument('--backend', choices=['sim', 'native'], default='sim')
    parser.add_argument('--runtime', type=Path, required=True)
    parser.add_argument('--budget', type=int, default=50000000)
    parser.add_argument('--seconds', type=int, default=300)
    args = parser.parse_args()
    if args.output.exists() or args.budget <= 0 or args.seconds <= 0:
        raise ValueError('Use a fresh output and positive execution limits')
    artifact = Artifact(args.artifact)
    description = json.loads((args.artifact / 'tankracer-terrain-assets/assets.json').read_text())
    count = artifact.constants['TERRAIN_QUERY_LIMIT']['value']
    count = min(count or description['queries'], description['queries'])
    args.output.mkdir(parents=True)
    runtime = args.output.resolve() / ('runtime.exe' if os.name == 'nt' else 'runtime')
    shutil.copyfile(args.runtime, runtime)
    names = ['TERRAIN_PHASE', 'TERRAIN_SAMPLE', 'TERRAIN_CHECKS', 'TERRAIN_FIELD', 'TERRAIN_SWITCHES']
    binding = dict(artifact_id=artifact.manifest['artifact_id'], package=artifact.manifest['package'],
                   package_sha256=hashlib.sha256(artifact.files[artifact.manifest['package']]).hexdigest(),
                   symbols={name: artifact.symbol(name, 'global') for name in names}, count=count,
                   output_words=description['output_words'],
                   runtime_sha256=hashlib.sha256(runtime.read_bytes()).hexdigest())
    (args.output / 'bindings.json').write_text(json.dumps(binding, indent=2) + '\n')
    if args.backend == 'native':
        command = [str(runtime), str(args.artifact.resolve()), str((args.output / 'bindings.json').resolve()),
                   str((args.output / 'native').resolve()), str(args.budget), str(args.seconds)]
    else:
        command = [str(runtime), '--artifact', str(args.artifact), '--package',
                   str(args.artifact / artifact.manifest['package']), '--cpu-mode', 'instruction',
                   '--max-cpu-steps', str(args.budget), '--frames', '4096',
                   '--out-dir', str(args.output / 'sim')]
    with (args.output / 'run.log').open('w') as log:
        subprocess.run(command, check=True, timeout=args.seconds + 10, stdout=log, stderr=subprocess.STDOUT)
    if args.backend == 'sim':
        result = json.loads((args.output / 'sim/summary-instruction.json').read_text())
        if result['halt_signal'] != 0x600d or result['cpu_fault'] or result['gpu_error']:
            raise ValueError('Terrain target validation failed: ' + str(result))
        print(json.dumps(dict(passed=True, records=count, checked_words=count * description['output_words'],
                              instruction_steps=result['cpu_step_calls'])))
    else:
        print((args.output / 'native/native-report.json').read_text())


if __name__ == '__main__':
    main()
