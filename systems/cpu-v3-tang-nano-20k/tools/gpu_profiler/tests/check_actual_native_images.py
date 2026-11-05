"""Native component oracle: two submissions overwrite the same color target."""
import json
import hashlib
from pathlib import Path
import sys
from check_actual_gpu_journal import check
import system_images

root=Path(sys.argv[1]);output=Path(sys.argv[2]);planes=output/'images'
case=sys.argv[3] if len(sys.argv)>3 else 'rgb'
data,gpu=check(root/'gpu-signals.jsonl',case)
raw=(root/'initial-memory.bin').read_bytes()
writes=[json.loads(line) for line in (root/'physical-writes.jsonl').read_text().splitlines()]
images=system_images.build(data,raw,writes,planes)
assert images['available'] and images['selected_submission_ids']==[1,2]
for sub in data['submissions']:
    first=next(c for c in data['commands'] if c['submission']==sub['id'])
    preimage=images['commands'][str(first['id'])]
    assert preimage['target_selection']=='selected-output-preimage'
    assert preimage['target_epoch'] is None
ends=[c for c in data['commands'] if c['opcode']==15]
assert len(ends)==2
for index,c in enumerate(ends,1):
    image=images['commands'][str(c['id'])]
    assert (planes/image['color_file']).read_bytes()==(root/f'oracle-{index}-color.bin').read_bytes()
    assert (planes/image['depth_file']).read_bytes()==(root/f'oracle-{index}-depth.bin').read_bytes()
assert images['commands'][str(ends[0]['id'])]['plane_sha256']!=images['commands'][str(ends[1]['id'])]['plane_sha256']
draws=[c for c in data['commands'] if c['opcode']==3]
assert all(images['commands'][str(c['id'])]['cache_override_words']>0 for c in draws)
if case=='rgb':assert images['commands'][str(draws[0]['id'])]['plane_sha256']==images['commands'][str(ends[0]['id'])]['plane_sha256']
# Adapt the real GPU component journal to exercise the system report writer.
# CPU/Display observations remain explicitly absent; this is not an actual
# whole-system run or a performance artifact for an application.
s=data['system'];s.update(version=1,quantum_hz=7992000000,logic_period=148,end_tick=s['coverage_end_tick'],
    states=[],phases=[],retires=[],native_model32=[],scanouts=[],
    scope='Synthetic report adapter around the finite real GPU/native-MC fixture. No CPU or Display capture.',
    client_boundary='Actual GPU client64 events; CPU and Display absent',native_boundary='Native MC component fixture, no board measurement')
s['capabilities'].update(client_payload64=True,native_model32=True,pin_electrical=False,instruction_owner_lineage=True)
data.update(schema='digitaldesign.gpu-profile',version=2,title='Component adapter fixture · not a system performance result',
    clock_hz=54000000,origin=0,start=0,end=s['end_tick']//148,submit=data['submissions'][0]['start'],
    gpu_end=data['submissions'][-1]['end'],cpu=[],sample_start=0,sample_end=s['end_tick']//148,images=images,
    provenance=dict(source_kind='system-cycle-emu',synthetic=True,report_inputs=dict(files={
        name:hashlib.sha256((root/name).read_bytes()).hexdigest() for name in
        ('initial-memory.bin','gpu-signals.jsonl','physical-writes.jsonl')})),
    checks=dict(component_fixture=True,board_qualified=False))
from model import write_report
write_report(data,output)
import reporting
rendered=reporting.render_report(output/'profile.json',output.with_name(output.name+'-render'),require_images=True)
assert rendered==data
for name in images['assets']:
    assert (output.with_name(output.name+'-render')/'images'/name).read_bytes()==(planes/name).read_bytes()
print(json.dumps(dict(passed=True,scope='actual GPU/native MC same-address overwrite, dirty cache and full native color/D16 guard planes',
                      native_plane_bytes=204800,images=len(images['commands']),raw_assets=len(images['assets']))))
