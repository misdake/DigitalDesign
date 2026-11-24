"""Bounded resident-matrix regression against production ROM, RAM and MAC."""
import argparse
import os
from pathlib import Path
import random
import subprocess


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--native', action='store_true')
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    frontend = Path(__file__).resolve().parents[1] / 'src/frontend/vliw'
    parts = ['primitives/ram.v', 'vertex/mac.v', 'vertex/rf.v',
             'vertex/control_rom.v', 'input_storage.v', 'input_t_work.v',
             't32_core.v', 'frontend.v']
    source = '' if args.native else '`define PORTABLE\n'
    source += '\n'.join((frontend / part).read_text() for part in parts)
    source = source.replace('`include "control_fields.vh"',
                            (frontend / 'vertex/control_fields.vh').read_text())
    bench = Path(__file__).with_name('world_matrix_prepare_tb.v').read_text()
    bench = bench.replace('@GSR@', "GSR GSR(.GSRI(1'b1));" if args.native else '')
    (output / 'dut.v').write_text(source)
    (output / 'tb.v').write_text(bench)
    rng = random.Random(0x627CF1)
    for case in range(32):
        view = [rng.randrange(-2**18, 2**18) for _ in range(32)]
        world = [rng.randrange(-2**18, 2**18) for _ in range(32)]
        world[12] = 65536
        world[13] = rng.randrange(65537)
        world[14] = rng.randrange(131073)
        world[15] = 0
        world[25] = 0
        view[28] = rng.randrange(65537)
        view[29] = rng.randrange(32769)
        gold = []
        for row in range(3):
            for col in range(4):
                value = sum(view[4*row+k] * world[4*k+col] for k in range(3))
                if col == 3:
                    value += view[4*row+3] * 65536
                gold.append((value >> 16) & 0xffffffff)
        for row in range(3):
            for col in range(3):
                value = sum(view[12+3*row+k] * world[16+3*k+col] for k in range(3))
                gold.append((value >> 16) & 0xffffffff)
        gold.append(65536)
        gold.extend([0, 0])
        gold.extend((view[28+i] * world[13+i] + (1 << 23)) >> 16 for i in range(2))
        if case >= 16:
            basis = [view[22], 0, view[23], 0, view[30], 0] if case >= 24 else [view[31], 0, 0, view[31], 0, 0]
            gold[24:30] = basis
        for name, values in [('view', view), ('world', world), ('gold', gold)]:
            (output / f'{name}-{case}.hex').write_text(
                ''.join(f'{value & 0xffffffff:08x}\n' for value in values))
    command = [os.environ.get('IVERILOG_EXE', 'iverilog'), '-g2012', '-s',
               'world_matrix_prepare_tb', '-o', 'test.vvp', 'dut.v', 'tb.v']
    if args.native:
        vendor = Path(os.environ['GOWIN_HOME']) / 'IDE/simlib/gw2a/prim_sim.v'
        command.append(str(vendor))
    for name, command, timeout in [
        ('compile', command, 60),
        ('run', [os.environ.get('VVP_EXE', 'vvp'), 'test.vvp'], 30),
    ]:
        result = subprocess.run(command, cwd=output, capture_output=True, timeout=timeout)
        (output / f'{name}.log').write_bytes(result.stdout + result.stderr)
        if result.returncode:
            raise RuntimeError((result.stdout + result.stderr).decode(errors='replace'))
    print(('Native' if args.native else 'Portable') +
          ' World preparation: matrix/light 97 edges, Billboard 65 edges, CE/DMA stalls PASS')


if __name__ == '__main__':
    main()
