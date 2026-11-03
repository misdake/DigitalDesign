"""Import bounded full-system cycle observations into the existing report/viewer."""
import base64
from collections import deque
import csv
import hashlib
from heapq import merge
import json
from pathlib import Path
import struct
import zlib

SOURCE = 'digitaldesign.system-emu-capture'
PORTS = ['display', 'instruction', 'data', 'dma', 'gpu_ro', 'gpu_fb_r', 'gpu_fb_w']
LIMIT = (1 << 53) - 1


def require(condition, message):
    if not condition:
        raise ValueError('System emu: ' + message)


def integer(value, minimum=0):
    return type(value) is int and minimum <= value <= LIMIT


def png(rgb):
    require(len(rgb) == 800 * 480 * 3, 'incomplete registered raster')
    def chunk(kind, body):
        data = kind + body
        return struct.pack('>I', len(body)) + data + struct.pack('>I', zlib.crc32(data) & 0xffffffff)
    rows = b''.join(b'\0' + rgb[y*2400:(y+1)*2400] for y in range(480))
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', 800, 480, 8, 2, 0, 0, 0))
            + chunk(b'sRGB', b'\0') + chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b''))


def evidence(folder, meta):
    names = ['capture.json', 'program.bin', 'source-executable.bin', 'system-events.jsonl']
    names += [f['file'] for f in meta['scanouts']]
    result = {}
    for name in names:
        require(Path(name).name == name and name not in result, 'unsafe/duplicate evidence filename')
        result[name] = hashlib.sha256((folder/name).read_bytes()).hexdigest()
    return result


def build(folder, output, *, allow_partial=False, binding=None, require_images=False):
    from model import write_report
    folder, output = Path(folder), Path(output)
    meta = json.loads((folder/'capture.json').read_text())
    require(meta.get('schema') == SOURCE and meta.get('version') == 1, 'unsupported capture')
    require(meta.get('source_kind') == 'system-cycle-emu' and meta.get('ports') == PORTS, 'source/ports')
    require(meta.get('quantum_hz') == 7992000000 and meta.get('logic_period') == 148, 'timebase')
    require(all(integer(meta.get(k)) for k in ('origin_tick','end_tick','coverage_end_tick','events')), 'capture bounds')
    origin, end = meta['origin_tick'], meta['coverage_end_tick']
    require(origin <= end <= meta['end_tick'] and meta['end_tick'] > origin, 'empty/reversed capture')
    files = evidence(folder, meta)
    if binding:
        expected = json.loads(Path(binding).read_text())
        require(expected.get('schema') == 'digitaldesign.system-emu-report-inputs' and expected.get('files') == files, 'binding mismatch')
    states, payload, native, phases, present, retires, tx = [], [], [], [], [], [], []
    queues = [deque() for _ in PORTS]
    subs, opened = [], None
    previous, count = origin, 0
    with (folder/'system-events.jsonl').open(encoding='utf-8') as stream:
        for line in stream:
            row = json.loads(line)
            require(type(row) is list and len(row) == 3, 'event envelope')
            tick, kind, value = row
            require(integer(tick) and previous <= tick <= end, 'unordered/out-of-range tick')
            previous, count = tick, count + 1
            require(count <= 10000000, 'event limit')
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
            elif kind in ('present_request','present_applied'):
                require((kind == 'present_request' and integer(value,1)) or
                        (kind == 'present_applied' and type(value) is list and len(value) == 2 and all(integer(v) for v in value)), 'present')
                present.append([tick,kind,value])
            else:
                raise ValueError('Unknown system event: '+str(kind))
    require(count == meta['events'], 'event count differs')
    partial = bool(meta['truncated'] or not meta['reached'] or opened or any(queues))
    require(allow_partial or not partial, 'partial capture requires --allow-partial')
    axis = origin // 148 * 148
    duration = (meta['end_tick'] - axis + 147)//148
    sys = dict(version=1, origin_tick=origin, axis_origin_tick=axis, end_tick=meta['end_tick'], coverage_end_tick=end,
               quantum_hz=7992000000, logic_period=148, states=states, phases=phases, retires=retires,
               client_payload=payload, native_model32=native, transactions=tx, presents=present,
               capabilities=meta['capabilities'], scanouts=[], submissions=subs,
               scope='Synthetic CB06 preload; real CPU/GPU/Display/shared MC cycle model. Fixed Device5 peer; no SPI loader, DS2 serial, serializer/electrical or board qualification.',
               client_boundary='Client 64-bit payload to shared SDRAM service; not physical pin bandwidth',
               native_boundary='Native model 32-bit valid read return / actual Pins write branch; not board pin bandwidth')
    for f in meta['scanouts']:
        require(integer(f['start_tick']) and origin <= f['start_tick'] <= f['end_tick'] <= meta['end_tick'], 'scanout interval')
        require(integer(f['effective_pixels']) and f['effective_pixels'] <= 384000, 'scanout pixels')
        raw = (folder/f['file']).read_bytes()
        item = dict(f, rgb_sha256=files[f['file']], png_sha256=hashlib.sha256(png(raw)).hexdigest(), src='data:image/png;base64,'+base64.b64encode(png(raw)).decode())
        sys['scanouts'].append(item)
    require(not require_images or bool(sys['scanouts']), 'no complete scanout saved')
    record = dict(schema='digitaldesign.system-emu-report-inputs', version=1, capture_directory=str(folder.resolve()), files=files)
    data = dict(schema='digitaldesign.gpu-profile', version=2, title='Full-system cycle emu', partial=partial,
                clock_hz=54000000, origin=axis//148, start=0, end=duration, submit=0, gpu_end=duration,
                submissions=[], commands=[], meshlets=[], jobs=[], dma_transactions=[], cpu=[], metric_defs=[], tracks={},
                sample_start=0, sample_end=duration, system=sys, provenance=dict(meta, report_inputs=record),
                checks=dict(capture_complete=not partial, instruction_owner_lineage='unavailable', board_qualified=False))
    validate(data)
    output.mkdir(parents=True, exist_ok=True)
    write_report(data, output)
    (output/'report-inputs.json').write_text(json.dumps(record,indent=2)+'\n')
    return data


