"""Bounded same-input regression against a separately frozen native SIM binary.

No RTL is run. Reuse the independent functional_replay vectors, including the
production asset subset, and compare full physical memory/access/effects.
Successful raw files are retained compressed; a failed run retains loose files.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import zipfile

sys.dont_write_bytecode = True
from functional_replay import (COLUMN, memcpy_capture, snapshot_abi,
                               source_case, synthetic_snapshot)
from simulator import seed

CASES = ['nearest', 'rgb', 'blend', 'diffuse', 'specular', 'bilinear',
         'trilinear', 'rtt-nearest', 'rtt-bilinear', 'ui-full', 'ui-range',
         'alpha-endpoints', 'depth-functions', 'normal16-half', 'normal18-half',
         'normal16-negative', 'normal18-negative', 'vertex-wrap', 'memcpy',
         'classic-floor-two-bunny']


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--baseline', type=Path, required=True)
    parser.add_argument('--candidate', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--threads', nargs='+', type=int, default=[1, 2, 4, 8])
    parser.add_argument('--cases', nargs='+', choices=CASES, default=CASES)
    args = parser.parse_args()
    assert all(1 <= n <= 64 for n in args.threads)
    args.out.mkdir(parents=True, exist_ok=False)
    summary = dict(baseline_sha256=sha(args.baseline.read_bytes()),
                   candidate_sha256=sha(args.candidate.read_bytes()),
                   script_sha256=sha(Path(__file__).read_bytes()),
                   timeout_seconds=30, maximum_work=64_000_000, cases=[])
    for name in args.cases:
        folder = args.out / name
        folder.mkdir()
        capture = (memcpy_capture(snapshot_abi.Abi())[0] if name == 'memcpy'
                   else synthetic_snapshot(COLUMN, source_case(name)))
        capture.save(folder / 'snapshot')
        memory, access = seed(capture)
        source, flags = folder/'memory.bin', folder/'access.bin'
        source.write_bytes(memory)
        flags.write_bytes(access)
        record = dict(name=name, input_sha256=capture.manifest()['input_sha256'],
                      memory_sha256=sha(memory), access_sha256=sha(access), runs=[])
        expected = None
        generated = [source, flags]
        with zipfile.ZipFile(folder/'physical-results.zip', 'w',
                             zipfile.ZIP_DEFLATED, compresslevel=1) as archive:
            archive.write(source, source.name)
            archive.write(flags, flags.name)
            for label, executable, threads in [('baseline', args.baseline, 1)] + [
                    (f'candidate-{n}', args.candidate, n) for n in args.threads]:
                output = folder/f'{label}.bin'
                command = [str(executable.resolve()), str(source.resolve()),
                           str(capture.header['cb_base']),
                           str(capture.header['count_minus_one']), '64000000',
                           str(output.resolve()), '--access', str(flags.resolve())]
                run = subprocess.run(command, capture_output=True, timeout=30,
                                     env=dict(os.environ, RAYON_NUM_THREADS=str(threads)))
                (folder/f'{label}.log').write_bytes(run.stdout+run.stderr)
                paths = [output, Path(str(output)+'.access'),
                         Path(str(output)+'.completion.json')]
                raw = [p.read_bytes() for p in paths]
                actual = (run.returncode, run.stdout, run.stderr, raw)
                if expected is None:
                    assert run.returncode == 0, (name, run.stderr)
                    expected = actual
                else:
                    assert actual == expected, (name, threads, 'memory/access/effects/diagnostic mismatch')
                record['runs'].append(dict(label=label, threads=threads,
                    sha256=[sha(data) for data in raw], full_physical_equal=True))
                for path in paths:
                    archive.write(path, path.name)
                    generated.append(path)
        for path in generated:
            path.unlink()
        summary['cases'].append(record)
        (args.out/'result.json').write_text(json.dumps(summary, indent=2)+'\n')
        print(f'{name}: full memory/access/effects equal at {args.threads}', flush=True)


if __name__ == '__main__':
    main()
