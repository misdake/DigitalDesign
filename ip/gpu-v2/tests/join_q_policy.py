"""Bounded actual join/Q/C-D source equivalence and frozen-fixture replay."""
from pathlib import Path
import argparse
import hashlib
import importlib.util
import json
import os
import re
import shutil
import subprocess
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[3]
ABORT = 'always @(posedge clk)if(reset)stopped<=0;else if(abort)stopped<=1;'
EMPTY = 'always @(posedge clk) begin if(reset)begin end else begin end end'
BANKS = ['gpu_v2_postquad_light_bank', 'gpu_v2_postquad_sample_bank']


def tokens(source):
    source = re.sub(r'//[^\n]*|/\*.*?\*/', '', source, flags=re.S)
    return re.findall(r'"(?:\\.|[^"\\])*"|[A-Za-z0-9_$\']+|[^\s]', source)


def modules(source):
    source = re.sub(r'//[^\n]*|/\*.*?\*/', '', source, flags=re.S)
    return [(m[1], tokens(m[0])) for m in re.finditer(
        r'\bmodule\s+(\w+)(?=[\s(#]).*?\bendmodule\b', source, re.S)]


def drop_exact_once(sequence, statement):
    part = tokens(statement)
    matches = [i for i in range(len(sequence)-len(part)+1)
               if sequence[i:i+len(part)] == part]
    assert len(matches) == 1, ('expected one exact statement', statement, len(matches))
    i = matches[0]
    return sequence[:i] + sequence[i+len(part):]


def call(command, directory, log):
    result = subprocess.run(list(map(str, command)), cwd=directory,
                            capture_output=True, text=True, timeout=60)
    log.write_text(result.stdout + result.stderr)
    return result


def preprocess(source, directory, synthesis):
    directory.mkdir()
    (directory/'dut.v').write_text(source)
    result = call([os.environ['IVERILOG_EXE'], '-g2012', '-E',
                   *(['-DSYNTHESIS'] if synthesis else []), '-o', directory/'pp.v',
                   directory/'dut.v'], directory, directory/'compile.log')
    assert result.returncode == 0, result.stdout + result.stderr
    return (directory/'pp.v').read_text()


def compare(base, current, out):
    rows = []
    profiles = [f'{profile}-{mode}.v' for profile in
                ['generic', 'generic-command-ports', 'command', 'full']
                for mode in ['native', 'portable']]
    for name in profiles:
        for synthesis in [False, True]:
            tag = name[:-2] + '-s' + str(int(synthesis))
            before = preprocess((base/name).read_text(), out/(tag+'-base'), synthesis)
            after = preprocess((current/name).read_text(), out/(tag+'-current'), synthesis)
            bm, cm = modules(before), modules(after)
            assert [n for n, _ in bm] == [n for n, _ in cm], name
            changes = []
            for (module, b), (_, c) in zip(bm, cm):
                if b == c:
                    continue
                if module == 'gpu_v2_vliw_backend_three_port' and name.startswith('command-'):
                    b, c = drop_exact_once(b, ABORT), drop_exact_once(c, ABORT)
                    changes.append([module, 'same independent abort process relocated'])
                elif module in BANKS and name.endswith('-native.v'):
                    b = drop_exact_once(b, EMPTY)
                    changes.append([module, 'exact empty native bank process removed'])
                assert b == c, ('unaccounted ordered module tokens', name, synthesis, module)
            rows.append(dict(profile=name, synthesis=synthesis,
                             complete_stream_exact=tokens(before) == tokens(after),
                             named_changes=changes))
    for mode in ['native', 'portable']:
        for width in [18, 24]:
            name = f'public-bank{width}-{mode}.v'
            assert (base/name).read_bytes() == (current/name).read_bytes(), name
        generic = (current/f'generic-{mode}.v').read_text()
        expected = preprocess(generic, out/('scope-generic-'+mode), True)
        forced = preprocess('`define GPU_V2_COMMAND_JOIN\n'+generic,
                            out/('scope-forced-'+mode), True)
        assert tokens(expected) == tokens(forced), 'forced private flag changed default checked'
        probe = '\n`ifdef GPU_V2_COMMAND_JOIN\nJOIN_PROFILE_LEAK\n`endif\n'
        command = (current/f'command-{mode}.v').read_text()
        scoped = preprocess(command+probe, out/('scope-leak-'+mode), True)
        assert 'JOIN_PROFILE_LEAK' not in scoped
    return rows


