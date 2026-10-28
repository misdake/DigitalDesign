"""Synthetic DRAW/independent oracle on explicit production GPU+MC exports.

This verifies the reference/replay plumbing; it is not a real CPU producer,
UART capture, physical button, board or mainline integration qualification.
"""
import argparse
import json
from pathlib import Path
import struct
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from draw_oracle import evaluate
from layout import ROW, COLUMN
from replay import prepare, execute
import snapshot as s
from test_snapshot import fixture
from test_draw_oracle import SyntheticCapture
from layout_replay import stage_column


def synthetic_snapshot(layout):
    abi = s.Abi()
    source = SyntheticCapture(layout, tint=0xff4080ff)
    header, _, _ = fixture(abi)
    pairs = dict(COMMAND_BASE=source.header['cb_base'], COMMAND_BYTES=source.header['cb_bytes'],
                 COMMAND_PADDED_BYTES=128, OBJECT_BASE=0x4d0000, OBJECT_BYTES=256,
                 COLOR_BASE=source.header['color_base'], DEPTH_BASE=source.header['depth_base'],
                 FRONT_BASE=0x400000, PLANE_BYTES=layout.plane_bytes)
    for name, value in pairs.items():
        at = abi['H_'+name]; header[at:at+2] = [value & 65535, value >> 16]
    header[abi['H_COUNT']] = source.header['cb_bytes']//4-1
    header[abi['H_LAYOUT_ABI']] = layout.abi
    source.put(0x4c0000, source.memory[0x4c0000]+bytes(128-source.header['cb_bytes']))
    source.put(0x400000, bytes(layout.plane_bytes))
    tags = {0x4c0000: 'COMMANDS', 0x4d0000: 'OBJECTS', 0x4e0000: 'MESH',
            0x4f0000: 'TEXTURE', 0x440000: 'COLOR', 0x480000: 'DEPTH', 0x400000: 'FRONT'}
    rows = []; blobs = []
    for address, payload in source.memory.items():
        tag = tags[address]; row = [0]*8
        row[abi['R_FLAGS']] = abi['READ']
        if tag in ('COLOR', 'DEPTH'):
            row[abi['R_FLAGS']] |= abi['WRITE'] | abi['PREIMAGE'] | abi['FULL_CLEAR']
        elif tag == 'FRONT':
            row[abi['R_FLAGS']] |= abi['PREIMAGE']
        else:
            row[abi['R_FLAGS']] |= abi['IMMUTABLE']
        row[abi['R_TAG']] = abi['TAG_'+tag]
        for name, value in [('BASE', address), ('BYTES', len(payload)), ('GENERATION', 1)]:
            at = abi['R_'+name]; row[at:at+2] = [value & 65535, value >> 16]
        rows.append(row); blobs.append(payload)
    header[abi['H_RANGE_COUNT']] = len(rows)
    captures, decoded = s.decode_stream(s.encode_reference(header, rows, blobs, abi), abi)
    if len(captures) != 1 or decoded['failures']:
        raise ValueError(decoded)
    return captures[0]


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--row-rtl', type=Path, required=True)
    parser.add_argument('--frozen-column', type=Path)
    parser.add_argument('--column-contract', type=Path)
    parser.add_argument('--layout', choices=('row', 'column'), required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args(); args.out.mkdir(parents=True, exist_ok=False)
    layout = ROW if args.layout == 'row' else COLUMN
    capture = synthetic_snapshot(layout); capture.save(args.out/'snapshot')
    reference = evaluate(capture)
    rtl = args.row_rtl
    if layout == COLUMN:
        if args.frozen_column is None or args.column_contract is None:
            raise ValueError('Column run requires explicit frozen GPU and contract')
        rtl = args.out/'rtl'; stage_column(args.row_rtl, args.frozen_column, args.column_contract, rtl)
    manifest = prepare(capture, rtl, args.out/'replay', 180000, 180,
                       dict(fixture_only=True, actual_cpu_producer=False, programming_receipt_verified=False,
                            captured_plane_bytes=layout.plane_bytes))
    result = execute(capture, manifest)
    actual = {role: layout.words((args.out/'replay/capture'/(role+'.bin')).read_bytes())
              for role in ('color', 'depth')}
    comparison = reference.compare(actual)
    for role in ('color', 'depth'):
        raw = (args.out/'replay/capture'/(role+'.bin')).read_bytes()
        for offset in range(0, layout.plane_bytes, 2):
            if layout.coordinates(offset) is None and struct.unpack_from('<H', raw, offset)[0] != reference.clear[role]:
                raise AssertionError('Physical padding differs from explicit CLEAR')
    report = dict(scope=__doc__, reference=reference.description(), comparison=comparison, replay=result)
    (args.out/'result.json').write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps(dict(pass_=comparison['pass'], layout=layout.abi,
                         stable_pixels=comparison['stable_pixels'], mismatches=comparison['mismatches'])))
    if not comparison['pass']:
        raise AssertionError('Independent mathematical color/depth mismatch')


if __name__ == '__main__':
    main()
