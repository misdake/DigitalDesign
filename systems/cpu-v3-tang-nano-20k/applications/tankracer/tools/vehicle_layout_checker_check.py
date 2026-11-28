"""Check relocated vehicle, line, alias and impact states against source answers."""
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
    cold = [i for i, case in enumerate(metadata['cases'])
            if case['name'] == 'complete-player-world-cold']
    if len(cold) != 1:
        raise ValueError('one cold-world case required')
    resources = {r['symbol']: r['file'] for r in metadata['resources']}
    aliases = metadata['resource_aliases']
    def resource(symbol):
        while symbol in aliases:
            symbol = aliases[symbol]
        return resources[symbol]
    complete = resource(f'DRIVING_COMPLETE_{cold[0]}')
    frames = resource(f'DRIVING_FRAME_{cold[0]}')
    # These are the original reference snapshot's fields. Target placement is
    # deliberately absent: comparisons must reach the actual relocated owners.
    tests = [
        ('baseline', complete, 6810, 0, 0x600d),
        ('first-vehicle', complete, 6810, 1, 0x8000 + 6810),
        ('second-vehicle', complete, 7766, 1, 0x8000 + 7766),
        # The main frame record has 60 input words before its source snapshot.
        ('line-pool', frames, 60 + 606, 1, 0x8000 + 606),
        ('line-quad-alias', frames, 60 + 2222, 1, 0x8000 + 2222),
        ('impact-batch', frames, 60 + 2238, 1, 0x8000 + 2238),
        ('inactive-input', complete, 10358, 1, 0x8000 + 10358),
        ('player-control', complete, 10414, 1, 0x8000 + 10414),
        ('body-vertex', frames, 60 + 1596 * 2, 1, 0x8000 + 1596 * 2),
    ]
    if 'DRIVING_FINISH_SCENE' in resources:
        route_file = resource('DRIVING_FINISH_SCENE')
        route = (assets / route_file).read_bytes()
        header = struct.unpack_from('<16H', route)
        indices = struct.unpack_from('<' + 'H' * header[3], route, header[5] * 2)
        first = next(i for i, value in enumerate(indices) if value != 65535)
        tests += [
            ('finish-format', route_file, 1, 1, 0x7f90),
            ('finish-row-range', route_file, header[5] + first, 32768, 0x7f90),
            ('finish-missing-point', route_file, header[5] + first,
             indices[first] ^ 65535, 0x7f91),
            ('finish-point-presence', route_file, header[6], 1, 0x7f91),
        ]
    if 'DRIVING_FINISH_TRANSFER' in resources:
        transfer_file = resource('DRIVING_FINISH_TRANSFER')
        h = struct.unpack_from('<32H', (assets / transfer_file).read_bytes())
        tests += [
            ('finish-clone-motion', transfer_file, h[16] + 178, 1, 0x8000 + h[16] + 178),
            ('finish-clone-light', transfer_file, h[17] + 832, 1, 0x8000 + h[17] + 832),
            ('finish-private-geometry', transfer_file, h[24] + 4, 1, 0x8000 + h[24] + 4),
            ('finish-saved-turn', transfer_file, h[26] + 34, 1, 0x8000 + h[26] + 34),
            ('finish-ai-checkpoint', transfer_file, h[27] + 16, 1, 0x8000 + h[27] + 16),
            ('finish-active-pickup-target', transfer_file, h[28] + 10, 1, 0x8000 + 10),
            ('finish-inactive-pickup-target', transfer_file, h[28] + 826, 1, 0x8000 + 826),
        ]
    if 'DRIVING_FINISH_RUN_4' in resources:
        run_file = resource('DRIVING_FINISH_RUN_4')
        h = struct.unpack_from('<32H', (assets / run_file).read_bytes())
        tests += [
            ('finish-running-motion', run_file, h[16] + 178, 1, 0x8000 + h[16] + 178),
            ('finish-running-ai', run_file, h[27] + 4, 1, 0x8000 + h[27] + 4),
            ('finish-running-horn', run_file, h[21] + 2, 1, 0x8000 + h[21] + 2),
        ]
    package = artifact.files[artifact.manifest['package']]
    args.output.mkdir(parents=True, exist_ok=False)
    results = []
    for label, file, word, mask, expected in tests:
        payload = (assets / file).read_bytes()
        offset = resource_offset(package, payload)
        changed = bytearray(package)
        value = struct.unpack_from('<H', changed, offset + word * 2)[0]
        struct.pack_into('<H', changed, offset + word * 2, value ^ mask)
        folder = args.output / label
        folder.mkdir()
        image = folder / 'changed.cb06'
        image.write_bytes(changed)
        with (folder / 'run.log').open('w') as log:
            subprocess.run([str(args.runtime.resolve()), '--package', str(image.resolve()),
                '--cpu-mode', 'instruction', '--frames', '4096', '--max-cpu-steps',
                '180000000', '--out-dir', str((folder / 'sim').resolve())],
                check=True, timeout=60, stdout=log, stderr=subprocess.STDOUT)
        result = json.loads((folder / 'sim/summary-instruction.json').read_text())
        if result['cpu_fault'] or result['gpu_error'] or result['halt_signal'] != expected:
            raise ValueError(f'{label}: expected {expected:#x}, got {result}')
        results.append(dict(label=label, file=file, word=word, halt=expected))
    (args.output / 'report.json').write_text(json.dumps(dict(passed=True,
        artifact_id=artifact.manifest['artifact_id'], tests=results), indent=2) + '\n')
    print(json.dumps(dict(passed=True, checks=len(results))))


if __name__ == '__main__':
    main()
