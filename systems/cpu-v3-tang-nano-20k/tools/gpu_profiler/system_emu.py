"""Import bounded full-system cycle observations into the existing report/viewer."""
import base64
from collections import deque
import csv
import hashlib
from heapq import merge
import json
from pathlib import Path
import math
import struct
import zlib

from application_sidecar import codec

SOURCE = 'digitaldesign.system-emu-capture'
PORTS = ['display', 'instruction', 'data', 'dma', 'gpu_ro', 'gpu_fb_r', 'gpu_fb_w']
LIMIT = (1 << 53) - 1


def require(condition, message):
    if not condition:
        raise ValueError('System emu: ' + message)


def integer(value, minimum=0):
    return type(value) is int and minimum <= value <= LIMIT


def native_time(value):
    return type(value) in (int,float) and math.isfinite(value) and 0<=value<=LIMIT


def png(rgb):
    require(len(rgb) == 800 * 480 * 3, 'incomplete registered raster')
    def chunk(kind, body):
        data = kind + body
        return struct.pack('>I', len(body)) + data + struct.pack('>I', zlib.crc32(data) & 0xffffffff)
    rows = b''.join(b'\0' + rgb[y*2400:(y+1)*2400] for y in range(480))
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', 800, 480, 8, 2, 0, 0, 0))
            + chunk(b'sRGB', b'\0') + chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b''))


def evidence(folder, meta):
    from event_protocol import SCHEMA, filename
    names = ['capture.json',filename(meta)]
    names += meta.get('evidence_files',['program.bin','source-executable.bin'])
    names += [f['file'] for f in meta['scanouts']]
    if meta.get('image_journal',{}).get('available'):
        for key in ('preimage','known_preimage'):
            name=meta['image_journal'].get(key)
            if name and name not in names:names.append(name)
    if meta.get('runner_version')==1:
        require(meta.get('runner_files')==['runner-plan.json','runner-events.jsonl'], 'runner evidence names')
        names += meta['runner_files']
    result = {}
    for name in names:
        require(Path(name).name == name and name not in result, 'unsafe/duplicate evidence filename')
        result[name] = hashlib.sha256((folder/name).read_bytes()).hexdigest()
    return result


