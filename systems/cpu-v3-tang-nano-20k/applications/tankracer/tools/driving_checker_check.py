"""Exercise driving comparison failures with single-word answer mutations."""
import argparse
import hashlib
import json
from pathlib import Path
import re
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
    if args.output.exists():
        raise ValueError('Fresh output required')
    artifact = Artifact(args.artifact)
    assets = args.artifact / 'tankracer-driving-assets'
    package = (args.artifact / artifact.manifest['package']).read_bytes()
    values = artifact.constants['DRIVING_STATE_VALUES']['value']
    mutations = [
        ('first-car-cache-boundary', 'frames-0.bin', 60 + 64, 0x8000 + 64),
        ('second-car-cache-boundary', 'frames-0.bin', 60 + 3566 + 64, 0x8000 + 3566 + 64),
        ('seed-final-partial-block', 'frames-0.bin', 60 + values * 2 + 1, 0x7f05),
    ]
    files = json.loads((assets / 'assets.json').read_text())['resources']
    surfaces = [r['file'] for r in files if re.fullmatch(r'surfaces-\d+\.bin', r['file'])]
    surfaces.sort(key=lambda name: int(name[9:-4]))
    surface_values = artifact.constants['DRIVING_SURFACE_VALUES']['value']
    mutations.append(('surface-final-partial-block', surfaces[0], surface_values * 2 - 1,
                      0x8000 + (surface_values - 1) * 2))
    versions = [r['file'] for r in files if re.fullmatch(r'mesh-versions-\d+\.bin', r['file'])]
    versions.sort(key=lambda name: int(name[14:-4]))
    mutations.append(('shared-resource-revision', versions[0], 0, 0x8000))
    parents = [r['file'] for r in files if re.fullmatch(r'parents-\d+\.bin', r['file'])]
    parents.sort(key=lambda name: int(name[8:-4]))
    packet = (assets / parents[0]).read_bytes()
    parent_words = struct.unpack('<' + 'H' * (len(packet) // 2), packet)
    cursor = 3 + parent_words[2] + 1 + 7
    found_motion = False
    for _ in range(7):
        if parent_words[cursor + 1] == 0:
            cursor += 2
            continue
        faces = parent_words[cursor + 7]
        cursor += 8
        if faces:
            mutations.append(('parent-fragment-velocity', parents[0], cursor + 8,
                              0x8000 + cursor + 8))
            found_motion = True
            break
    if not found_motion:
        raise ValueError('An activated parent fragment is required')
    scenes = [r['file'] for r in files if re.fullmatch(r'scenery-\d+\.bin', r['file'])]
    scenes.sort(key=lambda name: int(name[8:-4]))
    found = False
    for name in scenes:
        data = (assets / name).read_bytes()
        words = struct.unpack('<' + 'H' * (len(data) // 2), data)
        position = 0
        while position < len(words):
            header = words[position:position + 24]
            if len(header) != 24 or header[:2] != (0x5346, 1) or header[2] < 24:
                raise ValueError('Invalid semantic scene packet')
            if header[12]:
                # The first changed row contains ID, 60 collider words and
                # 16 property words. Bounds start at property word ten.
                mutations.append(('changed-scene-property', name,
                                  position + header[11] + 71, 0x8000 + 70))
                found = True
                break
            position += header[2]
        if found:
            break
    if not found:
        raise ValueError('A changed scene row is required')
    args.output.mkdir(parents=True)
    results = []
    for tag, name, word, signal in mutations:
        source = (assets / name).read_bytes()
        start = package.find(source)
        if start < 0 or package.find(source, start + 1) >= 0 or word * 2 + 1 >= len(source):
            raise ValueError(f'Unique complete source payload required: {name}')
        changed = bytearray(package)
        changed[start + word * 2] ^= 1
        output = args.output / tag
        output.mkdir()
        image = output / 'mutated.cb06'
        image.write_bytes(changed)
        # This is an intentionally altered CB06, not a qualified artifact.
        # Execute the same code directly; only one source-answer bit differs.
        command = [str(args.runtime.resolve()), '--package', str(image.resolve()),
                   '--cpu-mode', 'instruction', '--frames', '4096', '--max-cpu-steps',
                   '300000000', '--out-dir', str((output / 'sim').resolve())]
        with (output / 'run.log').open('w') as log:
            subprocess.run(command, check=True, timeout=300, stdout=log, stderr=subprocess.STDOUT)
        summary = json.loads((output / 'sim/summary-instruction.json').read_text())
        if summary['halt_signal'] != signal or summary.get('cpu_fault') or summary['gpu_error']:
            raise ValueError(f'{tag}: expected {signal:#x}, got {summary}')
        results.append(dict(test=tag, source=name, source_word=word, halt_signal=signal,
                            mutated_package_sha256=hashlib.sha256(changed).hexdigest()))
    report = dict(passed=True, artifact_id=artifact.manifest['artifact_id'],
                  scope='Single-bit changes to frame/surface/world/resource/fragment answers; source code and game inputs unchanged',
                  tests=results)
    (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(dict(passed=True, tests=len(results))))


if __name__ == '__main__':
    main()
