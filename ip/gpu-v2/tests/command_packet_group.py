"""Bounded selected command Membership/Work/Packet/Group ownership regression.

Independent bits/oracles are derived from logical texel locations, never DUT
outputs. Optional fixed exports compare every sampled trace and diagnostic edge.
"""
from pathlib import Path
import argparse
import hashlib
import json
import os
import random
import re
import subprocess
import sys

sys.dont_write_bytecode = True


def vectors(rtt):
    rng = random.Random(160835)
    rows, groups, work = [], [], []
    for n in ([9] if rtt else range(11)):
        side = 1 << n
        xs = [0, 14, 15, 16, 382, 398] if rtt else sorted({0, min(2, side-1), min(15, side-1), side-1})
        ys = [0, 6, 7, 8, 238] if rtt else sorted({0, min(4, side-1), min(7, side-1), side-1})
        for x, y in zip(xs * len(ys), [v for v in ys for _ in xs]):
            for mask in range(1, 16):
                weights = sum((rng.randrange(1, 512) if mask >> i & 1 else 0) << (9*i) for i in range(4))
                slot, key, fine, last = rng.randrange(16), rng.randrange(64), rng.randrange(2), rng.randrange(2)
                coords = [((x+i%2) % side, (y+i//2) % side) for i in range(4)]
                seen, emit = set(), 0
                for i, (xx, yy) in enumerate(coords):
                    line = (xx//16, yy//8)
                    if mask >> i & 1 and line not in seen:
                        emit |= 1 << i
                        seen.add(line)
                member = (weights | (emit << 36) | ((x//16) << 40) | ((y//8) << 46) |
                          ((x%16) << 53) | ((y%8) << 57) | (slot << 60) | (n << 64) |
                          ((key//4) << 68) | ((key%4) << 72) | (fine << 74) | ((last or not fine) << 75))
                stimulus = weights | (x << 36) | (y << 46) | (slot << 56) | (n << 60) | (key << 64) | (fine << 70) | (last << 71)
                rows.append((stimulus, member))
                for tap in range(4):
                    if not emit >> tap & 1:
                        continue
                    xx, yy = coords[tap]
                    coverage = sum(int((cx//16, cy//8) == (xx//16, yy//8)) << i for i, (cx, cy) in enumerate(coords))
                    prefix = 0 if rtt else sum(2 * max(1, (1 << k)//16)**2 for k in range(n))
                    tile = (xx//16) * (16 if rtt else max(1, side//16)) + yy//16
                    half = (0x1000 + prefix + tile*2 + (yy%16)//8) & 32767
                    owner = (key%4 << 6) | (key//4 << 2) | (int(bool(last or not fine) and emit >> (tap+1) == 0) << 1) | int(bool(fine) and tap == 0)
                    groups.append(half | (x%16 << 15) | (y%8 << 19) | (owner << 22) | ((n&1) << 30) | (coverage << 31))
                    work.append((member & ~((1 << 36)-1) & ~(15 << 60)) | (tap << 76))
    return rows, groups, work


def run(cmd, directory, label, fatal=None):
    with (directory/(label+'.log')).open('wb') as log:
        result = subprocess.run(list(map(str, cmd)), cwd=directory, stdout=log, stderr=subprocess.STDOUT, timeout=60)
    text = (directory/(label+'.log')).read_text(errors='replace')
    if fatal:
        assert result.returncode != 0 and fatal in text, text[-2000:]
    else:
        assert result.returncode == 0, text[-2000:]
    return text


def compile_run(source, directory, native=0, monitors=True, fatal=None):
    cmd = [os.environ['IVERILOG_EXE'], '-g2012', '-s', 'tb', '-o', 'sim.vvp',
           f'-Ptb.NATIVE={native}', source, 'tb.v', Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v']
    if not monitors:
        cmd.insert(1, '-DSYNTHESIS')
    run(cmd, directory, 'compile')
    return run([os.environ['VVP_EXE'], 'sim.vvp'], directory, 'run', fatal)


def negatives(sources, out):
    # Each row names an original detector alternative. Forces inject a broken
    # local invariant; commit is disabled in wrappers to isolate the observer.
    cases = [
        ('membership-level', 'membership_leaf', 'mvalid=1;level=11;'),
        ('membership-empty', 'membership_leaf', 'mvalid=1;weights=0;'),
        ('work-put-empty', 'gpu_v2_texture_work_unique', "force work.put=1;"),
        ('work-fetch-reserved', 'gpu_v2_texture_work_unique', "force work.fetch=1;force work.read_data=6'h11;"),
        ('work-fetch-empty', 'gpu_v2_texture_work_unique', "force work.fetch=1;force work.read_data=0;"),
        ('work-head-busy', 'gpu_v2_texture_work_unique', "force work.fetch=1;force work.read_data=1;force work.head_valid=1;"),
        ('work-row-collision', 'gpu_v2_texture_work_unique', "force work.fetch=1;force work.put=1;force work.read_data=1;force work.in_member=76'h1000000000;"),
        ('work-inactive-tap', 'gpu_v2_texture_work_unique', 'force work.take=1;force work.emit=0;force work.out_tap=0;'),
        ('packet-inactive-tap', 'packet_leaf_unique', 'pvalid=1;pmember=0;'),
        ('packet-base-alignment', 'packet_leaf_unique', 'pvalid=1;base=1;'),
        ('packet-rtt-x-bound', 'packet_leaf_unique', 'fb=1;force packet.valid=1;force packet.tx=25;force packet.ty=0;'),
        ('packet-rtt-y-bound', 'packet_leaf_unique', 'fb=1;force packet.valid=1;force packet.tx=0;force packet.ty=30;'),
        ('group-reserve-full', 'gpu_v2_packet_store_unique', 'reserve=1;force group_dut.used=48;force group_dut.missing=0;'),
        ('group-write-unreserved', 'gpu_v2_packet_store_unique', 'write_valid=1;'),
        ('group-consume-empty', 'gpu_v2_packet_store_unique', 'consume=1;'),
        ('packets-input-empty', 'gpu_v2_runtime_packet_backend_unique', 'force packets.in_valid=1;'),
        ('packets-capture-inactive', 'gpu_v2_runtime_packet_backend_unique', 'force packets.issue=1;force packets.member=0;force packets.tap=0;'),
        ('packets-write-unreserved', 'gpu_v2_runtime_packet_backend_unique', 'force packets.packet_valid=1;'),
        ('packets-work-peer', 'gpu_v2_runtime_packet_backend_unique', 'force packets.work_fault=1;'),
        ('packets-packet-peer', 'gpu_v2_runtime_packet_backend_unique', 'force packets.packet_fault=1;'),
        ('packets-group-peer', 'gpu_v2_runtime_packet_backend_unique', 'force packets.pool_fault=1;'),
        ('runtime-credit-limit', 'gpu_v2_texture_runtime_backend_unique', 'force runtime_dut.reserved=17;'),
        ('runtime-work-credit', 'gpu_v2_texture_runtime_backend_unique', 'force runtime_dut.work_count=1;'),
        ('runtime-ack-credit', 'gpu_v2_texture_runtime_backend_unique', 'force runtime_dut.proposal_ack=1;'),
        ('runtime-member-credit', 'gpu_v2_texture_runtime_backend_unique', 'force runtime_dut.member_return=1;force runtime_dut.work_ready=0;'),
        ('runtime-member-empty', 'gpu_v2_texture_runtime_backend_unique', 'force runtime_dut.coefficient_valid=1;force runtime_dut.coefficient_weights=0;'),
        ('runtime-coefficient-peer', 'gpu_v2_texture_runtime_backend_unique', 'force runtime_dut.coefficient_bad=1;'),
        ('runtime-packet-peer', 'gpu_v2_texture_runtime_backend_unique', 'force runtime_dut.packet_bad=1;'),
    ]
    fixture = Path(__file__).with_name('fixtures')/'command_packet_group_negative_tb.v'
    results = []
    for case, module, injection in cases:
        observed, released = [], []
        for name, source in sources:
            for monitors in (False, True):
                directory = out/f'{name}-{case}-{int(monitors)}';directory.mkdir()
                (directory/'tb.v').write_text(fixture.read_text().replace('@INJECT@', injection).replace('@CASE@', case))
                log = compile_run(source, directory, monitors=monitors, fatal=('FATAL:' if monitors else None))
                if monitors:
                    assert 'GPU contract:' in log or ('packet-' in case and ('base must' in log or 'Group bounds' in log)), log
                    times = re.findall(r'Time: (\d+)', log)
                    assert times and set(times) == {'35000'}, (case, log)
                    labels = sorted(set(re.findall(r'(?:GPU contract: [^\n]+|Texture16x8 base[^\n]+|RTT400 Group bounds)', log)))
                    observed.append(labels)
                else:
                    released.append(log)
        assert all(row == observed[0] for row in observed), (case, observed)
        assert len(set(released)) == 1, (case, released)
        results.append(dict(case=case, module=module, injection=injection, failure_time_ps=35000, labels=observed[0], release=released[0].strip()))
    return results


def sampler(sources, out, portable_sources):
    # Reuse the existing independent RTT integer oracle and short sampler bench.
    import importlib.util
    spec = importlib.util.spec_from_file_location('rtt_reference', Path(__file__).with_name('test_rtt400.py'))
    ref = importlib.util.module_from_spec(spec);spec.loader.exec_module(ref)
    values = [(round((x+.37)*65536/400), round((y+.61)*65536/240)) for y in (0,7,8,239) for x in (0,15,16,399)]
    values = (values + [(0,0),(65535,65535),(262143,262143),(65536,65536)]*4) * 2
    results = []
    for nearest in (False, True):
        traces = []
        for name, source in sources:
            for native in (0, 1):
                for monitors in (False, True):
                    directory = out/f'{name}-sampler-{int(nearest)}-{native}-{int(monitors)}';directory.mkdir()
                    (directory/'stim.hex').write_text(''.join(f'{(v<<18)|u:09x}\n' for u,v in values))
                    (directory/'gold.hex').write_text(''.join(f'{ref.color(u,v,nearest):06x}\n' for u,v in values))
                    clamp = re.search(r' function\[15:0\]view_clamp_uv;.*?endfunction', source.read_text(), re.S)[0]
                    tb = ref.SAMPLER_TB.replace('@CLAMP@',clamp).replace('@LAST@',str(len(values)-1)).replace('@COUNT@',str(len(values))).replace('@MAX@','20000').replace('@FILTER@',str(0 if nearest else 1))
                    tb = tb.replace('reg[31:0]fill_address=0;', "reg[31:0]fill_address=32'h100000;")
                    tb = tb.replace('module tb;', 'module tb;parameter NATIVE=1;')
                    tb = tb.replace('endmodule', """
 defparam coefficients.packets.u_pool.NATIVE=NATIVE;
 defparam cache.NATIVE=NATIVE;
 always@(negedge clk)if(!reset)$display("TRACE %0d %h %h %h %h %h %h",cycle,
  {ww,ww?wa:7'd0,ww?wd:36'd0},
  {coefficients.member_return,coefficients.member_return?coefficients.member_payload:76'd0},
  {pv,pv?packet:35'd0,next_valid,next_valid?next_key:15'd0},
  {cache.read_issue,cache.read_issue?packet[14:0]:15'd0},
  {result,result?key:6'd0,result?rgb:24'd0},{request,request?address:32'd0});
endmodule
""")
                    (directory/'tb.v').write_text(tb)
                    log = compile_run(portable_sources.get(name, source) if native == 0 else source, directory, native, monitors)
                    trace = '\n'.join(line for line in log.splitlines() if line.startswith('TRACE '))
                    traces.append(trace)
                    results.append(dict(source=name,native=native,monitors=monitors,nearest=nearest,pixels=len(values),trace_sha256=hashlib.sha256(trace.encode()).hexdigest(),summary=[line for line in log.splitlines() if line.startswith('PASS ')]))
        assert len(set(traces)) == 1, 'Selected coefficient/Member76/Group35/cache read_issue traces differ'
    return results


def mutants(source, out):
    text = source.read_text()
    cases = [
        ('membership-x-boundary', 'wire sx=(common0[63:60]<=4)', 'wire sx=(common0[63:60]<=3)', False, 0, 'Member76'),
        ('rtt-pitch', "pitch_y_log2=texture_fb400240?4'd4:", "pitch_y_log2=texture_fb400240?4'd3:", True, 0, 'Packet35 bits'),
        ('lookahead-key', 'assign next_key=data_do[14:0];', "assign next_key=data_do[14:0]+15'd1;", False, 0, 'lookahead bits/order'),
        ('native-output-ce', '.OCE(load),', '.OCE(pr),', False, 1, 'Group35'),
    ]
    results = []
    fixture = Path(__file__).with_name('fixtures')/'command_packet_group_tb.v'
    for case, old, new, rtt, native, expected in cases:
        assert text.count(old) == 1, (case, text.count(old))
        directory = out/('mutant-'+case);directory.mkdir()
        mutant = directory/'mutant.v';mutant.write_text(text.replace(old, new, 1))
        rows, groups, work = vectors(rtt)
        for filename, values, digits in [('stim', [r[0] for r in rows], 18), ('members', [r[1] for r in rows], 19), ('groups', groups, 9), ('work', work, 20)]:
            (directory/(filename+'.hex')).write_text(''.join(f'{v:0{digits}x}\n' for v in values))
        tb = fixture.read_text()
        for key, value in {'LAST':len(rows)-1,'GLAST':len(groups)-1,'COUNT':len(rows),'GROUPS':len(groups),'MIP':int(not rtt),'RTT':int(rtt)}.items():
            tb = tb.replace('@'+key+'@',str(value))
        (directory/'tb.v').write_text(tb)
        log = compile_run(mutant, directory, native, False, expected)
        results.append(dict(case=case,original=old,mutated=new,mutant_sha256=hashlib.sha256(mutant.read_bytes()).hexdigest(),diagnostic='\n'.join(log.splitlines()[-2:])))
    return results


def aborts(sources, out):
    fixture = Path(__file__).with_name('fixtures')/'command_packet_group_negative_tb.v'
    results, traces = [], []
    for name, source in sources:
        for monitors in (False, True):
            directory=out/f'{name}-explicit-abort-{int(monitors)}';directory.mkdir()
            tb=fixture.read_text().replace('@CASE@','explicit-abort').replace('@INJECT@',
                'force packets.abort=1;force runtime_dut.abort=1;force packets.proposal_ce=0;force runtime_dut.ce=0;')
            tb=tb.replace('@(posedge clk);#1;', """@(posedge clk);#1;
  if({packets.terminal,runtime_dut.stopped}!==2'b11)$fatal(1,"abort must latch without CE");
  @(negedge clk);force packets.abort=0;force runtime_dut.abort=0;
  @(posedge clk);#1;
  if({packets.terminal,runtime_dut.stopped}!==2'b11)$fatal(1,"abort must stay sticky");
  @(negedge clk);reset=1;
  @(posedge clk);#1;""")
            (directory/'tb.v').write_text(tb)
            log=compile_run(source,directory,monitors=monitors);traces.append(log)
            results.append(dict(source=name,monitors=monitors,summary=log.strip()))
    assert len(set(traces))==1,'Explicit abort/reset lifetime differs'
    return results


def main():
    ap = argparse.ArgumentParser(__doc__)
    ap.add_argument('--export', required=True, type=Path)
    ap.add_argument('--baseline', type=Path)
    ap.add_argument('--portable-export', type=Path)
    ap.add_argument('--portable-baseline', type=Path)
    ap.add_argument('--output', required=True, type=Path)
    ap.add_argument('--negative-only', action='store_true')
    ap.add_argument('--sampler-only', action='store_true')
    args = ap.parse_args()
    out = args.output.resolve();out.mkdir(parents=True, exist_ok=False)
    sources = [('candidate', args.export.resolve())]
    if args.baseline:
        sources.insert(0, ('baseline', args.baseline.resolve()))
    portable_sources = {}
    if args.portable_export:
        portable_sources['candidate'] = args.portable_export.resolve()
    if args.portable_baseline:
        portable_sources['baseline'] = args.portable_baseline.resolve()
    source_files = sources + [(name+'-portable', p) for name,p in portable_sources.items()]
    identities = {name: hashlib.sha256(p.read_bytes()).hexdigest() for name, p in source_files}
    fixture = Path(__file__).with_name('fixtures')/'command_packet_group_tb.v'
    results = []
    for rtt in (() if args.negative_only or args.sampler_only else (False, True)):
        rows, groups, work = vectors(rtt)
        traces = []
        for name, source in sources:
            for native in (0, 1):
                for monitors in (False, True):
                    directory = out/f'{name}-{int(rtt)}-{native}-{int(monitors)}';directory.mkdir()
                    for filename, values, digits in [('stim', [r[0] for r in rows], 18), ('members', [r[1] for r in rows], 19), ('groups', groups, 9), ('work', work, 20)]:
                        (directory/(filename+'.hex')).write_text(''.join(f'{v:0{digits}x}\n' for v in values))
                    text = fixture.read_text()
                    for key, value in {'LAST':len(rows)-1, 'GLAST':len(groups)-1, 'COUNT':len(rows), 'GROUPS':len(groups), 'MIP':int(not rtt), 'RTT':int(rtt)}.items():
                        text = text.replace('@'+key+'@', str(value))
                    (directory/'tb.v').write_text(text)
                    log = compile_run(portable_sources.get(name, source) if native == 0 else source, directory, native, monitors)
                    trace = '\n'.join(line for line in log.splitlines() if line.startswith('TRACE '))
                    traces.append(trace)
                    results.append(dict(source=name, native=native, monitors=monitors, rtt=rtt, members=len(rows), groups=len(groups),
                                        trace_sha256=hashlib.sha256(trace.encode()).hexdigest(),summary=[line for line in log.splitlines() if line.startswith('PASS ')]))
        assert len(set(traces)) == 1, 'Legal bits/state/cycles differ across source/native/monitor modes'
    negative_results = [] if args.sampler_only else negatives(sources, out)
    abort_results = [] if args.sampler_only else aborts(sources, out)
    mutant_results = [] if args.negative_only or args.sampler_only else mutants(args.export.resolve(), out)
    sampler_results = [] if args.negative_only else sampler(sources, out, portable_sources)
    assert all(hashlib.sha256(p.read_bytes()).hexdigest() == identities[name] for name, p in source_files)
    test_paths = [Path(__file__), fixture, fixture.with_name('command_packet_group_negative_tb.v'), Path(__file__).with_name('test_rtt400.py')]
    test_identities = {str(p.resolve()):hashlib.sha256(p.read_bytes()).hexdigest() for p in test_paths}
    receipt = dict(status='pass', test_inputs=test_identities, exports=identities, cases=results, negative_cases=negative_results, sampler_cases=sampler_results,abort_cases=abort_results,mutants=mutant_results,max_cycles=30000, subprocess_seconds=60)
    (out/'receipt.json').write_text(json.dumps(receipt, indent=2));print(f'PASS upstream={len(results)} negatives={len(negative_results)} receipt={out}')


if __name__ == '__main__':
    main()
