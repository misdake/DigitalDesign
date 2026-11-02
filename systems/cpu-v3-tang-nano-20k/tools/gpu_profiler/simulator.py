"""Result-only native GPU backend for the existing frozen-dump frontends."""
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess

from images import preview, preview_policy
from layout import COMMAND_ABI, capture_description, header_layout
from snapshot import SnapshotError

MEMORY_BYTES = 0x800000
DEFAULT_WORK = 64_000_000


def write_result(data, folder, source=None):
    """Use the existing viewer for a distinct final-result document, never a profile."""
    if (data.get('schema') != 'digitaldesign.gpu-result' or data.get('version') != 1
            or data.get('backend') != 'sim' or data.get('complete') is not True
            or type(data.get('has_target')) is not bool or not isinstance(data.get('outputs'), list)
            or any(key in data for key in ('cold_submission_cycles', 'clock_hz', 'metric_defs', 'tracks'))):
        raise SnapshotError('Invalid result-only simulator document')
    for key in ('input_sha256', 'executable_sha256'):
        if not re.fullmatch('[0-9a-f]{64}', data.get(key, '')):
            raise SnapshotError('Missing simulator result identity')
    if data['has_target']:
        for role in ('color', 'depth'):
            entry = data.get('final', {}).get(role, {})
            if not any(all(row.get(key) == entry.get(key) for key in ('file', 'address', 'bytes', 'sha256'))
                       for row in data['outputs']):
                raise SnapshotError('Missing final framebuffer result')
        image = data['final']['color'].get('image', '')
        if not image.startswith('data:image/png;base64,'):
            raise SnapshotError('Missing final PNG preview')
        if not base64.b64decode(image.split(',', 1)[1], validate=True).startswith(b'\x89PNG\r\n\x1a\n'):
            raise SnapshotError('Invalid final PNG preview')
    folder = Path(folder)
    for entry in data['outputs']:
        if (not re.fullmatch(r'capture/(?:color|depth|range-\d+)\.bin', entry.get('file', ''))
                or type(entry.get('address')) is not int or type(entry.get('bytes')) is not int
                or not 0 <= entry['address'] < entry['address']+entry['bytes'] <= MEMORY_BYTES
                or not re.fullmatch('[0-9a-f]{64}', entry.get('sha256', ''))):
            raise SnapshotError('Invalid final output extent/path/identity')
        if source is not None:
            original = Path(source)/entry['file']
            raw = original.read_bytes()
            if len(raw) != entry['bytes'] or hashlib.sha256(raw).hexdigest() != entry['sha256']:
                raise SnapshotError('Final raw output identity changed')
            destination = folder/entry['file']
            if destination.resolve() != original.resolve():
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(original, destination)
    folder.mkdir(parents=True, exist_ok=True)
    payload = json.dumps(data, separators=(',', ':'), ensure_ascii=True)
    (folder/'result.json').write_text(payload+'\n')
    template = Path(__file__).with_name('viewer.html').read_text()
    safe = payload.replace('<', '\\u003c').replace('>', '\\u003e').replace('&', '\\u0026')
    (folder/'profiler.html').write_text(template.replace('/*PROFILE_DATA*/null', safe))


def executable_path(path=None):
    path = Path(path) if path else Path(__file__).resolve().parents[4] / 'target/release' / (
        'gpu-v2-sim.exe' if os.name == 'nt' else 'gpu-v2-sim')
    if not path.is_file():
        raise SnapshotError('Native simulator not built: build -p gpu-v2 --bin gpu-v2-sim, or supply --simulator')
    return path.resolve()


def seed(capture):
    """Keep physical aliases, captured-byte validity and WRITE facts separate."""
    memory, access = bytearray(MEMORY_BYTES), bytearray(MEMORY_BYTES)
    for region in capture.ranges:
        data = bytes(capture.data[region.index])
        if region.address < 0 or region.address + region.length > MEMORY_BYTES or len(data) != region.length:
            raise SnapshotError('Captured range exceeds physical simulator memory')
        for offset, value in enumerate(data, region.address):
            if access[offset] & 1 and memory[offset] != value:
                raise SnapshotError(f'Conflicting captured physical alias at {offset:#x}')
            memory[offset] = value
            access[offset] |= 1 | (2 if region.flags & capture.abi['WRITE'] else 0)
    return memory, access


