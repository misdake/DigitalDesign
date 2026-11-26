"""Check sampled controls feed physics from live fields and independent keys."""
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
    if manifest['qualification_scope'] != 'fast-controls' or manifest['updates'] != 4:
        raise ValueError('two-frame fast-controls artifact required')
    complete_words = (assets / 'complete-0.bin').stat().st_size // 2 // 4
    tests = [
        ('derived-throttle', 'frames-0.bin', 38, 1, True),
        ('derived-steering', 'frames-0.bin', 42, 1, True),
        ('derived-alternate', 'frames-0.bin', 46, 1, True),
        ('replaced-saved-turn', 'complete-initial-0.bin', 10418, 1, True),
        ('sampled-auxiliary-key', 'frames-0.bin', 58, 16, False),
        ('live-model-turn', 'complete-initial-0.bin', 10420, 1, False),
        ('stored-surface-override', 'complete-initial-0.bin', 10370, 1, False),
        ('signed-turret-angle', 'complete-0.bin', 7723, 1, False),
        ('reverse-braking', 'complete-0.bin', 7758, 1, False),
        ('gradual-restoration', 'complete-0.bin', 2 * complete_words + 10370, 1, False),
    ]
    package = artifact.files[artifact.manifest['package']]
    args.output.mkdir(parents=True, exist_ok=False)
    results = []
    for label, file, word, mask, passed in tests:
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
        if summary['cpu_fault'] or summary['gpu_error'] or (signal == 0x600d) != passed:
            raise ValueError(f'{label}: {summary}')
        if not passed and (signal is None or signal < 0x8000):
            raise ValueError(f'{label}: expected a computed result difference, got {summary}')
        results.append(dict(label=label, file=file, word=word, expected_pass=passed, halt=signal))
    (args.output / 'report.json').write_text(json.dumps(dict(
        passed=True, artifact_id=artifact.manifest['artifact_id'], tests=results,
        scope='Derived frame input is replaced by sampled live state; changing keys, model state or answers is detected.'), indent=2) + '\n')


if __name__ == '__main__':
    main()