def build(folder, output, *, allow_partial=False, binding=None, require_images=False,
          frames=(), source_identity=None, only_frame=None, preview='display1'):
    from model import write_report
    folder, output = Path(folder), None if output is None else Path(output)
    meta = json.loads((folder/'capture.json').read_text())
    from event_protocol import SCHEMA, validate as validate_protocol, filename, time_value
    common=meta.get('schema')==SCHEMA
    if common:validate_protocol(meta)
    else:require(meta.get('schema') == SOURCE and meta.get('version') == 1, 'unsupported capture')
    require(meta.get('source_kind') in ('system-cycle-emu','gpu-rtl','gpu-cycle-emu') and meta.get('ports') == PORTS, 'source/ports')
    clock=meta.get('clock_hz',54000000)
    require(meta.get('quantum_hz') == clock*148 and meta.get('logic_period') == 148, 'timebase')
    require(all(integer(meta.get(k)) for k in ('origin_tick','end_tick','coverage_end_tick','events')), 'capture bounds')
    origin, end = meta['origin_tick'], meta['coverage_end_tick']
    require(origin <= end <= meta['end_tick'] and meta['end_tick'] > origin, 'empty/reversed capture')
    files = evidence(folder, meta)
    if meta.get('runner_version')==1:
        require(integer(meta.get('event_limit'),1) and meta['event_limit']<=112000000
                and meta.get('event_byte_limit')==4294967296
                and (folder/filename(meta)).stat().st_size==meta.get('event_bytes')<=4294967296,
                'explicit runner event/byte ceilings')
    if binding:
        expected = json.loads(Path(binding).read_text())
        require(expected.get('schema') == 'digitaldesign.system-emu-report-inputs' and expected.get('files') == files, 'binding mismatch')
    states, payload, native, phases, present, retires, tx = [], [], [], [], [], [], []
    cpu_spans=[];cpu_open={}
    command_events = [];native_times=[]
    gpu_journal, physical_writes = [], []
    queues = [deque() for _ in PORTS]
    subs, opened, stopped = [], None, False
    previous, count = origin, 0
    with (folder/filename(meta)).open(encoding='utf-8') as stream:
        for line in stream:
            row = json.loads(line)
            require(type(row) is list and len(row) == 3, 'event envelope')
            tick, kind, value = row
            if common and meta['version']==2 and kind in ('native','physical_write'):
                require(type(tick) is list,'v2 native events require exact rational time')
            if common and type(tick) is list:
                native_times.append([tick,kind]);tick=time_value(tick,kind,meta)
                row=[tick,kind,value]
            require((native_time(tick) if common and meta['version']==2 and kind in ('native','physical_write') else integer(tick)) and previous <= tick <= end, 'unordered/out-of-range tick')
            previous, count = tick, count + 1
            require(count <= min(meta.get('event_limit',10000000),112000000 if meta.get('runner_version')==1 else 10000000), 'event limit')
            if kind == 'state':
                require(type(value) is list and len(value) == 10 and all(integer(v) for v in value) and all(v <= 1 for v in value[:8]) and value[8] < 128 and value[9] < 128, 'state')
                states.append([tick, value])
            elif kind == 'cpu':
                require(type(value) is list and len(value) == 2 and all(integer(v) for v in value), 'CPU counters')
                require(not retires or value[0] >= retires[-1][1], 'CPU counter reversed')
                retires.append([tick, *value])
            elif kind == 'phase':
                require(integer(value) and value < 256, 'CP phase')
                phases.append([tick,value])
            elif kind in ('accept','payload','terminal'):
                require(type(value) is list and value and integer(value[0]) and value[0] < 7 and value[0] != 3, 'client port')
                port = value[0]
                if kind == 'accept':
                    require(len(value) in (3,4) and integer(value[1]) and type(value[2]) is bool and (len(value)==3 or integer(value[3],1) and value[3]<=16), 'descriptor')
                    item = dict(id=len(tx)+1, port=port, start_tick=tick, end_tick=None, address_halfword=value[1], writing=value[2], requested_beats=value[3] if len(value)==4 else None, read_bytes=0, write_bytes=0, last_data_tick=None, error=None)
                    tx.append(item); queues[port].append(item)
                else:
                    require(bool(queues[port]), 'unowned payload/terminal')
                    item = queues[port][0]
                    if kind == 'payload':
                        require(len(value) == 3 and all(integer(v) and v in (0,8) for v in value[1:]) and sum(value[1:]) == 8, 'payload width')
                        if value[2]:
                            candidates=[t for t in queues[port] if t['writing'] and (t['requested_beats'] is None or t['write_bytes'] < t['requested_beats']*8)]
                            require(bool(candidates), 'write payload direction/owner')
                            item=candidates[0]
                        require((value[2] == 8) == item['writing'], 'payload direction')
                        item['read_bytes'] += value[1]; item['write_bytes'] += value[2]
                        item['last_data_tick'] = tick
                        payload.append([tick,*value])
                    else:
                        require(len(value) == 2 and type(value[1]) is bool, 'terminal error')
                        item['end_tick'] = tick + 148; item['error'] = value[1]; queues[port].popleft()
            elif kind == 'native':
                require(type(value) is list and len(value) == 3 and all(integer(v) for v in value) and value[0] in (0,4) and value[1] in (0,4) and value[0]+value[1] == 4 and value[2] <= value[1], 'native32 slot')
                native.append([tick,*value])
            elif kind == 'submit':
                require(integer(value,1) and value == len(subs)+1 and opened is None, 'submission identity')
                opened = dict(id=value, start_tick=tick, end_tick=None); subs.append(opened)
            elif kind == 'gpu_done':
                require(opened is not None and opened['id'] == value, 'GPU completion owner')
                opened['end_tick'] = tick; opened = None
            elif kind == 'gpu_stopped':
                require(type(value) is list and len(value)==2 and opened is not None
                        and value[0]==opened['id'] and value[1] in ('abort','reset'),'GPU stop owner/reason')
                opened.update(stop_tick=tick,stop_reason=value[1]);opened=None;stopped=True
            elif kind in ('present_request','present_applied'):
                require((kind == 'present_request' and integer(value,1)) or
                        (kind == 'present_applied' and type(value) is list and len(value) == 2 and all(integer(v) for v in value)), 'present')
                present.append([tick,kind,value])
            elif kind in ('gpu_load','gpu_cache','gpu_event','gpu_control'):
                gpu_journal.append(row)
            elif kind == 'physical_write':
                require(type(value) is list and len(value)==3 and all(integer(v) for v in value)
                        and value[0]%4==0 and value[0]<8388608 and value[1]<=0xffffffff
                        and value[2]<16 and (common and meta['version']==2 or tick%74==37), 'physical masked write')
                physical_writes.append(row)
            elif type(kind) is str and kind.startswith('cb_'):
                command_events.append(row)
            elif common and kind=='capture_end':
                require(type(value) is bool and tick==end,'capture end boundary')
            elif common and kind in ('cpu_begin','cpu_end'):
                require(type(value) is list and integer(value[0],1),'CPU marker identity')
                if kind=='cpu_begin':
                    require(len(value)==3 and integer(value[1]) and type(value[2]) is str and value[0] not in cpu_open,'CPU begin marker')
                    item=dict(id=len(cpu_spans)+1,stage_id=value[0],frame_id=value[1],name=value[2],start=tick//148,end=None)
                    cpu_spans.append(item);cpu_open[value[0]]=item
                else:
                    require(len(value)==1 and value[0] in cpu_open,'CPU end marker')
                    cpu_open.pop(value[0])['end']=tick//148
            else:
                raise ValueError('Unknown system event: '+str(kind))
    require(count == meta['events'], 'event count differs')
    partial = bool(meta['truncated'] or not meta['reached'] or opened or stopped or any(queues))
    require(partial or not cpu_open,'complete capture has an open CPU marker')
    require(allow_partial or not partial, 'partial capture requires --allow-partial')
    axis = origin // 148 * 148
    duration = (meta['end_tick'] - axis + 147)//148
    sys = dict(version=1, origin_tick=origin, axis_origin_tick=axis, end_tick=meta['end_tick'], coverage_end_tick=end,
               quantum_hz=clock*148, logic_period=148, states=states, phases=phases, retires=retires,
               client_payload=payload, native_model32=native, transactions=tx, presents=present,
               capabilities=meta['capabilities'], scanouts=[], submissions=subs,
               scope='Synthetic CB06 preload; real CPU/GPU/Display/shared MC cycle model. Fixed Device5 peer; no SPI loader, DS2 serial, serializer/electrical or board qualification.',
               client_boundary='Client 64-bit payload to shared SDRAM service; not physical pin bandwidth',
               native_boundary='Native model 32-bit valid read return / actual Pins write branch; not board pin bandwidth')
    if common:
        sys['domains']=meta['domains']
        if meta['version']==2:sys['native_source_times']=native_times
        sys['scope']='Observed domains: '+', '.join(k for k,v in meta['domains'].items() if v)+'. Absent domains are unavailable.'
    if meta.get('synthetic') is True:
        sys['scope']=meta.get('scope','Synthetic importer fixture; CPU/Display/application qualification is not asserted')
    if meta['capabilities'].get('command_control_lineage'):
        sys['command_events'] = command_events
    else:
        require(not command_events, 'command events without capability')
    if meta['capabilities'].get('gpu_signal_journal'):
        sys.update(gpu_journal=gpu_journal,gpu_load_columns=meta.get('gpu_load_columns'))
    else: require(not gpu_journal, 'GPU signal journal without capability')
    if meta.get('image_journal',{}).get('available'):
        sys['physical_writes'] = physical_writes
        sys['image_journal'] = meta['image_journal']
    else: require(not physical_writes,'physical image writes without preimage')
    if meta.get('runner_version')==1:
        sys['runner']=codec(meta['runner_version']).import_sidecar(folder,meta)
    for f in meta['scanouts']:
        require(integer(f['start_tick']) and origin <= f['start_tick'] <= f['end_tick'] <= meta['end_tick'], 'scanout interval')
        require(integer(f['effective_pixels']) and f['effective_pixels'] <= 384000, 'scanout pixels')
        raw = (folder/f['file']).read_bytes()
        item = dict(f, rgb_sha256=files[f['file']], png_sha256=hashlib.sha256(png(raw)).hexdigest(), src='data:image/png;base64,'+base64.b64encode(png(raw)).decode())
        sys['scanouts'].append(item)
    record = dict(schema='digitaldesign.system-emu-report-inputs', version=1, capture_directory=str(folder.resolve()), files=files)
    data = dict(schema='digitaldesign.gpu-profile', version=2, title=meta.get('title','Full-system cycle emu' if meta['source_kind']=='system-cycle-emu' else 'GPU profiling capture'), partial=partial,
                clock_hz=clock, origin=axis//148, start=0, end=duration, submit=0, gpu_end=duration,
                submissions=[], commands=[], meshlets=[], jobs=[], dma_transactions=[], cpu=cpu_spans, metric_defs=[], tracks={},
                sample_start=0, sample_end=duration, system=sys, provenance=dict(meta, report_inputs=record),
                checks=dict(capture_complete=not partial, instruction_owner_lineage='unavailable', board_qualified=False))
    from system_commands import normalize
    data['submissions'], data['commands'] = normalize(sys, partial)
    from system_gpu import attach as attach_gpu, normalize as normalize_gpu
    attach_gpu(data, normalize_gpu(data))
    if 'gpu' in sys:data['checks']['instruction_owner_lineage']='observed from committed GPU handshakes'
    data=prepare_report(data)
    from system_frames import attach_identity, write_reports
    data = attach_identity(data, source_identity)
    selection=only_frame if only_frame is not None else meta.get('profile_frame')
    if only_frame is not None and meta.get('profile_frame') is not None:
        require(only_frame==meta['profile_frame'],'frame option differs from capture selection')
    if selection is not None:
        require(not frames,'--only-frame and --frames are mutually exclusive')
        from system_frames import make_gpu
        data=make_gpu(data,selection)
        sys=data['system']
        if 'image_journal' in sys:
            sys['image_journal']=dict(sys['image_journal'],selected_presentations=[selection])
    validate(data)
    if output is None:return data
    output.mkdir(parents=True, exist_ok=True)
    # Publish the already-validated common model and real registered images
    # before historical native reconstruction. A reconstruction error must not
    # hide otherwise usable CB/stage/frame evidence.
    data['checks']['native_history']='pending reconstruction' if meta.get('image_journal',{}).get('available') else 'unavailable'
    write_report(data,output,validated=True)
    write_reports(data,output,frames,validated=True)
    if meta.get('image_journal',{}).get('available'):
        require('gpu' in sys,'native image journal needs actual GPU owner lineage')
        from system_images import build as build_native_images
        data['images']=build_native_images(data,(folder/meta['image_journal']['preimage']).read_bytes(),
                                          physical_writes,output/'images',preview_mode=preview,
                                          known_preimage=(folder/meta['image_journal']['known_preimage']).read_bytes()
                                          if meta['image_journal'].get('known_preimage') else None)
        from system_images import validate as validate_native_images, project
        if selection is not None:data=project(data,data['system']['frame_view']['owned_submission_ids'])
        validate_native_images(data)
        data['checks']['native_history']='available' if data['images']['available'] else 'unavailable'
    frame = data['system'].get('frame_view')
    raster_available = frame['scanout_index'] is not None if frame else bool(sys['scanouts'])
    require(not require_images or raster_available or data.get('images',{}).get('available'),
            'no complete scanout or native image saved')
    write_report(data, output,validated=True)
    (output/'report-inputs.json').write_text(json.dumps(record,indent=2)+'\n')
    write_reports(data, output, frames,validated=True,update=True)
    return data


def service_memory(data):
    """Adapt only saved arbiter-service facts to the shared memory viewer.

    Legacy captures lack pre-slot GPU acceptance and DMA/texture attribution.
    Neither the boundary nor the unknown source is silently promoted to master.
    """
    from collections import defaultdict
    from memory import build
    s=data['system'];axis=s['axis_origin_tick'];events=defaultdict(list)
    cycle=lambda tick:(tick-axis)//148
    transactions=[t for t in s['transactions'] if t['port']>=4]
    if any(t['requested_beats'] not in (4,8,16) for t in transactions):
        return dict(available=False,observation_boundary='arbiter-service64',
                    reason='Legacy GPU service descriptor sizes are missing or unsupported; request accounting and master timing are unavailable')
    for t in transactions:
        port=t['port']-4
        events['MEM_REQUEST'].append([cycle(t['start_tick']),port,t['address_halfword'],t['requested_beats']//4-1,0])
        if t['end_tick'] is not None:events['MEM_COMPLETE'].append([cycle(t['end_tick']),port,int(t['error'])])
    for tick,port,read,write in s['client_payload']:
        if port>=4:events['MEM_DATA'].append([cycle(tick),port-4,255,0])
    tracks={name:[] for name in ('ro_wait','fb_read_wait','fb_write_wait')}
    for tick,state in s['states']:
        for port,name in enumerate(tracks,4):
            value=int(bool(state[9]&(1<<port)));points=tracks[name]
            if not points or points[-1][1]!=value:points.append([cycle(tick),value])
    stop=max(1,(s['coverage_end_tick']-axis+147)//148)
    interval=(cycle(s['states'][0][0]),stop) if s['states'] else None
    report=build(events,0,stop,54000000,not data['partial'],[],tracks,interval)
    report.update(observation_boundary='arbiter-service64',
        boundary='GPU 64-bit arbiter service after board descriptor slots; master acceptance and DMA/texture owners unavailable; not physical SDRAM pins')
    if not transactions:report['reason']='No GPU arbiter-service descriptor observed in this capture'
    return report


def prepare_report(data):
    """Add a derived legacy view on build/render; saved source facts stay intact."""
    if not data['system']['capabilities'].get('gpu_signal_journal') and 'memory' not in data:
        return dict(data,memory=service_memory(data))
    return data


def validate_transactions(transactions, payload, origin, coverage_end, partial, *, ceiling=10000000):
    """Shared raw-build/render semantics; terminal end is the edge plus148 ticks.

    Replay only the saved metadata, never simulation. At a coincident edge,
    accept precedes payload then terminal, preserving legal lookahead and LAST.
    """
    require(type(transactions) is list and len(transactions)<=ceiling, 'transaction bounds')
    terminals=[]
    previous=origin
    for i,t in enumerate(transactions,1):
        require(integer(t.get('id'),1) and t['id']==i and integer(t.get('port')) and t['port'] in (0,1,2,4,5,6), 'transaction identity')
        start,end=t.get('start_tick'),t.get('end_tick')
        require(integer(start) and previous<=start<=coverage_end and start%148==0 and type(t.get('writing')) is bool, 'transaction start/direction')
        previous=start
        require((t['port'] in (2,6) if t['writing'] else t['port']!=6), 'transaction port direction')
        require(integer(t.get('address_halfword')) and t['address_halfword']<(1<<22), 'transaction address')
        require(all(integer(t.get(k)) and t[k]%8==0 for k in ('read_bytes','write_bytes')), 'transaction bytes')
        require(t['read_bytes']==0 if t['writing'] else t['write_bytes']==0, 'transaction payload direction')
        beats=t.get('requested_beats')
        require(beats is None or integer(beats,1) and beats<=16, 'request size')
        size=t['read_bytes']+t['write_bytes']
        require(beats is None or size<=beats*8, 'payload exceeds request size')
        last=t.get('last_data_tick')
        require(last is None if size==0 else integer(last) and start<=last<=coverage_end and last%148==0, 'transaction last_data_tick')
        if end is None:
            require(partial and t.get('error') is None, 'open transaction requires partial coverage and no terminal error')
        else:
            require(integer(end) and start+148<=end<=coverage_end+148 and end%148==0 and type(t.get('error')) is bool, 'transaction terminal')
            require(last is None or last+148<=end, 'transaction terminal before payload')
            require(t['error'] or beats is None or size==beats*8, 'terminal before full payload')
            terminals.append((end-148,2,i,t))
    # Client rows were shape/time validated by validate(). Acceptance order is
    # monotonic; merge keeps metadata replay linear except terminal sorting.
    accepts=((t['start_tick'],0,t['id'],t) for t in transactions)
    data_events=((row[0],1,i,row) for i,row in enumerate(payload))
    actual=[[0,0,None] for _ in transactions]
    queues=[deque() for _ in PORTS]
    for tick,kind,_,item in merge(accepts,data_events,sorted(terminals)):
        if kind==0:
            queues[item['port']].append(item)
        elif kind==2:
            q=queues[item['port']]
            require(bool(q) and q[0]['id']==item['id'], 'transaction terminal owner/order')
            q.popleft()
        else:
            _,port,read,write=item
            q=queues[port]
            require(bool(q), 'unowned transaction payload')
            owner=q[0]
            if write:
                owner=next((t for t in q if t['writing'] and (t.get('requested_beats') is None or actual[t['id']-1][1]<t['requested_beats']*8)),None)
                require(owner is not None, 'write payload direction/owner')
            require(bool(write)==owner['writing'], 'transaction payload direction')
            counts=actual[owner['id']-1]
            counts[0]+=read;counts[1]+=write;counts[2]=tick
    for t,counts in zip(transactions,actual):
        require(counts==[t['read_bytes'],t['write_bytes'],t.get('last_data_tick')], 'transaction payload/last_data_tick differs from owned events')


def validate(data):
    require(data.get('schema') == 'digitaldesign.gpu-profile' and data.get('version') == 2, 'profile schema')
    require(data.get('provenance',{}).get('source_kind') in ('system-cycle-emu','gpu-rtl','gpu-cycle-emu'), 'profile source')
    s=data.get('system',{})
    ceiling=112000000 if s.get('runner') else 10000000
    if 'runner' in s:
        codec(data['provenance']['runner_version']).validate_report(data)
    require(s.get('version') == 1 and s.get('quantum_hz') == data['clock_hz']*148 and s.get('logic_period') == 148, 'system timebase')
    require(integer(data.get('end'),1) and type(data.get('partial')) is bool, 'profile range/status')
    require(all(integer(s.get(k)) for k in ('origin_tick','axis_origin_tick','coverage_end_tick','end_tick')), 'system bounds')
    require(s['axis_origin_tick'] <= s['origin_tick'] <= s['coverage_end_tick'] <= s['end_tick'], 'coverage')
    require(s['axis_origin_tick'] % 148 == 0 and data.get('start') == 0 and data['end'] == (s['end_tick']-s['axis_origin_tick']+147)//148, 'profile axis')
    caps=s.get('capabilities',{})
    require(type(caps.get('client_payload64')) is bool and type(caps.get('native_model32')) is bool and caps.get('pin_electrical') is False
            and type(caps.get('instruction_owner_lineage')) is bool
            and (not caps['instruction_owner_lineage'] or caps.get('gpu_signal_journal') is True
                 and caps.get('command_control_lineage') is True), 'capability declaration')
    if data['provenance'].get('schema')=='digitaldesign.profiling-capture' and data['provenance'].get('version')==2:
        from event_protocol import time_value
        facts=s.get('native_source_times',[])
        require(type(facts) is list,'exact native time facts')
        for kind,actual in [('native',[r[0] for r in s['native_model32']]),
                            ('physical_write',[r[0] for r in s.get('physical_writes',[])])]:
            expected=[time_value(raw,k,data['provenance']) for raw,k in facts if k==kind]
            require(actual==expected,'native time projection differs from exact source times')
    if 'domains' in s:
        require(caps['client_payload64'] or not s['transactions'] and not s['client_payload'],'service rows without observation domain')
        require(caps['native_model32'] or not s['native_model32'],'native slots without observation domain')
        require(s['domains'].get('cpu') or not s['retires'],'CPU rows without observation domain')
        require(s['domains'].get('display') or not s['presents'] and not s['scanouts'],'Display rows without observation domain')
        require(s['domains'].get('physical_write') or not s.get('physical_writes'),'physical writes without observation domain')
    for key in ('states','phases','retires','client_payload','native_model32','presents'):
        rows=s.get(key)
        require(type(rows) is list and len(rows) <= ceiling, key+' bounds')
        previous=s['origin_tick']
        for row in rows:
            rational=data['provenance'].get('schema')=='digitaldesign.profiling-capture' and data['provenance'].get('version')==2
            require(type(row) is list and len(row)>=2 and (native_time(row[0]) if key=='native_model32' and rational else integer(row[0])) and previous <= row[0] <= s['coverage_end_tick'], key+' timestamps')
            previous=row[0]
            if key == 'states':
                require(len(row)==2 and type(row[1]) is list and len(row[1])==10 and all(integer(v) for v in row[1]) and all(v<=1 for v in row[1][:8]) and all(v<128 for v in row[1][8:]), 'state values')
            if key == 'client_payload':
                require(len(row)==4 and integer(row[1]) and row[1] in (0,1,2,4,5,6) and all(integer(v) and v in (0,8) for v in row[2:]) and sum(row[2:])==8 and row[0]%148==0, 'client64 values')
            if key == 'native_model32':
                require(len(row)==4 and all(integer(v) for v in row[1:]) and row[1] in (0,4) and row[2] in (0,4) and row[1]+row[2]==4 and row[3]<=row[2] and (data['provenance'].get('schema')=='digitaldesign.profiling-capture' and data['provenance'].get('version')==2 or row[0]%74==(0 if row[1] else 37)), 'native32 values/phase')
            if key == 'phases':require(len(row)==2 and integer(row[1]) and row[1]<256, 'CP phase')
            if key == 'retires':require(len(row)==3 and all(integer(v) for v in row[1:]), 'retirement values')
    validate_transactions(s.get('transactions',[]),s['client_payload'],s['origin_tick'],s['coverage_end_tick'],data['partial'],ceiling=ceiling)
    for f in s.get('scanouts',[]):
        require(type(f.get('file')) is str and Path(f['file']).name == f['file'], 'scanout path')
        require(all(integer(f.get(k)) for k in ('start_tick','end_tick','effective_pixels')) and s['origin_tick']<=f['start_tick']<=f['end_tick']<=s['end_tick'] and f['effective_pixels']<=384000, 'scanout interval')
        raw=base64.b64decode(f['src'].removeprefix('data:image/png;base64,'),validate=True)
        require(f['src'].startswith('data:image/png;base64,') and raw.startswith(b'\x89PNG\r\n\x1a\n') and len(raw)<10000000 and struct.unpack('>II',raw[16:24])==(800,480) and hashlib.sha256(raw).hexdigest()==f['png_sha256'], 'scanout PNG binding')
    from system_frames import validate as validate_frame
    from system_commands import validate as validate_commands
    validate_commands(data)
    if not caps.get('gpu_signal_journal') and 'memory' in data:
        require(data['memory']==service_memory(data),'GPU service memory differs from saved arbiter facts')
    from system_images import validate as validate_images
    validate_images(data)
    validate_frame(data)
    return data


def write_outputs(data, folder):
    """Extra outputs share the existing write_report/render path."""
    s=data['system']; folder=Path(folder)
    if 'runner' in s:
        timing=codec(data['provenance']['runner_version']).timing_report(data)
        if 'frame_view' in s:
            n=s['frame_view']['presentation']
            timing=dict(timing,intervals=[r for r in timing['intervals'] if r['presentation']==n],
                        selections=[r for r in timing['selections'] if r['presentation']==n],
                        frame_states=[r for r in timing['frame_states'] if r['presentation']==n])
        (folder/'runner-frame-timings.json').write_text(json.dumps(timing,indent=2)+'\n')
    from system_images import write_pngs
    write_pngs(data,folder)
    if 'frame_view' in s:
        from system_frames import write_outputs as write_frame_outputs
        return write_frame_outputs(data, folder)
    with (folder/'system-memory.csv').open('w',newline='') as f:
        w=csv.writer(f); w.writerow(['boundary','tick','port','read_bytes','write_slot_bytes','write_unmasked_bytes'])
        for t,p,r,b in s['client_payload']: w.writerow(['client64',t,PORTS[p],r,b,'unavailable'])
        for t,r,b,u in s['native_model32']: w.writerow(['native-model32',t,'all',r,b,u])
    with (folder/'system-state.csv').open('w',newline='') as f:
        w=csv.writer(f); w.writerow(['tick','startup_hold','system_hold','cache_sweep','halt','fault','gpu_busy','producer_busy','backend_busy','client_valid_mask','client_backpressure_mask'])
        for t,v in s['states']: w.writerow([t,*v])
    with (folder/'system-transactions.csv').open('w',newline='') as f:
        fields=['id','port','start_tick','end_tick','address_halfword','writing','requested_beats','read_bytes','write_bytes','last_data_tick','error']
        writer=csv.DictWriter(f,fieldnames=fields);writer.writeheader();writer.writerows(s['transactions'])
    for i,item in enumerate(s['scanouts']):
        (folder/f'scanout-{i:06}.png').write_bytes(base64.b64decode(item['src'].split(',',1)[1],validate=True))