def validate_transactions(transactions, payload, origin, coverage_end, partial):
    """Shared raw-build/render semantics; terminal end is the edge plus148 ticks.

    Replay only the saved metadata, never simulation. At a coincident edge,
    accept precedes payload then terminal, preserving legal lookahead and LAST.
    """
    require(type(transactions) is list and len(transactions)<=10000000, 'transaction bounds')
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
    require(data.get('provenance',{}).get('source_kind') == 'system-cycle-emu', 'profile source')
    s=data.get('system',{})
    require(s.get('version') == 1 and s.get('quantum_hz') == 7992000000 and s.get('logic_period') == 148, 'system timebase')
    require(integer(data.get('end'),1) and type(data.get('partial')) is bool, 'profile range/status')
    require(all(integer(s.get(k)) for k in ('origin_tick','axis_origin_tick','coverage_end_tick','end_tick')), 'system bounds')
    require(s['axis_origin_tick'] <= s['origin_tick'] <= s['coverage_end_tick'] <= s['end_tick'], 'coverage')
    require(s['axis_origin_tick'] % 148 == 0 and data.get('clock_hz') == 54000000 and data.get('start') == 0 and data['end'] == (s['end_tick']-s['axis_origin_tick']+147)//148, 'profile axis')
    caps=s.get('capabilities',{})
    require(caps.get('client_payload64') is True and caps.get('native_model32') is True and caps.get('pin_electrical') is False and caps.get('instruction_owner_lineage') is False, 'capability declaration')
    for key in ('states','phases','retires','client_payload','native_model32','presents'):
        rows=s.get(key)
        require(type(rows) is list and len(rows) <= 10000000, key+' bounds')
        previous=s['origin_tick']
        for row in rows:
            require(type(row) is list and len(row)>=2 and integer(row[0]) and previous <= row[0] <= s['coverage_end_tick'], key+' timestamps')
            previous=row[0]
            if key == 'states':
                require(len(row)==2 and type(row[1]) is list and len(row[1])==10 and all(integer(v) for v in row[1]) and all(v<=1 for v in row[1][:8]) and all(v<128 for v in row[1][8:]), 'state values')
            if key == 'client_payload':
                require(len(row)==4 and integer(row[1]) and row[1] in (0,1,2,4,5,6) and all(integer(v) and v in (0,8) for v in row[2:]) and sum(row[2:])==8 and row[0]%148==0, 'client64 values')
            if key == 'native_model32':
                require(len(row)==4 and all(integer(v) for v in row[1:]) and row[1] in (0,4) and row[2] in (0,4) and row[1]+row[2]==4 and row[3]<=row[2] and row[0]%74==(0 if row[1] else 37), 'native32 values/phase')
            if key == 'phases':require(len(row)==2 and integer(row[1]) and row[1]<256, 'CP phase')
            if key == 'retires':require(len(row)==3 and all(integer(v) for v in row[1:]), 'retirement values')
    validate_transactions(s.get('transactions',[]),s['client_payload'],s['origin_tick'],s['coverage_end_tick'],data['partial'])
    for f in s.get('scanouts',[]):
        require(type(f.get('file')) is str and Path(f['file']).name == f['file'], 'scanout path')
        require(all(integer(f.get(k)) for k in ('start_tick','end_tick','effective_pixels')) and s['origin_tick']<=f['start_tick']<=f['end_tick']<=s['end_tick'] and f['effective_pixels']<=384000, 'scanout interval')
        raw=base64.b64decode(f['src'].removeprefix('data:image/png;base64,'),validate=True)
        require(f['src'].startswith('data:image/png;base64,') and raw.startswith(b'\x89PNG\r\n\x1a\n') and len(raw)<10000000 and struct.unpack('>II',raw[16:24])==(800,480) and hashlib.sha256(raw).hexdigest()==f['png_sha256'], 'scanout PNG binding')
    return data


def write_outputs(data, folder):
    """Extra outputs share the existing write_report/render path."""
    s=data['system']; folder=Path(folder)
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
