"""Verify bounded driving checks retain impact, identity and scene coverage."""
import argparse
import json
from pathlib import Path
import struct
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
    if manifest['qualification_scope'] != 'complete':
        raise ValueError('complete driving artifact required')
    scene_case = next(i for i, c in enumerate(manifest['cases']) if c['scenery_ticks'] > 0)
    scene_file = f'scenery-{scene_case}.bin'
    header = struct.unpack('<24H', (assets / scene_file).read_bytes()[:48])
    tests = [
        ('line-pool', 'frames-0.bin', 60 + 606, None),
        ('line-overflow', 'frames-0.bin', 60 + 2206, None),
        ('line-exhausted', 'frames-0.bin', 60 + 2222, None),
        ('batch-header', 'frames-0.bin', 60 + 2238, None),
        ('first-particle', 'frames-0.bin', 60 + 2250, None),
        ('last-particle-color', 'frames-0.bin', 60 + 3164, None),
        ('identity-row', scene_file, header[5] + 12, None),
        ('scene-row', scene_file, header[4] + 7, 0x7f26),
    ]
    package = artifact.files[artifact.manifest['package']]
    args.output.mkdir(parents=True, exist_ok=False)
    results = []
    for label, file, word, expected_signal in tests:
        payload = (assets / file).read_bytes()
        offset = package.find(payload)
        if offset < 0 or package.find(payload, offset + 1) >= 0:
            raise ValueError(f'unique payload required: {file}')
        changed = bytearray(package)
        changed[offset + word * 2] ^= 1
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
        rejected = signal is not None and (signal >= 0x8000 if expected_signal is None else signal == expected_signal)
        if summary['cpu_fault'] or summary['gpu_error'] or not rejected:
            raise ValueError(f'{label}: expected a result difference, got {summary}')
        results.append(dict(label=label, file=file, word=word, rejected=True, halt=signal))
    (args.output / 'report.json').write_text(json.dumps(dict(
        passed=True, artifact_id=artifact.manifest['artifact_id'], tests=results), indent=2) + '\n')


if __name__ == '__main__':
    main()
