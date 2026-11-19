"""Bloxorz level-29 launcher and ordinary-profile v1 sidecar codec."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import time


# This application consumes the system profiler; the model knows no game policy.
PROFILER = Path(__file__).resolve().parents[3] / 'tools/gpu_profiler'
if __name__ == '__main__':
    sys.path.insert(0, str(PROFILER))
from interval_activity import interval_activity


def require(value, message):
    if not value:
        raise ValueError('Ordinary runner: '+message)


def derive(events, plan, origin, end):
    require(type(events) is list and len(events)<=100000, 'sidecar bound')
    dedicated = plan.get('dedicated_final_wait') is True
    previous=origin;clear=None;intervals=[];selected=[];inputs=[]
    for e in events:
        tick=e.get('tick')
        require(type(tick) is int and previous<=tick<=end, 'sidecar time')
        previous=tick;kind=e.get('kind')
        require(kind in ('device','gate','site','input','selected','raster'), 'sidecar kind')
        if kind=='input':
            require(e.get('buttons_active_low') in (65535,65503),'physical input')
            inputs.append(e)
        elif kind=='gate':
            require(e.get('name') in ('before_input','consumed','sample') and
                    list(e.get('pc',[]))==plan['gates'][e['name']] and type(e.get('state')) is dict,'committed gate')
        elif kind=='site':
            require(e.get('name') in ('vblank_return','final_wait_return') and
                    list(e.get('pc',[]))==plan['sites'][e['name']]
                    and e.get('observation','retired')==plan.get('site_observation','retired'), 'timing site observation')
            if not dedicated and e.get('observation')=='first-unheld-execute':
                require(type(e.get('finish_r10')) is int and 0<=e['finish_r10']<65536
                        and (e['name']!='final_wait_return' or e['finish_r10']!=0),'final wait finish discriminator')
        elif kind=='selected':
            require(e.get('label') in ('warm','stable-A','rolling-B') and type(e.get('state')) is dict,
                    'selection')
            selected.append(e)
        elif kind=='raster':
            require(e.get('effective_pixels')==384000 and type(e.get('presentation')) is int and
                    e['presentation']>0 and origin<=e.get('start_tick',-1)<=tick,'stable raster')
        else:
            require(type(e.get('device')) is int and 0<=e['device']<8 and
                    type(e.get('channel')) is int and 0<=e['channel']<16 and
                    type(e.get('write')) is bool and type(e.get('value')) is int and 0<=e['value']<65536
                    and type(e.get('pc')) is list and len(e['pc'])==2
                    and type(e.get('committed')) is bool,'CPU device edge')
            if e['committed'] and e['device']==0 and e['channel']==11:
                if plan.get('site_observation')=='first-unheld-execute':
                    key='clear' if e['write'] else 'read'
                    site=plan['clock_sites'][key]
                    require(e['pc']==[site['code_segment'],site['pc']],'unexpected ch11 source PC')
                if e['write'] and e['value']&1:
                    clear=e
                elif not e['write']:
                    row=dict(read_tick=tick,read_pc=e['pc'],microseconds=e['value'],end_epoch=e.get('epoch'),
                             finish_r10=e.get('finish_r10'),
                             wait_started_r8=e.get('wait_started_r8'),wait_start_epoch_r9=e.get('wait_start_epoch_r9'),
                             clear_tick=None if clear is None else clear['tick'],
                             clear_pc=None if clear is None else clear['pc'],
                             start_epoch=None if clear is None else clear.get('epoch'))
                    row['cycles']=None if clear is None else (tick-clear['tick'])//148
                    if clear is not None:
                        require((tick-clear['tick'])%148==0 and row['cycles']>0,'timer clock alignment')
                        require(e['value']==min(65535,(row['cycles']-1)//54),'timer prescaler/read-before-edge')
                    row['valid']=clear is not None and e['value']<65535
                    if dedicated:
                        row['reason']='actual CPU clear/RECV' if row['valid'] else 'cold or saturated'
                    else:
                        row['valid']=row['valid'] and row['start_epoch'] is not None and row['start_epoch']==row['end_epoch']
                        if plan.get('site_observation')=='first-unheld-execute':
                            row['valid']=row['valid'] and type(row['wait_started_r8']) is int and row['wait_started_r8']!=0 \
                                         and row['wait_start_epoch_r9']==row['end_epoch']
                        row['reason']='actual CPU clear/RECV with unchanged epoch' if row['valid'] else 'cold, saturated or changed/unknown epoch'
                    intervals.append(row)
                    if dedicated:
                        clear=None
    require(inputs and inputs[0]['buttons_active_low']==65535,'initial neutral')
    require([e['buttons_active_low'] for e in inputs] in ([65535],[65535,65503],[65535,65503,65535]),
            'one real press/release sequence')
    return dict(events=events,plan=plan,intervals=intervals,selections=selected,
                scope='Actual CPU MMIO Execute edges; watches excluded. '
                      + ('' if dedicated else 'RAM epoch sampled passively after that edge. ') +
                      'Timer interval is distinct from applied-to-applied frame window.')


def import_sidecar(folder, meta):
    plan=json.loads((folder/'runner-plan.json').read_text())
    events=[json.loads(line) for line in (folder/'runner-events.jsonl').read_text().splitlines()]
    require(len(events)==meta.get('runner_sidecar_events'),'sidecar event count')
    result=derive(events,plan,meta['origin_tick'],meta['end_tick'])
    result['raw_files']={name:hashlib.sha256((folder/name).read_bytes()).hexdigest() for name in meta['runner_files']}
    return result


def validate_report(data):
    s=data['system'];r=s['runner']
    require({k:v for k,v in r.items() if k!='raw_files'}==derive(r['events'],r['plan'],s['origin_tick'],s['end_tick']),'derived sidecar differs')
    files=data['provenance']['report_inputs']['files']
    require(r.get('raw_files')=={name:files[name] for name in ('runner-plan.json','runner-events.jsonl')},'sidecar raw identity')
    require(data['provenance'].get('runner_version')==1,'runner source version')
    for row in r['events']:
        if row['kind']=='raster':
            require(any(f['start_tick']==row['start_tick'] and f['end_tick']==row['tick'] and
                        f['applied_present_count']==row['presentation'] and
                        f['front_at_start_halfword']==row['front_halfword'] and
                        not f['present_changed_during_scanout'] for f in s['scanouts']),
                    'sidecar raster not bound to saved registered image')


def timing_report(data):
    s=data['system'];r=s.get('runner')
    if not r:return None
    requests=[(tick,value) for tick,kind,value in s['presents'] if kind=='present_request']
    rows=[]
    for row in r['intervals']:
        n=next((n for tick,n in requests if tick>row['read_tick']),None)
        def site(name,tick):
            return next((e['tick'] for e in reversed(r['events']) if e['kind']=='site' and e['name']==name
                         and e.get('next_present')==n and e.get('epoch')==row['end_epoch']
                         and tick is not None and e['tick']<=tick),None)
        begin=site('vblank_return',row['clear_tick']);finish=site('final_wait_return',row['read_tick'])
        bound=begin is not None and finish is not None and row['clear_tick']<=finish<=row['read_tick']
        if r['plan'].get('site_observation')=='first-unheld-execute' and not r['plan'].get('dedicated_final_wait'):
            bound=bound and row['finish_r10'] is not None and row['finish_r10']!=0
        return_cycles=(finish-begin)//148 if bound else None
        rows.append(dict(row,presentation=n,vblank_return_tick=begin,final_wait_return_tick=finish,
                         source_endpoints_bound=bound,
                         return_interval_cycles=return_cycles,
                         return_within_900000_cycles=(return_cycles<=900000) if row['valid'] and bound else None,
                         return_to_clear_cycles=(row['clear_tick']-begin)//148 if bound else None,
                         wait_return_to_read_cycles=(row['read_tick']-finish)//148 if bound else None,
                         activity=interval_activity(s,row['clear_tick'],row['read_tick']),
                         within_900000_cycles=(row['cycles']<=900000) if row['valid'] and bound else None))
    states=[dict(presentation=e['next_present'],tick=e['tick'],state=e['state']) for e in r['events']
            if e['kind']=='gate' and e['name']=='sample']
    return dict(intervals=rows,selections=r['selections'],frame_states=states,scope=r['scope'])


def verify_file(root, binding):
    path=root/binding['file'];raw=path.read_bytes()
    require(len(raw)==binding['bytes'] and hashlib.sha256(raw).hexdigest()==binding['sha256'],
            'file identity '+str(path))
    return path


def launch(plan_file):
    started=time.monotonic();plan_file=Path(plan_file).resolve();root=plan_file.parent
    p=json.loads(plan_file.read_text())
    require(p.get('schema')=='digitaldesign.ordinary-profile-plan' and p.get('version')==1,'plan schema')
    runtime=verify_file(root,p['runtime']);verify_file(root,p['program'])
    require(p.get('bindings'),'source/resource bindings missing')
    for binding in p['bindings']:verify_file(root,binding)
    total=p['max_total_host_seconds']
    require(0<p['max_host_seconds']<total<=7200 and total-p['max_host_seconds']>=60,'total wall/export reserve')
    output=root/p['output'];require(not output.exists(),'new capture directory required')
    argv=[str(runtime),str(plan_file)]
    receipt=plan_file.with_suffix('.launch.json')
    require(not receipt.exists(),'launch receipt exists; no automatic rerun')
    record=dict(argv=argv,plan_sha256=hashlib.sha256(plan_file.read_bytes()).hexdigest(),
                program_sha256=p['program']['sha256'],runtime_sha256=p['runtime']['sha256'],
                max_total_host_seconds=total,complete=False)
    receipt.write_text(json.dumps(record,indent=2)+'\n')
    def execute(command,label):
        with plan_file.with_suffix('.'+label+'.stdout.log').open('wb') as stdout, plan_file.with_suffix('.'+label+'.stderr.log').open('wb') as stderr:
            process=subprocess.Popen(command,cwd=root,stdout=stdout,stderr=stderr)
            try:
                return process.wait(timeout=max(0,total-(time.monotonic()-started))),False
            except subprocess.TimeoutExpired:
                process.kill();process.wait()
                return process.returncode,True
    status,timed_out=execute(argv,'capture')
    record.update(capture_exit_code=status,wall_timeout=timed_out,complete=False)
    capture=output/'capture.json'
    if not timed_out and status in (0,2) and capture.is_file():
        meta=json.loads(capture.read_text())
        selected=meta['runner_selected_presentations']
        complete=meta['selected_rasters']
        frames=[n for n in selected if n in complete]
        missing=[label for label,key in [('stable-A','stable_present'),('rolling-B','rolling_present')]
                 if meta.get(key) is None or meta[key] not in complete]
        missing+=['F'+str(n) for n in (1,2) if n not in complete]
        record['required_scopes_missing']=missing
        report=root/p['report_output'];require(not report.exists(),'new report directory required')
        command=[sys.executable,'-B',str(PROFILER/'profiler.py'),'build',str(output),
                 '--out',str(report),'--allow-partial']
        if frames:command+=['--frames',','.join(map(str,frames)),'--require-images']
        record['report_argv']=command
        status,timeout=execute(command,'report')
        record.update(report_exit_code=status,wall_timeout=timeout,
                      complete=not missing and status==0 and not timeout and record['capture_exit_code']==0)
    record['total_host_seconds']=time.monotonic()-started
    receipt.write_text(json.dumps(record,indent=2)+'\n')
    print(json.dumps(record,indent=2))
    return 0 if record['complete'] else 2


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('plan')
    raise SystemExit(launch(parser.parse_args().plan))
