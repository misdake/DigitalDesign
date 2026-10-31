"""Deterministic MEMCPY diagnostic specialization, before the capture lease."""
import argparse
import hashlib
import json
from pathlib import Path

SYSTEM = Path(__file__).resolve().parents[3]
TAIL = SYSTEM / 'tests/fixtures/capture_native_tail.rcc'
MARKER = '    GPU_CAPTURE_CONTROL[2u16] += 1;'
EXPLICIT_TAIL = SYSTEM / 'tests/fixtures/capture_native_explicit_tail.rcc'


def materialize(shared, output, reserved=False, explicit_api=False):
    shared, output = Path(shared).resolve(), Path(output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    tail = EXPLICIT_TAIL if explicit_api else TAIL
    extension = tail.read_text()
    if reserved:
        marker = '    payload[4u16] = 0x6a6a;' if explicit_api else 'p.write(4, 0x6a6a);'
        if extension.count(marker) != 1:
            raise ValueError('Native negative fixture marker moved')
        invalid = '    payload[2u16] = 0x80;\n' if explicit_api else 'p.write(2, 0x80); '
        extension = extension.replace(marker, invalid + marker)
    origins = []
    paths={p.name:p for p in shared.glob('*.rs')}
    if not explicit_api and 'gpu_capture.rs' not in paths:
        paths.update({p.name:p for p in (SYSTEM/'tests/fixtures/legacy_renderer').glob('*.rs')})
    for path in sorted(paths.values(),key=lambda p:p.name):
        original = path.read_bytes()
        data = original
        if explicit_api and path.name == 'gpu_packet.rs':
            data = (original.decode('utf-8') + '\n' + extension).encode('utf-8')
        elif explicit_api and path.name == 'gpu_execute.rs':
            source = original.decode('utf-8')
            marker = '    if !gpu_record_seal(slot) {'
            if source.count(marker) != 1:
                raise ValueError('Explicit native diagnostic seal boundary moved')
            source = source.replace('use gpu_record::*;', 'use gpu_record::*;\nmod gpu_packet;use gpu_packet::*;')
            source = source.replace(marker, '    if !profiling_native_explicit_tail(slot) { return gpu_execute_fail(4); }\n' + marker)
            data = source.encode('utf-8')
        elif not explicit_api and path.name == 'gpu_capture.rs':
            source = original.decode('utf-8')
            if source.count(MARKER) != 1:
                raise ValueError('Native diagnostic capture boundary moved')
            source = source.replace(MARKER, '    if !profiling_native_tail(cb) { gpu_capture_fail(GPU_CAPTURE_DEPENDENCY); return false; }\n' + MARKER)
            data = (source + '\n' + extension).encode('utf-8')
        dest = output / path.name
        if not dest.is_file() or dest.read_bytes() != data:
            dest.write_bytes(data)
        origins.append(dict(module=path.name, source=str(path),
                            source_sha256=hashlib.sha256(original).hexdigest(),
                            generated_sha256=hashlib.sha256(data).hexdigest(), changed=data != original))
    expected = ['gpu_execute.rs','gpu_packet.rs'] if explicit_api else ['gpu_capture.rs']
    if [e['module'] for e in origins if e['changed']] != expected:
        raise ValueError('Unexpected native specialization module set')
    identity = dict(case='memcpy', reserved_negative=reserved, extension=str(tail),explicit_api=explicit_api,
                    extension_sha256=hashlib.sha256(tail.read_bytes()).hexdigest(), modules=origins,
                    boundary='Before explicit seal/publication; declared facts and original execute owner' if explicit_api else 'Before header/range construction and publication lease; existing encoders/scheduler')
    (output / 'specialization.json').write_text(json.dumps(identity, indent=2) + '\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--shared', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--explicit-api',action='store_true')
    args = parser.parse_args()
    materialize(args.shared, args.output, explicit_api=args.explicit_api)
