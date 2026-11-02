"""Generate bounded multi-window pressure from the frozen105-command scene.

This is synthetic stress, not another CPU capture or RTL qualification. Native
words are copied from the supplied scene; no second hand-maintained program is
introduced. The accepted CLI computes the independently frozen expected result.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--input', type=Path, required=True)
    parser.add_argument('--accepted', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--repeats', type=int, default=8)
    args = parser.parse_args()
    assert 1 <= args.repeats <= 16
    args.out.mkdir(parents=True, exist_ok=False)
    original = (args.input/'memory-before.bin').read_bytes()
    memory = bytearray(original)
    access = bytearray((args.input/'access-before.bin').read_bytes())
    words = list(struct.unpack_from('<105I', memory, 4980736))
    assert sum(w >> 28 == 3 for w in words) == 24
    assert not any(w >> 28 in (11, 12) for w in words)
    last_clear = max(i for i, w in enumerate(words) if w >> 28 == 8)
    body = [w for w in words if w >> 28 not in (8, 10, 15)]
    commands = words[:last_clear+1]+body*args.repeats+[0x40000003, 0x70000000, 0xf0000000]
    padded = (len(commands)*4+127)//128*128
    assert padded < 65536
    memory[4980736:4980736+padded] = bytes(padded)
    struct.pack_into('<'+'I'*len(commands), memory, 4980736, *commands)
    access[4980736:4980736+padded] = bytes([1])*padded
    source, flags, output = [args.out/name for name in
                             ['memory-before.bin', 'access-before.bin', 'memory-after.bin']]
    source.write_bytes(memory)
    flags.write_bytes(access)
    run = subprocess.run([str(args.accepted.resolve()), str(source.resolve()),
        '4980736', str(len(commands)-1), '64000000', str(output.resolve()),
        '--access', str(flags.resolve())], capture_output=True, timeout=60)
    (args.out/'baseline.log').write_bytes(run.stdout+run.stderr)
    assert run.returncode == 0, run.stderr
    identity = dict(scope='Synthetic repeated scene; not CPU/RTL evidence',
        accepted_sha256=hashlib.sha256(args.accepted.read_bytes()).hexdigest(),
        original_memory_sha256=hashlib.sha256(original).hexdigest(),
        cb_base=4980736, count_minus_one=len(commands)-1, draws=24*args.repeats,
        files={path.name: hashlib.sha256(path.read_bytes()).hexdigest()
               for path in args.out.iterdir()})
    (args.out/'identity.json').write_text(json.dumps(identity, indent=2)+'\n')


if __name__ == '__main__':
    main()
