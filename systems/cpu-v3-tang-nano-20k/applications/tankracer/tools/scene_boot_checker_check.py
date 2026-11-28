"""Check first-map construction overwrites snapshots and consumes authored inputs."""
import argparse
import json
from pathlib import Path
import struct
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'tools'))
from application_artifact import Artifact
from scenery_loading_check import resource_offset


def words(path):
    data = path.read_bytes()
    return struct.unpack('<' + 'H' * (len(data) // 2), data)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('artifact', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--runtime', type=Path, required=True)
    args = parser.parse_args()
    artifact = Artifact(args.artifact)
    assets = args.artifact / 'tankracer-driving-assets'
    manifest = json.loads((assets / 'assets.json').read_text())
    if len(manifest['cases']) != 1 or manifest['cases'][0]['name'] != 'complete-player-world-cold':
        raise ValueError('one complete cold-scene case required')
    live = 'scenery-initial-complete-player-world-cold.bin'
    h = words(assets / live)
    context = words(assets / 'player-context.bin')
    ambient = context[38] + context[56]
    smoke = ambient + 6 + context[ambient + 2] * 10
    debris = words(assets / 'debris.bin')
    deform = words(assets / 'deform-bound.bin')
    boot = words(assets / 'scene-boot.bin')
    directory = []
    cursor = boot[4]
    for object_id in range(boot[2]):
        if boot[cursor] != object_id:
            raise ValueError('authored constructor row order')
        directory.append(cursor)
        cursor += 5 + boot[cursor + 2] + boot[cursor + 3]
    if cursor != boot[5]:
        raise ValueError('authored constructor extent')
    # Each row names the owner and a real field, not a padding byte. Snapshot
    # mutations must be overwritten before the pre-frame scene comparison.
    tests = [
        ('baseline', 'scene-boot.bin', 0, 0, True, 0x600d),
        ('constructed-shield', 'complete-initial-0.bin', 9086 + 6, 1, True, None),
        ('constructed-flock-flight', 'flock-initial.bin', 16 + 4, 1, True, None),
        ('constructed-flock-bird', 'flock-initial.bin', 16 + 4 + 64, 1, True, None),
        ('constructed-emitter-timer', 'player-context.bin', context[38] + context[54] + 1, 1, True, None),
        ('constructed-train-travel', 'player-context.bin', context[38] + context[7] + 2, 1, True, None),
        ('constructed-train-path', 'player-context.bin', context[38] + context[7] + 2 + 42, 1, True, None),
        ('constructed-pickup-flight', 'player-pickup-flights.bin', 0, 1, True, None),
        ('constructed-ambient-selection', 'player-context.bin', ambient + 4, 1, True, None),
        ('constructed-smoke-timer', 'player-context.bin', smoke + 4, 1, True, None),
        ('constructed-smoke-position', 'player-context.bin', smoke + 10 + 14, 1, True, None),
        ('constructed-debris-basis', 'debris.bin', debris[7] + debris[18] + 12, 1, True, None),
        ('constructed-crush-flags', 'deform-bound.bin', deform[4], 1, True, None),
        ('constructed-closer', live, h[6] + h[h[4] + 172 * 8 + 1], 1, True, None),
        ('constructed-cell', live, h[4] + 75 * 8 + 6, 1, True, None),
        ('constructed-tick-flag', live, h[10] + 76 * 2, 16, True, None),
        ('authored-lap', 'scene-boot.bin', directory[75] + 5 + 4, 3, False, None),
        ('authored-debris-pose', 'scene-boot.bin', directory[169] + 5 + 26, 4096, False, None),
        ('authored-camera', 'scene-boot.bin', boot[5] + 18, 4096, False, None),
        ('camera-answer', 'scene-boot-camera.bin', 18, 1, False, None),
        ('initial-scene-answer', 'scene-boot-expected.bin', 15, 1, False, None),
        ('initial-debris-answer', 'scene-boot-debris.bin', 21, 1, False, None),
        ('invalid-row-kind', 'scene-boot.bin', directory[0] + 5, 1, False, 0x7fa0),
        ('invalid-row-size', 'scene-boot.bin', directory[0] + 2, 256, False, 0x7fa0),
        ('invalid-debris-count', 'scene-boot.bin', directory[169] + 5 + 34, 16, False, 0x7fa0),
        ('invalid-crush-count', 'scene-boot.bin', directory[124] + 5 + 1, 1, False, 0x7fa0),
        ('invalid-camera-offset', 'scene-boot.bin', 5, 1, False, 0x7fa0),
    ]
    package = artifact.files[artifact.manifest['package']]
    args.output.mkdir(parents=True, exist_ok=False)
    results = []
    for label, file, word, mask, passed, expected_signal in tests:
        payload = (assets / file).read_bytes()
        if word * 2 + 2 > len(payload):
            raise ValueError(f'{label}: field outside {file}')
        offset = resource_offset(package, payload)
        changed = bytearray(package)
        value = struct.unpack_from('<H', changed, offset + word * 2)[0]
        struct.pack_into('<H', changed, offset + word * 2, value ^ mask)
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
        if summary['cpu_fault'] or summary['gpu_error'] or signal is None:
            raise ValueError(f'{label}: {summary}')
        if (signal == 0x600d) != passed or (expected_signal is not None and signal != expected_signal):
            raise ValueError(f'{label}: unexpected halt {signal:#x}')
        if not passed and expected_signal is None and signal < 0x8000:
            raise ValueError(f'{label}: expected a result difference, got {signal:#x}')
        results.append(dict(label=label, file=file, word=word, expected_pass=passed, halt=signal))
    report = dict(passed=True, artifact_id=artifact.manifest['artifact_id'], tests=results,
                  scope='First-map randomized fields, actor state, camera and memberships are constructed '
                        'before the initial comparison. Immutable geometry and resource metadata remain prepared assets.')
    (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(dict(passed=True, perturbations=len(results))))


if __name__ == '__main__':
    main()
