"""Check branch selection and state assertions in continuous vehicle driving."""
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
    if manifest['qualification_scope'] != 'visibility' or manifest['updates'] != 8:
        raise ValueError('four-frame two-car visibility artifact required')
    package = artifact.files[artifact.manifest['package']]
    frame_words = (assets / 'frames-0.bin').stat().st_size // 2 // 8
    complete_words = (assets / 'complete-0.bin').stat().st_size // 2 // 8
    tests = [
        ('enter-middle-distance', 'frames-0.bin', 2 * frame_words + 54, 1, None),
        ('resume-visible', 'frames-0.bin', 4 * frame_words + 54, 1, None),
        ('middle-distance-height', 'complete-0.bin', 2 * complete_words + 10358, 1, None),
        ('middle-distance-rng', 'frames-0.bin', 3 * frame_words - 2, 1, 0x7f05),
    ]
    args.output.mkdir(parents=True, exist_ok=False)
    results = []
    for label, file, word, mask, expected_signal in tests:
        payload = (assets / file).read_bytes()
        offset = package.find(payload)
        if offset < 0 or package.find(payload, offset + 1) >= 0:
            raise ValueError(f'unique payload required: {file}')
        changed = bytearray(package)
        changed[offset + word * 2] ^= mask
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
