"""Prove cold driving uses model/spawn metadata instead of initial car answers."""
import argparse
import json
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'tools'))
from application_artifact import Artifact


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('artifact', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--runtime', type=Path, required=True)
    args = parser.parse_args()
    artifact = Artifact(args.artifact)
    assets = args.artifact / 'tankracer-driving-assets'
    manifest = json.loads((assets / 'assets.json').read_text())
    if manifest['qualification_scope'] != 'startup':
        raise ValueError('startup-only artifact required')
    package = artifact.files[artifact.manifest['package']]
    args.output.mkdir(parents=True, exist_ok=False)
    results = []
    tests = [('unused-physical-position', 'initial.bin', 184, 1, True),
             ('unused-gun-vertices', 'initial.bin', 542, 1, True),
             ('unused-prefix', 'prefix-initial-0.bin', 0, 1, True),
             ('unused-initial-pose', 'complete-initial-0.bin', 6810, 1, True),
             ('authored-spawn', 'startup-models.bin', 120, 4096, False)]
    for label, file, word, mask, passed in tests:
        data = (assets / file).read_bytes()
        offset = package.find(data)
        if offset < 0 or package.find(data, offset + 1) >= 0:
            raise ValueError(f'unique payload required: {file}')
        changed = bytearray(package)
        changed[offset + word * 2] ^= mask & 255
        changed[offset + word * 2 + 1] ^= mask >> 8
        folder = args.output / label
        folder.mkdir()
        image = folder / 'changed.cb06'
        image.write_bytes(changed)
        command = [str(args.runtime.resolve()), '--package', str(image.resolve()),
                   '--cpu-mode', 'instruction', '--frames', '4096',
                   '--max-cpu-steps', '300000000', '--out-dir', str((folder / 'sim').resolve())]
        with (folder / 'run.log').open('w') as log:
            subprocess.run(command, check=True, timeout=90, stdout=log, stderr=subprocess.STDOUT)
        summary = json.loads((folder / 'sim/summary-instruction.json').read_text())
        signal = summary['halt_signal']
        if summary['cpu_fault'] or summary['gpu_error'] or (signal == 0x600d) != passed:
            raise ValueError(f'{label}: {summary}')
        if not passed and not (signal is not None and signal >= 0x8000):
            raise ValueError(f'{label}: expected a result difference, got {summary}')
        results.append(dict(label=label, file=file, word=word, expected_pass=passed, halt=signal))
    (args.output / 'report.json').write_text(json.dumps(dict(
        passed=True, artifact_id=artifact.manifest['artifact_id'], tests=results,
        scope='Four initial-answer mutations leave driving unchanged; changing the independent authored spawn changes the computed result.'), indent=2) + '\n')


if __name__ == '__main__':
    main()
