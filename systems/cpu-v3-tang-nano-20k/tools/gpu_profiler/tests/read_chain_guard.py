"""Bounded native-MC regression for the actual replay initialized-read guard."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

sys.dont_write_bytecode = True
TOOLS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS))
from replay import load_contract


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(peer, output):
    peer, output = peer.resolve(), output.resolve()
    contract = load_contract(peer)
    output.mkdir(parents=True, exist_ok=False)
    template_path = TOOLS / 'replay_tb.v'
    template = template_path.read_text()
    # Reuse the actual MC instance, clocks, pin model and complete guard block.
    # The GPU instance and full-frame seed/dump stimulus are replaced by two reads.
    gpu = template.index('gpu_v2_vliw_command gpu(')
    mc = template.index('GowinSdramCombination mc(')
    guard_end = template.index('integer write_location;')
    stimulus = Path(__file__).with_name('read_chain_guard_tb.v')
    bench = output / 'tb.v'
    bench.write_text(template[:gpu] + template[mc:guard_end] + stimulus.read_text())
    primitive = Path(os.environ['GOWIN_HOME']) / 'IDE/simlib/gw2a/prim_sim.v'
    sources = [peer / name for name in contract['files'] if name.endswith('.v')]
    sources += [primitive, bench]
    executable = output / 'test.vvp'
    command = [os.environ.get('IVERILOG_EXE', 'iverilog'), '-g2012', '-D__ICARUS__',
               '-s', 'tb', '-o', str(executable), *map(str, sources)]
    result = subprocess.run(command, cwd=output, capture_output=True, timeout=30)
    (output / 'compile.log').write_bytes(result.stdout + result.stderr)
    if result.returncode:
        raise RuntimeError('Read-chain regression compile failed; see compile.log')
    results = []
    for missing in (False, True):
        name = 'missing-successor' if missing else 'initialized-successor'
        command = [os.environ.get('VVP_EXE', 'vvp'), str(executable)]
        if missing:
            command.append('+MISSING_SUCCESSOR')
        result = subprocess.run(command, cwd=output, capture_output=True, timeout=15)
        log = result.stdout + result.stderr
        (output / (name + '.log')).write_bytes(log)
        if missing:
            passed = (result.returncode != 0 and
                      b'Uncaptured/uninitialized read: logical byte=00000080' in log and
                      b'take=0 read_chain=1' in log)
        else:
            passed = result.returncode == 0 and (
                b'READ_CHAIN_GUARD_PASS requests=2 take=1 read_chain=1 terminal=2 pairs=32' in log)
        results.append(dict(case=name, passed=passed, exit_code=result.returncode,
                            log=str(output / (name + '.log'))))
    report = dict(scope=__doc__, passed=all(r['passed'] for r in results),
                  max_cycles=10000, compile_timeout_seconds=30, run_timeout_seconds=15,
                  guard_template=dict(path=str(template_path), sha256=digest(template_path)),
                  stimulus=dict(path=str(stimulus), sha256=digest(stimulus)),
                  contract=dict(path=str(peer / 'stream-contract.json'),
                                sha256=digest(peer / 'stream-contract.json')),
                  sources=[dict(path=str(p), sha256=digest(p)) for p in sources], results=results)
    (output / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(dict(passed=report['passed'], results=results)))
    if not report['passed']:
        raise RuntimeError('Read-chain guard regression failed; see result.json and case logs')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--peer', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    run(args.peer, args.out)
