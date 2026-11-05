"""Presentation-specific views of an intact, validated system capture.

Window measurements and submission ownership are deliberately separate. Original
events/transaction endpoints remain available for conservation and validation.
"""
import base64
import csv
import hashlib
import json
from pathlib import Path
import re


def require(value, message):
    if not value:
        raise ValueError('System frame: ' + message)


def attach_identity(data, filename):
    if filename is None:
        return data
    raw = Path(filename).read_bytes()
    identity = json.loads(raw)
    files = data['provenance']['report_inputs']['files']
    source = identity.get('application_source', identity.get('source'))
    tree = identity.get('application_tree', identity.get('tree'))
    program = identity.get('package_sha256', identity.get('boot_sha256'))
    executable = identity.get('executable_sha256', identity.get('runtime_sha256'))
    require(type(source) is str and re.fullmatch('[0-9a-f]{40}', source), 'source identity')
    require(type(tree) is str and re.fullmatch('[0-9a-f]{40}', tree), 'tree identity')
    require(program == files['program.bin'] and (executable is None or executable == files['source-executable.bin']),
            'application identity does not match captured program/executable')
    result = dict(data, provenance=dict(data['provenance']))
    result['provenance']['application_identity'] = dict(
        source=source, tree=tree, program_sha256=program, executable_sha256=files['source-executable.bin'],
        identity_sha256=hashlib.sha256(raw).hexdigest(),
        scope='Supplied source mapping; program and optional executable identities checked; captured machine inputs bound')
    return result


def describe(data, number, session_report='../../profiler.html'):
    s = data['system']
    require(type(session_report) is str and not session_report.startswith('/')
            and ':' not in session_report and '\\' not in session_report
            and session_report.endswith('profiler.html'), 'relative full-session report link')
    require(type(number) is int and number > 0, 'positive presentation ordinal required')
    requests = [r for r in s['presents'] if r[1] == 'present_request']
    applied = [r for r in s['presents'] if r[1] == 'present_applied']
    require([r[2] for r in requests] == list(range(1, len(requests)+1)), 'request sequence')
    require([r[2][0] for r in applied] == list(range(1, len(applied)+1)), 'applied sequence')
    require(number <= min(len(requests), len(applied)), 'selected presentation is not applied')
    request, application = requests[number-1], applied[number-1]
    previous_request = requests[number-2][0] if number > 1 else s['origin_tick']-1
    require(request[0] <= application[0], 'application before request')
    if number > 1:
        require(applied[number-2][0] < request[0], 'overlapping presentation requests unsupported')
    owned = [r for r in s['submissions'] if previous_request < r['start_tick'] <= request[0]]
    require(owned and all(r['end_tick'] is not None and r['end_tick'] <= request[0] for r in owned),
            'submission group must drain before presentation request')
    candidates = [(i, f) for i, f in enumerate(s['scanouts'])
                  if f['effective_pixels'] == 384000 and f['start_tick'] >= application[0]
                  and f['applied_present_count'] == number
                  and f['front_at_start_halfword'] == application[2][1]
                  and not f['present_changed_during_scanout']
                  and f['end_tick'] <= s['coverage_end_tick']
                  and not any(f['start_tick'] <= a[0] <= f['end_tick'] for a in applied[number:])]
    require(candidates, 'no complete stable matching registered source raster')
    index, raster = min(candidates, key=lambda item: item[1]['start_tick'])
    lo = s['origin_tick'] if number == 1 else applied[number-2][0] + 148
    hi = application[0] + 148
    require(lo < hi <= s['coverage_end_tick'], 'frame window outside captured coverage')
    return dict(version=1, presentation=number, start_tick=lo, end_tick=hi,
                request_tick=request[0], applied_tick=application[0], front_halfword=application[2][1],
                owned_submission_ids=[r['id'] for r in owned], scanout_index=index,
                scanout_start_tick=raster['start_tick'], scanout_end_tick=raster['end_tick'],
                session_report=session_report,
                window_scope='Observed applied-to-applied half-open window; F1 includes initialization. '
                             'Time-window CPU/memory activity is concurrent activity, not CPU frame ownership.',
                ownership_scope='GPU submission group between the prior and selected presentation requests; '
                                'full job lifetimes are separate from window intersections.',
                cpu_frame_owner='unavailable', hud_wait_return_ticks='unavailable')


