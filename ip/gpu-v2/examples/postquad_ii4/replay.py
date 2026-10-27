"""Bounded complete 400x240 post-interpolator replay, independent RGB/DRAM oracle.

Usage: replay.py SOURCE OUTPUT [hot|hot-stall|cold-stall|rich|paid-bad|paid-abort]
Set IVERILOG_EXE/VVP_EXE or provide the tools on PATH; native needs GOWIN_HOME.
"""
from pathlib import Path
import hashlib, inspect, json, os, shutil, subprocess, sys, time
sys.dont_write_bytecode = True
import oracle

HERE = Path(__file__).resolve().parent
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()

def main():
    source, out = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve()
    case = sys.argv[3] if len(sys.argv) > 3 else 'hot-stall'
    assert case in ('hot', 'hot-stall', 'cold-stall', 'rich', 'paid-bad', 'paid-abort')
    assert not out.exists(), 'Use a fresh output directory'
    fixtures = HERE / 'fixtures'
    for name, digest in json.loads((fixtures/'manifest.json').read_text()).items():
        assert sha(fixtures/name) == digest, name
    out.mkdir(parents=True)
    shutil.copyfile(source, out/'dut.v')
    tb = (fixtures/(case+'.v')).read_text()
    (out/'tb.v').write_text(tb)
    rows = oracle.values(fixtures/'rom.mem')
    cold = case == 'cold-stall'
    for j in range(len(rows)//8):
        x = 398 - (16*(j%9) if cold else 2*(j%8))
        for lane in range(8):
            head = rows[j*8+lane] >> 104
            head = (head & 255) | ((x//2)<<8) | (119<<16)
            rows[j*8+lane] = (head<<104) | (rows[j*8+lane]&((1<<104)-1))
    (out/'rom.mem').write_text(''.join(f'{v:032x}\n' for v in rows))
    for name in ('rgb.mem', 'sample.mem', 'light.mem'):
        shutil.copyfile(fixtures/name, out/name)
    initial = list((fixtures/'texture.bin').read_bytes()) + [0]*192000 + [255]*192000
    (out/'initial.mem').write_text(''.join(f'{v:02x}\n' for v in initial))
    # This exact independent implementation was qualified before fixture adoption.
    # Changing the surface only changes tile stride and plane bases.
    body = inspect.getsource(oracle.framebuffer).replace('yy//16*32','yy//16*25').replace('da=49152','da=224768')
    namespace = dict(oracle.__dict__)
    exec(body, namespace)
    gold = namespace['framebuffer'](rows, oracle.values(fixtures/'rgb.mem'), initial, tb)
    (out/'gold.mem').write_text(''.join(f'{v:02x}\n' for v in gold))
    native = source.read_text().startswith('`define GPU_V2_GOWIN_DSP')
    compile = [os.environ.get('IVERILOG_EXE', 'iverilog'), '-g2012', '-s', 'tb', '-o', 'sim.vvp', 'dut.v', 'tb.v']
    if native:
        compile.append(str(Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v'))
    started = time.time()
    for label, command in [('compile', compile), ('run', [os.environ.get('VVP_EXE', 'vvp'), 'sim.vvp'])]:
        run = subprocess.run(command, cwd=out, capture_output=True, timeout=180)
        (out/(label+'.log')).write_bytes(run.stdout+run.stderr)
        assert run.returncode == 0, (run.stdout+run.stderr).decode(errors='replace')[-3000:]
    assert 'PASS' in run.stdout.decode()
    if case in ('hot', 'hot-stall', 'cold-stall'):
        expected = [None]*(len(rows)//2)
        for line in (out/'issues.txt').read_text().splitlines():
            j,l,t,tex,s,g,h = line.split()
            expected[int(j)*4+int(l)] = oracle.shade(int(t,16),int(tex,16),int(s,16),int(g),int(h))
        assert oracle.values(out/'actual-rgb.mem') == expected, 'independent final RGB'
        gold = namespace['framebuffer'](rows, expected, initial, tb)
        assert oracle.values(out/'actual-memory.mem') == list(gold), 'independent full-plane memory'
        commits = oracle.values(out/'actual-commit.mem')
        assert len(commits) == len(rows)//8
        for j,value in enumerate(commits):
            x = 398 - (16*(j%9) if cold else 2*(j%8))
            assert value == ((x<<12)|(238<<4)|((rows[j*8]>>104)&15)), (j,value,x)
    receipt = dict(passed=True, case=case, backend='native' if native else 'portable', source_sha256=sha(source), testbench_sha256=sha(out/'tb.v'), oracle_sha256=sha(HERE/'oracle.py'), seconds=time.time()-started, max_edges=200000, log=run.stdout.decode())
    (out/'receipt.json').write_text(json.dumps(receipt, indent=2))
    print(json.dumps(receipt, indent=2))

if __name__ == '__main__':
    main()
