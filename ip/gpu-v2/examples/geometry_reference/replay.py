"""Replay an immutable historical geometry reference, not a production GPU.

Usage: replay.py CASE OUTPUT. Tools: IVERILOG_EXE, VVP_EXE, GOWIN_HOME.
"""
from pathlib import Path
import hashlib, json, os, shutil, subprocess, sys

root = Path(__file__).resolve().parent
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
case, out = sys.argv[1], Path(sys.argv[2]).resolve()
row = next(r for r in json.loads((root/'cases.json').read_text()) if r['case'] == case)
assert not out.exists(), 'Use a fresh output directory'
manifest = json.loads((root/'manifest.json').read_text())
inputs = list((root/'cases'/case).iterdir()) + [root/row['source']]
for p in inputs:
    assert sha(p) == manifest[p.relative_to(root).as_posix()], p
out.mkdir(parents=True)
for p in inputs:
    shutil.copyfile(p, out/('dut.v' if p.name == row['source'] else p.name))
command = [os.environ.get('IVERILOG_EXE','iverilog'), '-g'+row['language'], '-s','tb', '-o','sim.vvp']
if row['top_gsr']:
    command += ['-s','GSR']
command += ['dut.v','tb.v',str(Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v')]
for label, cmd, limit in [('compile',command,90),('run',[os.environ.get('VVP_EXE','vvp'),'sim.vvp'],row['timeout_seconds'])]:
    run = subprocess.run(cmd,cwd=out,capture_output=True,timeout=limit)
    (out/(label+'.log')).write_bytes(run.stdout+run.stderr)
    assert run.returncode == 0, (run.stdout+run.stderr).decode(errors='replace')[-4000:]
assert 'PASS' in run.stdout.decode() and 'FAIL' not in run.stdout.decode()
receipt = dict(case=case,passed=True,source_sha256=sha(out/'dut.v'),scope='Frozen standalone reference and captured oracle only; no current canonical input/production/board qualification.',log=run.stdout.decode())
(out/'receipt.json').write_text(json.dumps(receipt,indent=2))
print(json.dumps(receipt,indent=2))