def describe_gpu(data, number):
    """Complete presentation GPU group; no applied present or raster required."""
    s=data['system']
    require(type(number) is int and number>0, 'positive presentation ordinal required')
    requests=[r for r in s['presents'] if r[1]=='present_request']
    require([r[2] for r in requests]==list(range(1,len(requests)+1)), 'request sequence')
    require(number<=len(requests), 'selected presentation has not been requested')
    request=requests[number-1]
    previous=requests[number-2][0] if number>1 else s['origin_tick']-1
    owned=[r for r in s['submissions'] if previous<r['start_tick']<=request[0]]
    require(owned and all(r['end_tick'] is not None and r['end_tick']<=request[0] for r in owned),
            'submission group must drain before presentation request')
    lo=min(r['start_tick'] for r in owned);hi=max(r['end_tick'] for r in owned)
    require(lo<hi<=s['coverage_end_tick'], 'GPU window outside captured coverage')
    return dict(version=1,boundary='gpu-completion',presentation=number,
                start_tick=lo,end_tick=hi,request_tick=request[0],
                owned_submission_ids=[r['id'] for r in owned],scanout_index=None,session_report=None,
                window_scope='First GPU START through last GPU completion of the selected presentation; warmup excluded. '
                             'Concurrent CPU/memory activity is not CPU frame ownership.',
                ownership_scope='All CBs between the prior and selected presentation requests; original IDs retained.',
                cpu_frame_owner='unavailable',hud_wait_return_ticks='unavailable')


def make_gpu(data, number):
    frame=describe_gpu(data,number)
    frame['summary']=summary(data,frame)
    result=dict(data,title=f'Application frame {number} GPU',system=dict(data['system'],frame_view=frame))
    from system_images import project
    return project(result,frame['owned_submission_ids'])


def summary(data, frame):
    s = data['system']; lo, hi = frame['start_tick'], frame['end_tick']
    clients = [[0, 0] for _ in range(7)]; native = [0, 0, 0]
    for t, p, read, write in s['client_payload']:
        if lo <= t < hi:
            clients[p][0] += read; clients[p][1] += write
    for t, read, write, unmasked in s['native_model32']:
        if lo <= t < hi:
            native[0] += read; native[1] += write; native[2] += unmasked
    before = [r for r in s['retires'] if r[0] < lo]
    inside = [r for r in s['retires'] if lo <= r[0] < hi]
    baseline = before[-1] if before else (inside[0] if inside else None)
    last = inside[-1] if inside else baseline
    cpu = None if baseline is None else dict(
        retired_words=last[1]-baseline[1], baseline_tick=baseline[0],
        baseline_at_or_before_window=bool(before), frame_owner='unavailable')
    tx = s['transactions']
    end = lambda t: t['end_tick'] if t['end_tick'] is not None else s['coverage_end_tick']
    return dict(client_payload_bytes=clients, native_model32_bytes=native, cpu_counter=cpu,
                accepted_in_window=sum(lo <= t['start_tick'] < hi for t in tx),
                terminal_events_in_window=sum(t['end_tick'] is not None and lo <= t['end_tick']-148 < hi for t in tx),
                carry_in=sum(t['start_tick'] < lo < end(t) for t in tx),
                carry_out=sum(t['start_tick'] < hi < end(t) or
                              t['end_tick'] is None and t['start_tick'] < hi <= end(t) for t in tx))


def make(data, number):
    frame = describe(data, number)
    frame['summary'] = summary(data, frame)
    result=dict(data, title=f'Application frame {number}',system=dict(data['system'], frame_view=frame))
    from system_images import project
    return project(result,frame['owned_submission_ids'])


def validate(data):
    identity=data['provenance'].get('application_identity')
    if identity is not None:
        files=data['provenance']['report_inputs']['files']
        require(all(type(identity.get(k)) is str and re.fullmatch('[0-9a-f]{40}',identity[k]) for k in ('source','tree'))
                and identity.get('program_sha256')==files['program.bin']
                and identity.get('executable_sha256')==files['source-executable.bin']
                and type(identity.get('identity_sha256')) is str
                and re.fullmatch('[0-9a-f]{64}',identity['identity_sha256']), 'application identity binding differs')
    frame = data['system'].get('frame_view')
    if frame is None:
        return
    expected = (describe_gpu(data, frame.get('presentation')) if frame.get('boundary')=='gpu-completion'
                else describe(data, frame.get('presentation'), frame.get('session_report')))
    expected['summary'] = summary(data, expected)
    require(frame == expected, 'frame identity/window/summary differs from saved events')


