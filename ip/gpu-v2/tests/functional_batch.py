"""Bounded whole-batch/error-prefix differential against a frozen accepted CLI.

Inputs derive from an explicitly supplied full-scene memory/access image. They
are synthetic mutations, not new CPU or RTL evidence. Each case keeps all raw
files, diagnostics, completion metadata and actual fast-path telemetry.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--baseline', type=Path, required=True)
    parser.add_argument('--candidate', type=Path, required=True)
    parser.add_argument('--input', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=False)
    original = (args.input/'memory-before.bin').read_bytes()
    original_flags = (args.input/'access-before.bin').read_bytes()
    commands = list(struct.unpack_from('<105I', original, 4980736))
    draws = [i for i, word in enumerate(commands) if word >> 28 == 3]
    # Recover real object source locations from native DMA_LOCAL/SET_RENDER.
    source_map = list(range(4096))
    renders = set()
    for word in commands:
        if word >> 28 == 1:
            source = ((word >> 10) & 0x3ffff)*32
            slot = ((word >> 3) & 127)*32
            size = [32, 64, 128][(word >> 1) & 3]
            source_map[slot:slot+size] = range(source, source+size)
        elif word >> 28 == 6:
            renders.add(source_map[(word & 127)*32])
    cases = ['normal', 'unknown-texture', 'unknown-depth', 'denied-color',
             'texture-feedback', 'dma-target-read', 'command-target-alias',
             'late-invalid-state', 'unknown-future-command']
    cases += [f'budget-{n}' for n in [204810, 220000, 400000, 1000000]]
    results = []
    for name in cases:
        folder = args.out/name
        folder.mkdir()
        memory, flags, words = bytearray(original), bytearray(original_flags), commands.copy()
        maximum, base = 64_000_000, 4980736
        if name == 'unknown-texture':
            flags[0x500000:0x503200] = bytes(12800)
        elif name == 'unknown-depth':
            words = [0 if w >> 28 == 8 and w & 65536 else w for w in words]
            memory[0x480000:0x480000+204800] = b'\xff\xff'*102400
            offset = ((200//16)*16 + 120//16)*256 + (120 % 16)*16 + 200 % 16
            flags[0x480000+offset*2] &= 2
        elif name == 'denied-color':
            words = [0 if w >> 28 == 8 and not w & 65536 else w for w in words]
            offset = ((200//16)*16 + 120//16)*256 + (120 % 16)*16 + 200 % 16
            flags[0x440000+offset*2] = 1
        elif name == 'texture-feedback':
            for address in renders:
                struct.pack_into('<I', memory, address,
                                 (0x440000//128) | (9 << 16) | (1 << 23) | (1 << 21))
        elif name == 'dma-target-read':
            words.insert(draws[1]+1, (1 << 28) | ((0x440000//32) << 10) | (76 << 3))
        elif name == 'command-target-alias':
            words = [0 if w >> 28 == 8 and not w & 65536 else w for w in words]
            base = 0x440000
        elif name == 'late-invalid-state':
            address = 0x510000
            memory[address:address+32] = bytes(32)
            memory[address+8] = 64  # Reserved Render control bit.
            flags[address:address+32] = bytes([1])*32
            words[draws[1]+1:draws[1]+1] = [
                (1 << 28) | ((address//32) << 10) | (79 << 3),
                0x40000003, 0x6000004f]
        elif name.startswith('budget-'):
            maximum = int(name.split('-')[1])
        padded = (len(words)*4+127)//128*128
        memory[base:base+padded] = bytes(padded)
        struct.pack_into('<'+'I'*len(words), memory, base, *words)
        if name != 'command-target-alias':
            flags[base:base+padded] = bytes([1])*padded
        if name == 'unknown-future-command':
            flags[base+(draws[1]+1)*4] = 0
        source, access = folder/'before.bin', folder/'before.access'
        source.write_bytes(memory)
        access.write_bytes(flags)
        expected = None
        rows = []
        for label, executable, threads in [('baseline', args.baseline, 1),
                                            ('candidate-1', args.candidate, 1),
                                            ('candidate-4', args.candidate, 4)]:
            destination = folder/(label+'.bin')
            telemetry = folder/(label+'-stats.json')
            run = subprocess.run([str(executable.resolve()), str(source.resolve()),
                str(base), str(len(words)-1), str(maximum), str(destination.resolve()),
                '--access', str(access.resolve())], capture_output=True, timeout=60,
                env=dict(os.environ, RAYON_NUM_THREADS=str(threads), GPU_SIM_REBIN='B',
                         GPU_SIM_BATCH_STATS=str(telemetry.resolve())))
            (folder/(label+'.log')).write_bytes(run.stdout+run.stderr)
            buffers = [Path(str(destination)+suffix).read_bytes()
                       for suffix in ['', '.access', '.completion.json']]
            actual = (run.returncode, run.stdout, run.stderr, buffers)
            if expected is None:
                expected = actual
            else:
                assert actual == expected, (name, label, 'error/memory/access/effects prefix')
            stats = json.loads(telemetry.read_text()) if telemetry.exists() else None
            rows.append(dict(label=label, returncode=run.returncode, stats=stats,
                             sha256=[hashlib.sha256(raw).hexdigest() for raw in buffers]))
            if stats and name == 'normal':
                assert stats['applied'] == 1 and stats['fallbacks'] == 0
                assert stats['batches'][0][0] == 24 and stats['messages'][2] == 4
            if stats and name in ['unknown-texture', 'unknown-depth', 'denied-color',
                                  'late-invalid-state', 'unknown-future-command']:
                assert stats['fallbacks'] > 0, (name, stats)
        results.append(dict(case=name, input_sha256=hashlib.sha256(memory).hexdigest(),
                            access_sha256=hashlib.sha256(flags).hexdigest(), runs=rows))
        (args.out/'result.json').write_text(json.dumps(results, indent=2)+'\n')
        print(name+': exact result/error/full memory/access/effects prefix', flush=True)


if __name__ == '__main__':
    main()
