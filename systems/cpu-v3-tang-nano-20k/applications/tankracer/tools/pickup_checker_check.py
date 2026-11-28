"""Verify pickup comparison failures with single-bit answer changes."""
import argparse
import hashlib
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
    if args.output.exists():
        raise ValueError('Fresh output required')
    artifact = Artifact(args.artifact)
    assets = args.artifact / 'tankracer-driving-assets'
    files = json.loads((assets / 'assets.json').read_text())['resources']
    source = next(r['file'] for r in files
                  if r['file'].startswith('pickup-') and r['file'][7:-4].isdigit())
    blob = (assets / source).read_bytes()
    package = (args.artifact / artifact.manifest['package']).read_bytes()
    if package.count(blob) != 1 or len(blob) < 10040 * 4:
        raise ValueError('Unique pickup answers covering collection required')
    start = package.index(blob)
    args.output.mkdir(parents=True)
    results = []
    for name, word, field in [('flight-state', 0, 0),
                              ('collected-inventory', 10040 + 1620, 1620),
                              ('allocated-quad-position', 10040 + 3264, 3264)]:
        folder = args.output / name
        folder.mkdir()
        changed = bytearray(package)
        changed[start + word * 2] ^= 1
        image = folder / 'mutated.cb06'
        image.write_bytes(changed)
        command = [str(args.runtime.resolve()), '--package', str(image.resolve()),
                   '--cpu-mode', 'instruction', '--frames', '4096', '--max-cpu-steps',
                   '80000000', '--out-dir', str((folder / 'sim').resolve())]
        with (folder / 'run.log').open('w') as log:
            subprocess.run(command, check=True, timeout=60, stdout=log,
                           stderr=subprocess.STDOUT)
        summary = json.loads((folder / 'sim/summary-instruction.json').read_text())
        signal = 0x8000 + field
        if summary['halt_signal'] != signal or summary['cpu_fault'] or summary['gpu_error']:
            raise ValueError(f'{name}: expected {signal:#x}, got {summary}')
        results.append(dict(test=name, source=source, source_word=word,
                            halt_signal=signal,
                            mutated_package_sha256=hashlib.sha256(changed).hexdigest()))
    report = dict(passed=True, artifact_id=artifact.manifest['artifact_id'], tests=results,
                  scope='Pickup answer bit changes; code and game inputs unchanged')
    (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(dict(passed=True, tests=len(results))))


if __name__ == '__main__':
    main()