def write_reports(data, output, numbers, *, validated=False, update=False):
    if not numbers:
        return
    from model import write_report
    require('system' in data and 'frame_view' not in data['system'], 'frame export needs a full system session')
    require(len(numbers) == len(set(numbers)), 'duplicate frame ordinal')
    # Resolve all requested frames before publishing any frame output.
    frames = [make(data, n) for n in numbers]
    records = []
    for d in frames:
        n = d['system']['frame_view']['presentation']
        folder = Path(output)/'frames'/f'frame-{n}'
        require(update or not folder.exists(), 'frame output already exists')
        from system_images import copy_assets
        copy_assets(d,output,folder)
        if validated:
            from system_images import validate as validate_images
            validate_images(d)
        write_report(d, folder,validated=validated)
        records.append(dict(presentation=n, html=f'frame-{n}/profiler.html',
                            frame=d['system']['frame_view']))
    (Path(output)/'frames/index.json').write_text(json.dumps(records, indent=2)+'\n')


def write_outputs(data, folder):
    """Frame CSVs retain full transaction endpoints and label clipped visibility."""
    s = data['system']; f = s['frame_view']; lo, hi = f['start_tick'], f['end_tick']
    folder = Path(folder)
    (folder/'frame-summary.json').write_text(json.dumps(f, indent=2)+'\n')
    with (folder/'system-memory.csv').open('w', newline='') as stream:
        writer = csv.writer(stream)
        writer.writerow(['boundary','tick','port','read_bytes','write_slot_bytes','write_unmasked_bytes'])
        for t, p, read, write in s['client_payload']:
            if lo <= t < hi: writer.writerow(['client64', t, p, read, write, 'unavailable'])
        for t, read, write, mask in s['native_model32']:
            if lo <= t < hi: writer.writerow(['native-model32', t, 'all', read, write, mask])
    with (folder/'system-state.csv').open('w', newline='') as stream:
        writer = csv.writer(stream)
        writer.writerow(['observed_tick','visible_start_tick','visible_end_tick','state_values'])
        for i, (t, values) in enumerate(s['states']):
            end = s['states'][i+1][0] if i+1 < len(s['states']) else s['coverage_end_tick']
            if max(t, lo) < min(end, hi):
                writer.writerow([t, max(t,lo), min(end,hi), json.dumps(values)])
    with (folder/'system-transactions.csv').open('w', newline='') as stream:
        fields = ['id','port','start_tick','end_tick','visible_start_tick','visible_end_tick',
                  'carry_in','carry_out','read_bytes','write_bytes','bytes_scope']
        writer = csv.DictWriter(stream, fields); writer.writeheader()
        for t in s['transactions']:
            end = t['end_tick'] if t['end_tick'] is not None else s['coverage_end_tick']
            a,b = max(t['start_tick'],lo),min(end,hi)
            if a < b:
                writer.writerow(dict(id=t['id'],port=t['port'],start_tick=t['start_tick'],end_tick=t['end_tick'],
                                     visible_start_tick=a,visible_end_tick=b,carry_in=t['start_tick']<lo,
                                     carry_out=end>hi or t['end_tick'] is None and end>=hi,
                                     read_bytes=t['read_bytes'],write_bytes=t['write_bytes'],
                                     bytes_scope='full transaction lifetime; window bytes in frame-summary.json'))
    with (folder/'system-submissions.csv').open('w', newline='') as stream:
        writer=csv.writer(stream);writer.writerow(['id','start_tick','end_tick','ownership'])
        for t in s['submissions']:
            if t['id'] in f['owned_submission_ids']:
                writer.writerow([t['id'],t['start_tick'],t['end_tick'],'selected presentation GPU group'])
    if f['scanout_index'] is not None:
        raster = s['scanouts'][f['scanout_index']]
        (folder/'scanout-selected.png').write_bytes(base64.b64decode(raster['src'].split(',',1)[1],validate=True))