def capture(base, out):
    # Reuse the existing independent integer/complete-memory oracle unchanged.
    (ROOT/'target/gpu-vliw-integration-20261018').mkdir(exist_ok=True)
    out.mkdir()
    for mode in ['native', 'portable']:
        shutil.copy2(base/f'generic-{mode}.v', out/f'backend-{mode}.v')
    saved_argv = sys.argv
    saved_environment = {k: os.environ.get(k) for k in
                         ['VLIW_BACKEND_EXPORT', 'VLIW_SOURCE_HOLD_CYCLES']}
    absent = object()
    saved_base_module = sys.modules.get('stage1_frozen_replay', absent)
    try:
        # The fixture embeds the hold count in its bench at import time.
        os.environ['VLIW_BACKEND_EXPORT'] = str(out)
        os.environ['VLIW_SOURCE_HOLD_CYCLES'] = '17'
        spec = importlib.util.spec_from_file_location(
            'join_complete_fixture', Path(__file__).with_name('vliw_backend_complete.py'))
        fixture = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(fixture)
        fixture.m.O = out
        for tag, args in [('baseline', ['--steady', '--count', '64']),
                          ('masks', ['--mask-matrix'])]:
            sys.argv = ['join', '--tag', tag, '--mode', 'both', *args]
            fixture.m.main()
    finally:
        sys.argv = saved_argv
        if saved_base_module is absent:
            sys.modules.pop('stage1_frozen_replay', None)
        else:
            sys.modules['stage1_frozen_replay'] = saved_base_module
        for key, value in saved_environment.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value
    return out


