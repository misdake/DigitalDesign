"""Check canonical identity ownership and compact scenery loading on a full driving artifact."""
import argparse
import json
from pathlib import Path
import struct
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'tools'))
from application_artifact import Artifact


def resource_offset(package, payload):
    cursor = 4
    matches = []
    while cursor + 8 <= len(package):
        _, control = struct.unpack_from('<II', package, cursor)
        cursor += 8
        length = control & 0x7fffffff
        if length == 0:
            break
        if control >> 31 == 0:
            data = package[cursor:cursor + length]
            if data.startswith(payload) and not any(data[len(payload):]):
                matches.append(cursor)
            cursor += length
    if len(matches) != 1:
        raise ValueError('one complete COPY resource required')
    return matches[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('artifact', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--runtime', type=Path, required=True)
    parser.add_argument('--player', action='store_true')
    parser.add_argument('--clock', action='store_true')
    parser.add_argument('--tail', action='store_true')
    args = parser.parse_args()
    artifact = Artifact(args.artifact)
    assets = args.artifact / 'tankracer-driving-assets'
    manifest = json.loads((assets / 'assets.json').read_text())
    if manifest['qualification_scope'] not in ['complete', 'full', 'player', 'clock', 'tail']:
        raise ValueError('complete or full driving artifact required')
    image = (assets / 'scenery.bin').read_bytes()
    header = struct.unpack('<48H', image[:96])
    identities = (assets / 'identity.bin').read_bytes()
    lifetime_words = (assets / 'lifetime.bin').stat().st_size // 2
    identity_words = struct.unpack('<' + 'H' * (len(identities) // 2), identities)
    obstacle = 75
    entity = next(i for i in range((len(identity_words) - 12) // 6)
                  if identity_words[12 + i * 6] & 31 == 7
                  and identity_words[14 + i * 6:16 + i * 6] == (obstacle, 0))
    tests = [
        ('class-snapshot', 'scenery.bin', header[12], 1, True, None),
        ('object-snapshot', 'scenery.bin', header[14], 1, True, None),
        ('component-presence', 'scenery.bin', header[4] + obstacle * 8 + 7, 1, False, None),
        ('canonical-class', 'identity.bin', 13 + entity * 6, 1, False, None),
        ('component-high-word', 'identity.bin', 15 + entity * 6, 1, False, 0x7f5b),
        ('component-range', 'identity.bin', 14 + entity * 6, 512, False, 0x7f5b),
        ('free-link-high-word', 'identity.bin', 17 + entity * 6, 1, False, 0x7f5b),
        ('free-link-range', 'identity.bin', 16 + entity * 6, 1024, False, 0x7f5b),
        ('tick-flags-high-word', 'scenery.bin', header[10] + 1, 1, False, 0x7f11),
        ('tick-flags-section-count', 'scenery.bin', 11, 1, False, 0x7f11),
        ('section-offset', 'scenery.bin', 4, 1, False, 0x7f11),
        ('flag-count', 'scenery.bin', 3, 512, False, 0x7f11),
        ('section-length', 'scenery.bin', 5, 32768, False, 0x7f11),
        ('lifetime-range', 'lifetime.bin', 2, 240, False, 0x7f22),
        ('lap-membership-capacity', 'lifetime.bin', 3, 128, False, 0x7f22),
        ('truncated-final-list', 'lifetime.bin', lifetime_words - 1, 1, False, 0x7f22),
    ]
    if args.player:
        if manifest['qualification_scope'] != 'player':
            raise ValueError('player perturbations require the player-only artifact')
        context_words = struct.unpack('<56H', (assets / 'player-context.bin').read_bytes()[:112])
        initial_words = struct.unpack_from('<H', (assets / 'player-0.bin').read_bytes())[0]
        tests = [
            ('fire-key', 'frames-0.bin', 58, 128, False, None),
            ('gun-position', 'complete-initial-0.bin', 6810 + 42, 1, False, None),
            ('route-answer', 'player-0.bin', initial_words + 2 + 14, 1, False, None),
            ('inventory-answer', 'player-0.bin', initial_words + 62 + 2, 1, False, None),
            ('flight-answer', 'player-0.bin', initial_words + 70, 1, False, None),
            ('projectile-answer', 'player-0.bin', initial_words * 2 + 1 + 30 + 10, 1, False, None),
            ('context-total', 'player-context.bin', 42, 65535, False, 0x7f61),
            ('context-total-small', 'player-context.bin', 42, context_words[42], False, 0x7f61),
            ('context-section', 'player-context.bin', 9, 65535, False, 0x7f61),
        ]
    if args.clock:
        if args.player or manifest['qualification_scope'] != 'clock':
            raise ValueError('clock perturbations require the clock-only artifact')
        player = (assets / 'player-0.bin').read_bytes()
        offset = struct.unpack_from('<H', player)[0]
        first = offset
        for _ in range(3):
            offset += struct.unpack_from('<H', player, offset * 2)[0]
        if struct.unpack_from('<i', player, (offset + 24) * 2)[0] != 250:
            raise ValueError('clock trajectory must expose raw 250-ms racer elapsed')
        tests = [
            ('raw-elapsed', 'frames-0.bin', 0, 1, False, None),
            ('derived-stage-unused', 'frames-0.bin', 58, 512, True, None),
            ('clock-answer', 'clock-0.bin', 6, 1, False, None),
            ('racer-elapsed-answer', 'player-0.bin', offset + 24, 1, False, None),
            ('countdown-driving-answer', 'player-0.bin', first + 28, 1, False, None),
            ('initial-clock', 'clock-0.bin', 0, 1, False, None),
        ]
    if args.tail:
        if args.clock or args.player or manifest['qualification_scope'] != 'tail':
            raise ValueError('tail perturbations require the tail-only artifact')
        initial_words = struct.unpack_from('<H', (assets / 'player-0.bin').read_bytes())[0]
        cases = struct.unpack('<24H', (assets / 'cases.bin').read_bytes()[:48])
        frame_words = artifact.constants['DRIVING_FRAME_WORDS']['value']
        if len(manifest['cases']) != 2 or cases[20] != 3:
            raise ValueError('tail fixture must contain horn and hidden three-frame trajectories')
        tests = [
            ('horn-key', 'frames-0.bin', 58, 256, False, None),
            ('horn-timer-answer', 'player-0.bin', initial_words + 126, 1, False, None),
            ('gun-revision-answer', 'player-0.bin', initial_words + 127, 1, False, None),
            ('hidden-input', 'frames-1.bin', frame_words + 54, 1, False, None),
        ]
    package = artifact.files[artifact.manifest['package']]
    args.output.mkdir(parents=True, exist_ok=False)
    results = []
    for label, name, word, mask, passed, expected_signal in tests:
        payload = (assets / name).read_bytes()
        offset = resource_offset(package, payload)
        changed = bytearray(package)
        value = struct.unpack_from('<H', changed, offset + word * 2)[0]
        struct.pack_into('<H', changed, offset + word * 2, value ^ mask)
        folder = args.output / label
        folder.mkdir()
        image_path = folder / 'changed.cb06'
        image_path.write_bytes(changed)
        command = [str(args.runtime.resolve()), '--package', str(image_path.resolve()),
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
        results.append(dict(label=label, file=name, word=word, expected_pass=passed, halt=signal))
    report = dict(passed=True, artifact_id=artifact.manifest['artifact_id'], tests=results,
                  scope='Horn keys, horn timer, gun revision and hidden-vehicle input are checked against complete original Player calls.' if args.tail else
                        'Raw elapsed and persistent clock/racer answers are checked; source display-stage controls are recomputed.' if args.clock else
                        'Independent Player keys/gun and route/inventory/flight/projectile answers are checked; malformed context is rejected.' if args.player else
                        'Diagnostic class/object snapshots are unused; canonical identities and live '
                        'components remain checked, and malformed source sections are rejected before loading.')
    (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(dict(passed=True, perturbations=len(results))))


if __name__ == '__main__':
    main()
