"""The persisted event contract shared by new RTL and cycle-emu collectors.

Legacy transports remain importable; they do not acquire unobserved points.
The GPU normalizers own generations and completion endpoints, not the viewer.
"""
import json
from pathlib import Path
CATALOG=json.loads((Path(__file__).resolve().parents[4]/'ip/gpu-v2/src/system/vliw_cycle/profiling_contract.json').read_text())
SCHEMA=CATALOG['schema']
VERSION=CATALOG['version']
EVENTS=CATALOG['events_file']
GPU_POINTS=CATALOG['supported_points']
GPU_EVENTS=CATALOG['gpu_event_points']


def contract(clock_hz, domains):
    return dict(CATALOG,clock_hz=clock_hz,quantum_hz=clock_hz*148,domains=domains)


def validate(meta):
    if meta.get('schema') != SCHEMA or meta.get('version') not in (1,VERSION):
        raise ValueError('Unsupported common profiling event protocol')
    expected=contract(meta['clock_hz'],meta['domains'])
    expected['version']=meta['version']
    if meta['version']==1:
        points=meta.get('gpu_event_points',[])
        if type(points) is not list or len(points)!=len(set(points)) or not set(points)<=set(GPU_EVENTS):
            raise ValueError('Unknown legacy common observation points')
        expected['gpu_event_points']=points
    if any(meta.get(k)!=v for k,v in expected.items()):
        raise ValueError('Common profiling clock/point/phase contract differs')
    if meta['domains'].get('gpu') is not True:
        raise ValueError('Common capture requires the observed GPU domain')
    if any(type(v) is not bool for v in meta['domains'].values()):
        raise ValueError('Observation domain availability must be boolean')
    caps=meta['capabilities']
    if not all(caps.get(k) is True for k in ('command_control_lineage','gpu_signal_journal','gpu_master_requests','gpu_master_payloads')):
        raise ValueError('Common capture is missing a required GPU observation point')
    return meta


def time_value(raw, kind, meta):
    """Native edges use an exact rational in the same quantum as logic edges.

    Pair numerators are source integers, not JavaScript visualization values.
    Logic events must remain exact multiples of148; no event is shifted.
    """
    if type(raw) is list:
        if meta.get('version')!=2 or kind not in ('physical_write','native') or len(raw)!=2:
            raise ValueError('Unexpected rational event timestamp')
        n,d=raw
        if type(n) is not int or type(d) is not int or not 0<=n<1<<64 or not 0<d<1<<64:
            raise ValueError('Invalid exact native timestamp')
        return n/d
    if type(raw) is not int or not 0<=raw<1<<53:
        raise ValueError('Invalid event timestamp')
    return raw


def filename(meta):
    return EVENTS if meta.get('schema')==SCHEMA else 'system-events.jsonl'
