"""Exercise the public new-capture build/render path with real component facts.

Only GPU/native-MC journals are real. The capture envelope is synthetic and
contains no CPU/Display observations, application identity or performance claim.
"""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
import system_emu

source,output=map(Path,sys.argv[1:3])
capture=output/'capture';capture.mkdir(parents=True)
shutil.copyfile(source/'initial-memory.bin',capture/'initial-memory.bin')
(capture/'program.bin').write_bytes(b'SYNTHETIC component envelope, no CPU program')
(capture/'source-executable.bin').write_bytes(b'No system executable was run by this adapter')
rows=[json.loads(line) for line in (source/'gpu-signals.jsonl').read_text().splitlines()]
rows += [json.loads(line) for line in (source/'physical-writes.jsonl').read_text().splitlines()]
rows.sort(key=lambda r:r[0])
end=(rows[-1][0]//148+2)*148
(capture/'system-events.jsonl').write_text(''.join(json.dumps(row)+'\n' for row in rows))
meta=dict(schema=system_emu.SOURCE,version=1,source_kind='system-cycle-emu',synthetic=True,
          scope='Component importer fixture; no CPU, Display, application or board qualification',
          ports=system_emu.PORTS,quantum_hz=7992000000,logic_period=148,
          origin_tick=0,end_tick=end,coverage_end_tick=end,events=len(rows),reached=True,
          truncated=False,scanouts=[],gpu_load_columns=json.loads((source/'gpu-load-columns.json').read_text()),
          capabilities=dict(client_payload64=True,native_model32=True,pin_electrical=False,
              command_control_lineage=True,gpu_signal_journal=True,gpu_master_requests=True,instruction_owner_lineage=True),
          image_journal=dict(available=True,preimage='initial-memory.bin',completed_target_budget=2))
(capture/'capture.json').write_text(json.dumps(meta))
def cli(*args):
    run=subprocess.run([sys.executable,'-B',str(ROOT/'profiler.py'),*map(str,args)],
                       capture_output=True,text=True,timeout=180)
    assert run.returncode==0,run.stderr
    return json.loads(run.stdout)
build=cli('build',capture,'--out',output/'report','--require-images')
assert build['commands']==30 and build['native_image_count']==30,build
cli('render',output/'report/profile.json','--out',output/'render','--require-images')
profile=json.loads((output/'report/profile.json').read_text())
assert profile['provenance']['synthetic'] and not profile['system']['scanouts']
assert profile['system']['scope']==meta['scope']
ends=[c for c in profile['commands'] if c['opcode']==15]
for index,c in enumerate(ends,1):
    image=profile['images']['commands'][str(c['id'])]
    for role in ('color','depth'):
        expected=(source/f'oracle-{index}-{role}.bin').read_bytes()
        assert (output/'report/images'/image[role+'_file']).read_bytes()==expected
        assert (output/'render/images'/image[role+'_file']).read_bytes()==expected
before={str(p.relative_to(output/'report')):hashlib.sha256(p.read_bytes()).hexdigest()
        for p in (output/'report').rglob('*') if p.is_file()}
cli('build',capture,'--out',output/'report','--require-images','--replace-report')
mapping=json.loads((output/'report/report-replacement.json').read_text())
archive=Path(mapping['archive_directory'])
assert mapping['files']==before
assert before=={str(p.relative_to(archive)):hashlib.sha256(p.read_bytes()).hexdigest()
                for p in archive.rglob('*') if p.is_file()}
print(json.dumps(dict(passed=True,scope=meta['scope'],commands=30,native_images=30,
                      official_cli=['build','render','build --replace-report'])))
