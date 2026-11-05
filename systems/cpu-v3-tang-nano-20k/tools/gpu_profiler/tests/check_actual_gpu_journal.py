"""Check the finite real native-MC/GPU fixture emitted by vliw_cycle_gpu.

This is a GPU component fixture, not a synthetic full-system capture. No CPU,
Display scanout or board measurement is claimed by this source.
"""
import json
import copy
from collections import deque
from pathlib import Path
import sys
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import system_commands
import system_gpu


def check(filename,case='rgb'):
    rows=[json.loads(line) for line in Path(filename).read_text().splitlines()]
    assert len(rows)<2000000
    columns=Path(filename).with_name('gpu-load-columns.json')
    if columns.exists():assert json.loads(columns.read_text())==system_gpu.LOAD_COLUMNS
    subs=[]
    for tick,kind,value in rows:
        if kind=='submit':subs.append(dict(id=value,start_tick=tick,end_tick=None))
        elif kind=='gpu_done':subs[value-1]['end_tick']=tick
    s=dict(axis_origin_tick=0,origin_tick=0,coverage_end_tick=rows[-1][0]+148,presents=[],
           capabilities=dict(command_control_lineage=True,gpu_signal_journal=True,gpu_master_requests=True),submissions=subs,
           gpu_load_columns=system_gpu.LOAD_COLUMNS,
           command_events=[r for r in rows if r[1].startswith('cb_')],
           gpu_journal=[r for r in rows if r[1].startswith('gpu_') and r[1]!='gpu_done'])
    s['transactions']=[];s['client_payload']=[];queues=[deque() for _ in range(7)]
    for tick,kind,value in rows:
        if kind=='accept':
            port,address,writing,beats=value
            tx=dict(id=len(s['transactions'])+1,port=port,start_tick=tick,end_tick=None,
                    address_halfword=address,writing=writing,requested_beats=beats,
                    read_bytes=0,write_bytes=0,last_data_tick=None,error=None)
            s['transactions'].append(tx);queues[port].append(tx)
        elif kind=='payload':
            port,r,w=value
            tx=next(t for t in queues[port] if t['writing'] and t['write_bytes']<t['requested_beats']*8) if w else queues[port][0]
            tx['read_bytes']+=r;tx['write_bytes']+=w;tx['last_data_tick']=tick
            s['client_payload'].append([tick,*value])
        elif kind=='terminal':
            port,error=value;queues[port].popleft().update(end_tick=tick+148,error=error)
    partial=case=='uv-abort'
    submissions,commands=system_commands.normalize(s,partial)
    data=dict(system=s,partial=partial,submissions=submissions,commands=commands)
    gpu=system_gpu.normalize(data);system_gpu.attach(data,gpu)
    # Match the production Recorder's lossless RLE, including impulse samples
    # and the first sample after a slot/owner transition.
    compact=dict(data,system=dict(s));saved=[];last_load=last_cache=last_control=None
    event_ticks={t for t,k,v in s['gpu_journal'] if k=='gpu_event'}
    for row in s['gpu_journal']:
        tick,kind,value=row;keep=True
        if kind=='gpu_load':
            keep=value!=last_load or tick in event_ticks or tick-148 in event_ticks or any(value[15:21]);last_load=value
        elif kind=='gpu_cache':keep=value!=last_cache;last_cache=value
        elif kind=='gpu_control':keep=value[1:]!=last_control;last_control=value[1:]
        if keep:saved.append(row)
    compact['system']['gpu_journal']=saved
    assert system_gpu.normalize(compact)==gpu,'production RLE changed ownership, phase durations or impulses'
    assert len(gpu['meshlets'])==(12 if case=='uv-wrap' else 4 if case=='mixed' else 2)
    assert len(gpu['quads'])>0
    from model import union
    for mesh in gpu['meshlets']:
        spans=[[q['final_tick']//148,(q['retire_tick'] if q['retire_tick'] is not None else s['coverage_end_tick'])//148]
               for q in gpu['quads'] if q['meshlet']==mesh['id'] and q['final_tick'] is not None]
        assert mesh['final']==union(spans),'Final must end at pool retirement, before the independent ROP tail'
    if not partial:
        assert all(m['vt_end'] is not None and not m['open_quad_ids'] for m in gpu['meshlets'])
        assert all(c['complete_cycle'] is not None for c in data['commands'])
        assert all(q['commit_tick'] is not None or q['empty'] for q in gpu['quads'])
    else:
        assert any(c['complete_cycle'] is None for c in data['commands'])
        assert any(m['open_quad_ids'] for m in gpu['meshlets'])
    if case=='rgb':
        assert sum(v for _,v in gpu['tracks']['light_issue'])==12
        assert sum(v for _,v in gpu['tracks']['final_issue'])==12
    else:assert sum(v for _,v in gpu['tracks']['sample_issue'])>0
    assert all(m['command']!=gpu['meshlets'][0]['command'] for m in gpu['meshlets'][1:])
    for name,change in [
        ('duplicate allocation',lambda s:s['gpu_journal'].insert(next(i for i,r in enumerate(s['gpu_journal']) if r[1]=='gpu_event' and r[2][1]=='q_alloc'),
            copy.deepcopy(next(r for r in s['gpu_journal'] if r[1]=='gpu_event' and r[2][1]=='q_alloc')))),
        ('unowned ROP',lambda s:next(r for r in s['gpu_journal'] if r[1]=='gpu_event' and r[2][1]=='q_commit')[2][2].__setitem__(0,2)),
        ('wrong T generation',lambda s:next(r for r in s['gpu_journal'] if r[1]=='gpu_event' and r[2][1]=='t_publish')[2][2].__setitem__(0,15)),
        ('unknown cache word',lambda s:next(r for r in s['gpu_journal'] if r[1]=='gpu_event' and r[2][1]=='cache_write')[2][2].__setitem__(3,0)),
    ]:
        journal=[[t,k,copy.deepcopy(v) if k=='gpu_event' else v] for t,k,v in s['gpu_journal']]
        bad=dict(data,system=dict(s,gpu_journal=journal));change(bad['system'])
        try:system_gpu.normalize(bad)
        except ValueError:pass
        else:raise AssertionError('accepted '+name)
    prefix=dict(data,partial=True,system=dict(s))
    stop=next(i for i,r in enumerate(prefix['system']['gpu_journal']) if r[1]=='gpu_event' and r[2][1]=='q_commit')
    prefix['system']['gpu_journal']=prefix['system']['gpu_journal'][:stop]
    prefix['system']['coverage_end_tick']=prefix['system']['gpu_journal'][-1][0]
    prefix['system']['transactions']=[t for t in s['transactions'] if t['start_tick']<=prefix['system']['coverage_end_tick']]
    partial=system_gpu.normalize(prefix)
    assert partial['quads'][-1]['commit_tick'] is None
    assert any(m['open_quad_ids'] for m in partial['meshlets'])
    if case=='uv-wrap':
        # A real prefix where CP/VT has finished DRAW but its published T slot
        # still owns payload. Front completion cannot close asynchronous DRAW.
        draw=next(c for c in data['commands'] if c['opcode']==3);cut=draw['release_tick']
        observed=dict(s,coverage_end_tick=cut+37,
            submissions=[dict(sub,end_tick=None) for sub in s['submissions'] if sub['start_tick']<=cut],
            command_events=[r for r in s['command_events'] if r[0]<=cut],
            gpu_journal=[r for r in s['gpu_journal'] if r[0]<=cut],transactions=[],client_payload=[])
        ss,cs=system_commands.normalize(observed,True)
        prefix=dict(system=observed,submissions=ss,commands=cs,partial=True)
        graph=system_gpu.normalize(prefix);system_gpu.attach(prefix,graph)
        first=next(c for c in cs if c['opcode']==3)
        assert first['end'] is not None and first['complete_cycle'] is None
        assert graph['meshlets'][0]['open_triangle_ids'] and not graph['meshlets'][0]['open_quad_ids']
    print(json.dumps(dict(passed=True,source='actual GPU/native MC finite component fixture',
        commands=len(commands),jobs=len(gpu['jobs']),meshlets=len(gpu['meshlets']),quads=len(gpu['quads']))))
    return data,gpu


if __name__=='__main__':check(sys.argv[1],sys.argv[2] if len(sys.argv)>2 else 'rgb')
