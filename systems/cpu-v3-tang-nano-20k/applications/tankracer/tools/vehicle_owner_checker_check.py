"""Verify that racer slots, vehicle owners and presence use independent indices."""
import argparse
import json
from pathlib import Path
import struct
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'tools'))
from application_artifact import Artifact
from scenery_loading_check import resource_offset


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('artifact', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--runtime', type=Path, required=True)
    args = parser.parse_args()
    artifact = Artifact(args.artifact)
    assets = args.artifact / 'tankracer-driving-assets'
    metadata = json.loads((assets / 'assets.json').read_text())
    if len(metadata['cases']) != 1 or metadata['cases'][0]['name'] != 'complete-slot-gap':
        raise ValueError('single complete-slot-gap case required')
    case = metadata['cases'][0]
    if case['tank_contacts'] != 0 or case['updates'] < 2:
        raise ValueError('both vehicles must update without an earlier collision ending the slot scan')
    bindings = (assets / 'car-bindings.bin').read_bytes()
    slots = (assets / 'cases.bin').read_bytes()
    if struct.unpack_from('<H', bindings, 21 * 2)[0] != 4:
        raise ValueError('peer owner 1 must retain racer slot 4')
    if struct.unpack_from('<H', slots, 11 * 2)[0] != 1:
        raise ValueError('racer slot 4 must resolve to owner 1')
    tests = [
        ('baseline', 'car-bindings.bin', 21, 4, 0x600d),
        ('owner-used-as-racer-slot', 'car-bindings.bin', 21, 1, 0x7f0a),
        ('racer-slot-used-as-owner', 'cases.bin', 11, 4, 0x7f0a),
        ('missing-peer-presence', 'cases.bin', 6, 0, 0x7f0a),
    ]
    package = artifact.files[artifact.manifest['package']]
    args.output.mkdir(parents=True, exist_ok=False)
    results = []
    for label, file, word, value, expected in tests:
        payload = (assets / file).read_bytes()
        offset = resource_offset(package, payload)
        changed = bytearray(package)
        struct.pack_into('<H', changed, offset + word * 2, value)
        folder = args.output / label
        folder.mkdir()
        image = folder / 'changed.cb06'
        image.write_bytes(changed)
        with (folder / 'run.log').open('w') as log:
            subprocess.run([str(args.runtime.resolve()), '--package', str(image.resolve()),
                '--cpu-mode', 'instruction', '--frames', '4096', '--max-cpu-steps',
                '80000000', '--out-dir', str((folder / 'sim').resolve())],
                check=True, timeout=60, stdout=log, stderr=subprocess.STDOUT)
        result = json.loads((folder / 'sim/summary-instruction.json').read_text())
        if result['cpu_fault'] or result['gpu_error'] or result['halt_signal'] != expected:
            raise ValueError(f'{label}: expected halt {expected:#x}, got {result}')
        results.append(dict(name=label, halt=expected, passed=True))
    report = dict(passed=True, artifact_id=artifact.manifest['artifact_id'], tests=results)
    (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
