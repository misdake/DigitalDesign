"""Bounded real-producer A/B/A *address* reuse qualification, cold per input.

Never interprets three independent cold resets as a warm GPU queue sequence.
CPU producer receipts remain mandatory; synthetic fixtures have a separate test.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from producer_handoff import read_producer
from draw_oracle import evaluate
from layout import header_layout


def verify_reuse(captures):
    if len(captures) != 3:
        raise ValueError('Need exactly three frozen occurrences in A/B/A address order')
    headers = [c.header for c in captures]
    if headers[0]['cb_base'] != headers[2]['cb_base'] or headers[0]['cb_base'] == headers[1]['cb_base']:
        raise ValueError('Expected natural two-slot A/B/A CB address reuse')
    if len({h['token'] for h in headers}) != 3:
        raise ValueError('Reused address occurrences need distinct lease tokens')
    if len({h['submission'] for h in headers}) != 3:
        raise ValueError('Reused address occurrences need distinct submission identities')
    if any(header_layout(h) != header_layout(headers[0]) for h in headers):
        raise ValueError('One A/B/A producer sequence must use one storage layout')
    references = [evaluate(c) for c in captures]
    first, last = references[0], references[2]
    def different(i):
        return first.stable[i] and last.stable[i] and (
            first.planes['color'][i] != last.planes['color'][i] or
            first.planes['depth'][i] != last.planes['depth'][i])
    changed = sum(different(i) for i in range(96000))
    changed_draw = sum(different(i) for i in first.caller['drawn_pixels'] | last.caller['drawn_pixels'])
    if changed_draw < 256:
        raise ValueError('Reused address must change at least 256 stable DRAW pixels, beyond CLEAR-only differences')
    if changed < 256:
        raise ValueError('Reused address must have at least 256 observably changed stable output pixels')
    entries = []
    for c, reference in zip(captures, references):
        h = c.header
        entries.append(dict(cb_base=h['cb_base'], lease=h['token'], submission=h['submission'],
                            cb_sha256=hashlib.sha256(c.read(h['cb_base'], h['cb_bytes'])).hexdigest(),
                            input_sha256=c.manifest()['input_sha256'],
                            reference_color_sha256=hashlib.sha256(bytes().join(
                                v.to_bytes(2, 'little') for v in reference.planes['color'])).hexdigest(),
                            draw_commands=reference.caller['draw_commands']))
    return dict(occurrences=entries, reused_address_changed_stable_pixels=changed,
                reused_address_changed_stable_draw_pixels=changed_draw,
                scope='Distinct frozen occurrences and independent cold replays, not warm queue/CPU concurrency proof')


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--fixture', type=Path, action='append', required=True)
    parser.add_argument('--producer-receipt', type=Path, required=True)
    parser.add_argument('--producer-commit', required=True)
    parser.add_argument('--row-rtl', type=Path, required=True)
    parser.add_argument('--frozen-column', type=Path)
    parser.add_argument('--column-contract', type=Path)
    parser.add_argument('--include-edges', action='store_true')
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    # Validate the entire real producer/input closure before launching a replay.
    captures = [read_producer(p, args.producer_receipt, args.producer_commit)[0] for p in args.fixture]
    result = verify_reuse(captures)
    args.out.mkdir(parents=True, exist_ok=False)
    for index, path in enumerate(args.fixture):
        command = [sys.executable, '-B', str(Path(__file__).with_name('producer_handoff.py')),
                   '--fixture', str(path), '--producer-receipt', str(args.producer_receipt),
                   '--producer-commit', args.producer_commit, '--row-rtl', str(args.row_rtl),
                   '--oracle', 'draw', '--out', str(args.out/str(index))]
        for option, value in [('--frozen-column', args.frozen_column), ('--column-contract', args.column_contract)]:
            if value is not None:
                command.extend([option, str(value)])
        if args.include_edges:
            command.append('--include-edges')
        with (args.out/f'{index}.log').open('w') as log:
            subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, timeout=240, check=True)
        result['occurrences'][index]['result'] = json.loads((args.out/str(index)/'result.json').read_text())
    result['pass'] = True
    (args.out/'sequence.json').write_text(json.dumps(result, indent=2)+'\n')
    print(json.dumps(dict(pass_=True, occurrences=3, changed_pixels=result['reused_address_changed_stable_pixels'])))


if __name__ == '__main__':
    main()
