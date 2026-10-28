"""Real CPU frozen complete scene -> production GPU+CL3 MC -> independent check.

Requires the complete 6-box/1-RGB/17-rabbit draw stream, not a smoke substitute.
The reference is mathematical with declared approximation budgets, not bit-exact
Lighting equivalence. Physical buttons, UART and board timing are out of scope.
"""
import argparse
from collections import Counter
import json
from pathlib import Path
import struct
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from layout import header_layout
from replay import prepare, execute, preview
from scene_oracle import render
from producer_handoff import read_producer
from layout_replay import stage_column


def validate_scene(reference):
    s = reference.stats; records = s['draw_records']
    if (s['draws'] != 24 or s['triangles'] != 1141 or len(records) != 24 or
            sum(v['triangles'] for v in records[:6]) != 192 or records[6]['triangles'] != 1 or
            sum(v['triangles'] for v in records[7:]) != 948 or s['texture_draws'] != 6 or
            s['lit_draws'] != 17 or s['ui_commands'] < 1 or not s['ui_writes'] or
            set(s['clears']) != {'color','depth'}):
        raise ValueError('Input is not the complete box6+RGB1+rabbit17/948+UI scene')


def runtime_activity(path, expected_commands, command_bytes=None):
    counts = Counter(); opcodes = Counter()
    for line in path.read_text().splitlines():
        fields = line.split()
        if not fields: continue
        counts[fields[0]] += 1
        if fields[0] == 'CMD_EXEC':
            word=int(fields[3],16); pc=int(fields[2]);opcodes[word >> 28] += 1
            if command_bytes is not None:
                if pc != (counts['CMD_EXEC']-1)*4 or pc+4>len(command_bytes) or word != struct.unpack_from('<I',command_bytes,pc)[0]:
                    raise AssertionError('Actual executed command differs from frozen PC/word sequence')
    if (counts['DRAW'] != 24 or counts['VT_END'] != 24 or opcodes[3] != 24 or
            opcodes[11] < 1 or counts['CMD_EXEC'] != expected_commands or counts['DONE'] != 1 or
            counts['T_PUBLISH'] < 300 or counts['Q_ALLOC'] < 10000):
        raise AssertionError('Actual GPU trace did not execute a complete nonempty scene')
    return dict(events=dict(counts), executed_opcodes=dict(opcodes),
                frozen_command_words_verified=command_bytes is not None)


def check_outputs(capture,reference,folder):
    """Read original raw outputs with the final independent reference/monitors.

    Does not rewrite native receipts or imply another hardware execution when
    used to re-audit a frozen completed run after reference-only changes.
    """
    layout=header_layout(capture.header)
    actual={role:layout.words((folder/'replay/capture'/(role+'.bin')).read_bytes())
            for role in ('color','depth')}
    padding_words=0
    for role in ('color','depth'):
        raw=(folder/'replay/capture'/(role+'.bin')).read_bytes()
        for offset in range(0,layout.plane_bytes,2):
            if layout.coordinates(offset) is None:
                padding_words+=1
                if struct.unpack_from('<H',raw,offset)[0] != reference.stats['clears'][role]:
                    raise AssertionError('Physical padding differs from explicit plane CLEAR')
    comparison=reference.compare(actual)
    for group,minimum in [('unlit_rgb',100),('texture',10000),('lighting',500)]:
        if comparison['groups'][group]['pixels']<minimum:
            raise AssertionError('Too few independently checked '+group+' pixels')
    activity=runtime_activity(folder/'replay/capture/events.txt',reference.stats['commands'],
                               capture.read(capture.header['cb_base'],capture.header['cb_bytes']))
    return dict(comparison=comparison,activity=activity,physical_padding_words_checked=padding_words)


def main():
    p = argparse.ArgumentParser(__doc__)
    p.add_argument('--fixture', type=Path, required=True)
    p.add_argument('--producer-receipt', type=Path, required=True)
    p.add_argument('--producer-commit', required=True)
    p.add_argument('--row-rtl', type=Path, required=True)
    p.add_argument('--frozen-column', type=Path)
    p.add_argument('--column-contract', type=Path)
    p.add_argument('--out', type=Path, required=True)
    p.add_argument('--max-cycles', type=int, default=2000000)
    p.add_argument('--timeout', type=int, default=1800)
    args = p.parse_args(); args.out.mkdir(parents=True, exist_ok=False)
    capture,wire,context = read_producer(args.fixture,args.producer_receipt,args.producer_commit)
    capture.save(args.out/'snapshot'); (args.out/'host-wire.bin').write_bytes(wire)
    reference = render(capture); validate_scene(reference)
    (args.out/'reference-input.json').write_text(json.dumps(reference.stats,indent=2)+'\n')
    layout = header_layout(capture.header)
    # Preserve an independent image before launching the real GPU. No runtime
    # image or source-primitive owner trace can influence reference construction.
    raw = bytearray(layout.plane_bytes)
    for y in range(240):
        for x in range(400): struct.pack_into('<H',raw,layout.offset(x,y),int(reference.color[y,x]))
    preview(raw,args.out/'independent-reference.png',capture.header['target_format']==1,layout)
    rtl = args.row_rtl
    if layout.abi == 2:
        if args.frozen_column is None or args.column_contract is None:
            raise ValueError('Column run requires explicit frozen GPU and contract')
        rtl = args.out/'rtl'; stage_column(args.row_rtl,args.frozen_column,args.column_contract,rtl)
    manifest = prepare(capture,rtl,args.out/'replay',args.max_cycles,args.timeout,context)
    result = execute(capture,manifest)
    checks=check_outputs(capture,reference,args.out);comparison=checks['comparison']
    (args.out/'independent-comparison.json').write_text(json.dumps(comparison,indent=2)+'\n')
    report = dict(scope=__doc__,producer=context,replay=result,**checks)
    (args.out/'result.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(dict(pass_=comparison['pass'],layout=layout.abi,draws=24,triangles=1141,
                         cycles=result['cold_submission_cycles'],stable_pixels=comparison['stable_pixels'])))
    if not comparison['pass']:
        raise AssertionError('Full-scene independent reference mismatch; see independent-comparison.json')


if __name__ == '__main__':
    main()
