"""Same-input GPU collectors: compare actual points and the shared model."""
from collections import defaultdict
import json
from pathlib import Path
import sys
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from event_protocol import validate
from reporting import build_report
from model import build_capture


def compare(root):
    root=Path(root);streams=[];metadata=[]
    for backend in ('emu','rtl-capture'):
        folder=root/backend;meta=validate(json.loads((folder/'capture.json').read_text()))
        rows=[json.loads(line) for line in (folder/'events.jsonl').read_text().splitlines()]
        assert len(rows)==meta['events']
        streams.append(rows)
        metadata.append(meta)
    bench=(root/'rtl/tb.v').read_text()
    assert 'always #9.259260 clk=~clk' in bench and '`timescale 1ns/1fs' in bench,'profiling fixture clock differs from metadata'
    assert all(m['clock_hz']==54000000 and m['logic_period']==148 for m in metadata)
    first=min(t for t,k,v in streams[0] if k=='cb_submit')
    partial=any(m['truncated'] or not m['reached'] for m in metadata)
    last=min(m['coverage_end_tick'] for m in metadata)-1 if partial else max(t for t,k,v in streams[1] if k=='cb_drain')
    def points(rows):
        result=defaultdict(list)
        for t,k,v in rows:
            if type(t) is not int:continue # exact native clock domain is checked separately
            if not first<=t<=last:continue
            if k=='gpu_event':result[v[1]].append([t,v[0],v[2]])
            elif k.startswith('cb_'):result[k].append([t,v])
            elif k in ('gpu_load','gpu_control','gpu_cache'):result[k].append([t,v])
        return result
    a,b=map(points,streams);errors=[]
    for key in sorted(a.keys()|b.keys()):
        if a[key]!=b[key]:
            i=next((i for i,(x,y) in enumerate(zip(a[key],b[key])) if x!=y),min(len(a[key]),len(b[key])))
            errors.append(dict(point=key,index=i,counts=[len(a[key]),len(b[key])],
                               emu=a[key][i:i+1],rtl=b[key][i:i+1]))
    if errors:
        print(json.dumps(errors,indent=2));raise AssertionError('Collector points differ')
    models=[build_capture(root/backend,allow_partial=partial) for backend in ('emu','rtl-capture')]
    for backend in ('emu','rtl-capture'):
        if not (root/(backend+'-report')).exists():
            build_report(root/backend,root/(backend+'-report'),allow_partial=partial)
    # Profiles can include a different number of trailing idle edges. Compare
    # command/async ownership and payload counts, retaining original timestamps.
    for key in ('submissions','commands','meshlets','jobs','dma_transactions'):
        assert models[0][key]==models[1][key],key
    for key in ('wire_bytes','effective_bytes','data_beats','completed_transactions'):
        assert models[0]['memory']['summary'][key]==models[1]['memory']['summary'][key],key
    result=dict(passed=True,edge_points={k:len(v) for k,v in a.items()},
                clock_hz=54000000,rtl_logic_period_fs=18518520,
                commands=len(models[0]['commands']),meshlets=len(models[0]['meshlets']),
                retried_commands=sum(c['issue_tick'] is not None and c['execute_tick'] is not None and c['issue_tick']>c['execute_tick'] for c in models[0]['commands']),
                open_async_after_release=[c['id'] for c in models[0]['commands'] if c['release_tick'] is not None and c['complete_cycle'] is None],
                partial=partial,
                scope='same recorded native-MC GPU inputs; no CPU/Display or RTL MC/pin comparison')
    (root/'comparison.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result))


if __name__=='__main__':compare(sys.argv[1])
