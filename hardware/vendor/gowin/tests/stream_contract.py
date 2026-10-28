"""Validate/stage the sole freshly exported stream MC peer; no historical fallback."""
from pathlib import Path
import json
import shutil


def fnv1a64(data):
    value = 0xcbf29ce484222325
    for byte in data:
        value = ((value ^ byte) * 0x100000001b3) & 0xffffffffffffffff
    return f'{value:016x}'


def load_contract(peer):
    peer = Path(peer)
    path = peer/'stream-contract.json'
    if not path.is_file():
        raise ValueError(f'Missing current stream contract: {path}; export the peer again')
    contract = json.loads(path.read_text())
    if contract.get('schema') != 1 or contract.get('profile') != 'stream-54-108-cl3-v1':
        raise ValueError('Unsupported/retired SDRAM peer profile')
    for key, value in dict(logic_hz=54_000_000, controller_hz=108_000_000, cas=3,
                           rcd=3, rp=3, capture_latency=3, registered_read_return=1,
                           dq_input_delay_taps=32, phase_degrees=157.5).items():
        if contract.get(key) != value:
            raise ValueError(f'Current stream contract mismatch: {key}')
    for name, identity in contract['files'].items():
        if Path(name).name != name:
            raise ValueError(f'Unexpected peer source path: {name}')
        data = (peer/name).read_bytes()
        if len(data) != identity['bytes'] or fnv1a64(data) != identity['fnv1a64']:
            raise ValueError(f'Stale or mixed stream peer source: {peer/name}')
    return contract


def stage_peer(peer, output, *, ports=True):
    peer, output = Path(peer), Path(output)
    contract = load_contract(peer)
    names = [*contract['files'], 'stream-contract.json']
    if ports and 'ports.vh' not in contract['files']:
        raise ValueError('Peer ports.vh is not bound to its source manifest; export again')
    sources = []
    for name in names:
        if name.casefold() in ('dut.v', 'tb.v'):
            raise ValueError(f'Peer attempts to replace a selected input: {name}')
        destination = output/name
        if (peer/name).resolve() != destination.resolve():
            shutil.copyfile(peer/name, destination)
        if destination.suffix == '.v':
            sources.append(destination)
    load_contract(output)
    return sources