def run(capture, out, identity, *, executable=None, maximum_work=DEFAULT_WORK, timeout=60,
        preview_mode='capture', capture_format_known=True):
    if maximum_work < 1 or timeout <= 0:
        raise SnapshotError('Simulator requires positive work and process bounds')
    if capture.header['command_abi'] != COMMAND_ABI:
        raise SnapshotError('Simulator requires the current native command ABI')
    executable = executable_path(executable)
    out = Path(out).resolve()
    memory, access = seed(capture)
    out.mkdir(parents=True, exist_ok=False)
    source, flags, destination = (out / name for name in ('memory-before.bin', 'access-before.bin', 'memory-after.bin'))
    source.write_bytes(memory)
    flags.write_bytes(access)
    request = dict(backend='sim', input_sha256=capture.manifest()['input_sha256'],
                   executable_sha256=hashlib.sha256(executable.read_bytes()).hexdigest(),
                   maximum_work=maximum_work, timeout_seconds=timeout,
                   provenance=identity, scope='Native CB final results only; no profiling or cycle model')
    (out/'sim-request.json').write_text(json.dumps(request, indent=2)+'\n')
    try:
        process = subprocess.run([str(executable), str(source), str(capture.header['cb_base']),
                                  str(capture.header['count_minus_one']), str(maximum_work), str(destination),
                                  '--access', str(flags)], capture_output=True, timeout=timeout)
        (out/'sim.log').write_bytes(process.stdout+process.stderr)
        if process.returncode:
            raise SnapshotError('Native simulator failed: '+process.stderr.decode(errors='replace').strip())
        after = destination.read_bytes()
        known = Path(str(destination)+'.access').read_bytes()
        completion = json.loads(Path(str(destination)+'.completion.json').read_text())
        if len(after) != MEMORY_BYTES or len(known) != MEMORY_BYTES:
            raise SnapshotError('Incomplete native simulator memory/validity output')
        if any(a != b and not permit & 2 for a, b, permit in zip(memory, after, access)):
            raise SnapshotError('Simulator modified bytes outside captured writable ranges')
        target = ([capture.header['color_base'], capture.header['depth_base']]
                  if capture.header.get('has_target', True) else None)
        if completion.get('complete') is not True or completion.get('target') != target:
            raise SnapshotError('Native completion target does not match the captured final target')
        result = dict(schema='digitaldesign.gpu-result', version=1, backend='sim', complete=True,
                      input_sha256=request['input_sha256'], executable_sha256=request['executable_sha256'],
                      provenance=identity, layout=capture_description(capture.header),
                      has_target=capture.header.get('has_target', True), outputs=[], final={},
                      effects={key: completion[key] for key in ('target', 'written_bytes')}, scope=request['scope'])
        output_dir = out/'capture'
        output_dir.mkdir()
        def export(address, size, name, role):
            if address < 0 or address+size > MEMORY_BYTES or any(not value & 1 for value in known[address:address+size]):
                raise SnapshotError('Final output includes unknown or invalid physical bytes')
            data = after[address:address+size]
            (output_dir/name).write_bytes(data)
            entry = dict(role=role, address=address, bytes=size, file='capture/'+name,
                         sha256=hashlib.sha256(data).hexdigest())
            result['outputs'].append(entry)
            return entry
        if result['has_target']:
            layout = header_layout(capture.header)
            for role in ('color', 'depth'):
                result['final'][role] = dict(export(capture.header[role+'_base'], layout.plane_bytes, role+'.bin', role))
            png = out/'color.png'
            linear, presentation = preview_policy(preview_mode, capture.header['target_format'], capture_format_known)
            preview((output_dir/'color.bin').read_bytes(), png, linear, layout)
            result['final']['color']['preview'] = presentation
            result['final']['color']['image'] = 'data:image/png;base64,'+base64.b64encode(png.read_bytes()).decode()
        for region in capture.ranges:
            if region.flags & capture.abi['WRITE']:
                export(region.address, region.length, f'range-{region.index}.bin', 'write-range')
        write_result(result, out)
        return result
    except (OSError, ValueError, SnapshotError, subprocess.SubprocessError) as error:
        failure = dict(complete=False, backend='sim', error=str(error))
        if destination.is_file():
            failure['partial_memory'] = destination.name
        if Path(str(destination)+'.access').is_file():
            failure['partial_access'] = destination.name+'.access'
        if Path(str(destination)+'.completion.json').is_file():
            failure['native_completion'] = destination.name+'.completion.json'
        (out/'failure.json').write_text(json.dumps(failure, indent=2)+'\n')
        raise