def simulate(source, fixture, out, native, synthesis):
    out.mkdir()
    for path in fixture.iterdir():
        if path.suffix == '.mem' or path.name == 'tb.v':
            shutil.copy2(path, out/path.name)
    # Observe the existing paid-D3 contract without replacing its owner or oracle.
    bench = (out/'tb.v').read_text()
    anchor = '  if(z_pending&&dut.depth_data!==z_expected)'
    assert bench.count(anchor) == 1
    bench = bench.replace(anchor,
        '  if(!reset&&!fault&&(dut.raw_retire_proposal !== '
        '(dut.lane_accept&&(dut.out_lane==2\'d3))))'
        '$fatal(1,"raw retirement differs from paid D3");\n'+anchor, 1)
    (out/'tb.v').write_text(bench)
    (out/'dut.v').write_text(source)
    command = [os.environ['IVERILOG_EXE'], '-g2012', '-s', 'tb',
               *(['-DSYNTHESIS'] if synthesis else []), '-o', out/'sim.vvp']
    if native:
        command += [Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v']
    result = call(command+[out/'dut.v', out/'tb.v'], out, out/'compile.log')
    assert result.returncode == 0, result.stdout + result.stderr
    return call([os.environ['VVP_EXE'], out/'sim.vvp'], out, out/'run.log')


def replay(base, current, fixtures, out):
    rows = []
    for tag in ['baseline', 'masks']:
        for mode in ['native', 'portable']:
            f = fixtures/f'complete-{tag}-{mode}'
            assert (f/'dut.v').read_bytes() == (base/f'generic-{mode}.v').read_bytes()
            for synthesis in [False, True]:
                d = out/f'{tag}-{mode}-s{int(synthesis)}'
                result = simulate((current/f'generic-{mode}.v').read_text(), f, d,
                                  mode == 'native', synthesis)
                assert result.returncode == 0 and 'PASS complete' in result.stdout, result.stdout
                trace = (d/'trace.txt').read_bytes()
                assert trace == (f/'trace.txt').read_bytes(), d
                rows.append(dict(case=d.name, trace_sha256=hashlib.sha256(trace).hexdigest()))
    return rows


def negatives(current, fixtures, out):
    mutants = [
        ('masked-read', 'state==J_READ&&covered&&issue_light_ready',
         'state==J_READ&&issue_light_ready'),
        ('retire-at-C', 'raw_retire_proposal=ce&&lane_valid&&lane_ready&&out_lane==3;',
         'raw_retire_proposal=ce&&sink_color_accept&&out_lane==3;'),
        ('double-empty', "if(empty_skip||issue_lane==3)begin issue_lane<=0;issue_slot<=issue_slot+5'd1;end",
         "if(empty_skip||issue_lane==3)begin issue_lane<=0;issue_slot<=issue_slot+(empty_skip?5'd2:5'd1);end"),
        ('premature-Q', '.CEB(read_en && !collision && !reset)', '.CEB(!collision && !reset)'),
    ]
    rows = []
    source = (current/'generic-native.v').read_text()
    for name, old, new in mutants:
        if name == 'premature-Q':
            m = re.search(r'module gpu_v2_postquad_light_bank\(.*?endmodule', source, re.S)
            assert m and m[0].count(old) == 1
            changed = source[:m.start()]+m[0].replace(old, new)+source[m.end():]
        else:
            assert source.count(old) == 1, name
            changed = source.replace(old, new, 1)
        fixture = fixtures/('complete-masks-native' if name == 'masked-read'
                            else 'complete-baseline-native')
        result = simulate(changed, fixture, out/name, True, True)
        assert result.returncode != 0 and 'FATAL' in result.stdout, (name, result.stdout)
        rows.append(dict(name=name, caught=True, dut_sha256=hashlib.sha256(changed.encode()).hexdigest()))
    return rows


def fixture_hashes(directory):
    return {str(p.relative_to(directory)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in directory.rglob('*') if p.is_file()
            and (p.suffix == '.mem' or p.name in ['tb.v', 'dut.v', 'trace.txt'])}


def held_source_window(directory):
    """Check executed coverage against events, not merely the generated constant."""
    log = (directory/'run.log').read_text()
    count = re.search(r'^SOURCE .*holdCE0=(\d+)$', log, re.M)
    assert count and int(count[1]) == 17, ('17-edge hold not exercised', directory)
    events = [line.split() for line in (directory/'trace.txt').read_text().splitlines()
              if line.startswith('B ')]
    # The unchanged complete fixture packs29 events: CE is bit28, common
    # advance bit25, and accepted Basic/source read bit20.
    start = next(i for i, row in enumerate(events) if int(row[2], 16) & (1 << 20))
    window = events[start+1:start+18]
    assert len(window) == 17
    assert all(int(row[2], 16) & ((1 << 28) | (1 << 25)) == 0 for row in window), (
        'requested hold did not pause actual CE/advance', directory)
    assert all(row[3:] == events[start][3:] for row in window), (
        'source/retirement prefix changed during initial held Q', directory)
    return dict(edges=17, first_wall=int(window[0][1]), last_wall=int(window[-1][1]),
                CE_and_advance_zero=True, source_prefix_held=True)


def abort_wall(current, out):
    source = (current/'command-native.v').read_text()
    bench = '''`timescale 1ns/1ps
module tb;
reg clk=0,reset=1,abort=0;wire fault;
GSR GSR(.GSRI(1'b1));
gpu_v2_vliw_backend_three_port dut(.clk(clk),.reset(reset),.ce(1'b0),
 .abort(abort),.front_ce(1'b0),.phase(2'd0),.quad_valid(1'b0),
 .quad_end_valid(1'b0),.result_we(1'b0),.depth_event_valid(1'b0),
 .lod_valid(1'b0),.flush_request(1'b0),.fault(fault));
task tick;begin #2;clk=1;#2;clk=0;#1;end endtask
initial begin
 tick;tick;reset=0;tick;if(fault!==0)$fatal(1,"cold reset");
 abort=1;tick;abort=0;#1;
 if(fault!==1)$fatal(1,"JOIN CE0 abort was not sticky");
 repeat(3)tick;if(fault!==1)$fatal(1,"JOIN CE0 abort lost ownership");
 reset=1;tick;if(fault!==0)$fatal(1,"JOIN reset did not release");
 $display("PASS wall abort");$finish;
end
initial begin #1000;$fatal(1,"abort watchdog");end
endmodule
'''
    m = re.search(r'module gpu_v2_vliw_backend_three_port\(.*?endmodule', source, re.S)
    assert m and m[0].count(ABORT) == 1
    mutant = source[:m.start()]+m[0].replace(
        ABORT, ABORT.replace('else if(abort)', 'else if(abort&&ce)'), 1)+source[m.end():]
    for name, dut in [('positive', source), ('abort-CE0', mutant)]:
        d = out/name
        d.mkdir()
        (d/'dut.v').write_text(dut)
        (d/'tb.v').write_text(bench)
        result = call([os.environ['IVERILOG_EXE'], '-g2012', '-DSYNTHESIS',
                       '-s', 'tb', '-o', d/'sim.vvp', d/'dut.v', d/'tb.v',
                       Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v'],
                      d, d/'compile.log')
        assert result.returncode == 0, result.stdout+result.stderr
        result = call([os.environ['VVP_EXE'], d/'sim.vvp'], d, d/'run.log')
        if name == 'positive':
            assert result.returncode == 0 and 'PASS wall abort' in result.stdout, result.stdout
        else:
            assert result.returncode != 0 and 'JOIN CE0 abort' in result.stdout, result.stdout
    return dict(positive=True, ce_gated_actual_TOP_mutant_caught=True, cycle_bound=16)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--base-exports', required=True, type=Path)
    parser.add_argument('--exports', required=True, type=Path)
    parser.add_argument('--fixtures', type=Path,
                        help='frozen accepted complete-baseline/masks native/portable directories')
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--negatives', action='store_true')
    args = parser.parse_args()
    base, current, out = args.base_exports.resolve(), args.exports.resolve(), args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    pp = out/'preprocessed'
    pp.mkdir()
    rows = compare(base, current, pp)
    fixtures = args.fixtures.resolve() if args.fixtures else capture(base, out/'accepted-fixtures')
    frozen = fixture_hashes(fixtures)
    held = {f'complete-{tag}-{mode}': held_source_window(fixtures/f'complete-{tag}-{mode}')
            for tag in ['baseline', 'masks'] for mode in ['native', 'portable']}
    run = out/'replay'
    run.mkdir()
    traces = replay(base, current, fixtures, run)
    held.update({str(p.relative_to(out)): held_source_window(p)
                 for p in sorted(run.iterdir()) if p.is_dir()})
    bad = out/'negative-controls'
    bad.mkdir()
    controls = negatives(current, fixtures, bad) if args.negatives else []
    wall = out/'abort-wall'
    wall.mkdir()
    abort = abort_wall(current, wall)
    assert frozen == fixture_hashes(fixtures), 'accepted fixture evidence was modified'
    (out/'receipt.json').write_text(json.dumps(dict(
        passed=True, streams=rows, traces=traces, negative_controls=controls,
        paired_D3_observer=True, wall_abort=abort,
        held_source_waveforms=held,
        accepted_fixture_sha256=frozen, public_bank_exports_raw_identical=4,
        cycle_limit=30000, process_timeout_seconds=60,
        scope='actual TOP/Q/sink and complete module tokens; no new physical or board qualification'), indent=2))
    print('PASS16 full PP profiles,8 frozen traces,', len(controls), 'real DUT negatives')
