"""Check packed-map directory, cell bounds and query-answer failures."""
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
    assets = args.artifact / 'tankracer-terrain-assets'
    metadata = json.loads((assets / 'assets.json').read_text())
    if metadata['cells'] != list(range(64)):
        raise ValueError('complete packed-map artifact required')
    directory = struct.unpack('<64H', (assets / 'directory.bin').read_bytes())
    cell = (directory[0] & 32767) * 64
    tests = [
        ('baseline', 'directory.bin', 0, 0, 0x600d),
        ('directory-format', 'directory.bin', 0, 32768, 0x7c12),
        ('directory-range', 'directory.bin', 0, 4096, 0x7c12),
        ('cell-magic', 'cells.bin', cell, 1, 0x7c03),
        ('cell-crosses-page', 'cells.bin', cell + 3, 64, 0x7c03),
        ('wrong-cell', 'cells.bin', cell + 2, 1, 0x7c12),
        ('query-answer', 'queries.bin', 8, 1, 0x7c12),
    ]
    if cell % 16384 != 16320:
        raise ValueError('boundary fixture requires cell zero in the last sector')
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
        command = [str(args.runtime.resolve()), '--package', str(image.resolve()),
                   '--cpu-mode', 'instruction', '--frames', '4096',
                   '--max-cpu-steps', '100000000', '--out-dir', str((folder / 'sim').resolve())]
        with (folder / 'run.log').open('w') as log:
            subprocess.run(command, check=True, timeout=90, stdout=log, stderr=subprocess.STDOUT)
        result = json.loads((folder / 'sim/summary-instruction.json').read_text())
        signal = result['halt_signal']
        if result['cpu_fault'] or result['gpu_error'] or signal is None:
            raise ValueError(f'{label}: {result}')
        if signal != expected:
            raise ValueError(f'{label}: expected {expected:#x}, got {signal:#x}')
        results.append(dict(label=label, file=file, word=word, halt=signal))
    (args.output / 'report.json').write_text(json.dumps(dict(
        passed=True, artifact_id=artifact.manifest['artifact_id'], tests=results,
        scope='Packed directory, last-sector cell bounds, identity and query outputs.'), indent=2) + '\n')
    print(json.dumps(dict(passed=True, checks=len(results))))


if __name__ == '__main__':
    main()
