"""Deterministic MEMCPY diagnostic specialization, before the capture lease."""
import argparse
import hashlib
import json
from pathlib import Path

SYSTEM = Path(__file__).resolve().parents[3]
TAIL = SYSTEM / 'tests/fixtures/capture_native_tail.rcc'
MARKER = '    GPU_CAPTURE_CONTROL[2u16] += 1;'


def materialize(shared, output, reserved=False):
    shared, output = Path(shared).resolve(), Path(output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    extension = TAIL.read_text()
    if reserved:
        marker = 'p.write(4, 0x6a6a);'
        if extension.count(marker) != 1:
            raise ValueError('Native negative fixture marker moved')
        extension = extension.replace(marker, 'p.write(2, 0x80); ' + marker)
    origins = []
    for path in sorted(shared.glob('*.rs')):
        original = path.read_bytes()
        data = original
        if path.name == 'gpu_capture.rs':
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
    if [e['module'] for e in origins if e['changed']] != ['gpu_capture.rs']:
        raise ValueError('Expected exactly one specialized capture module')
    identity = dict(case='memcpy', reserved_negative=reserved, extension=str(TAIL),
                    extension_sha256=hashlib.sha256(TAIL.read_bytes()).hexdigest(), modules=origins,
                    boundary='Before header/range construction and publication lease; existing encoders/scheduler')
    (output / 'specialization.json').write_text(json.dumps(identity, indent=2) + '\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--shared', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    materialize(args.shared, args.output)
