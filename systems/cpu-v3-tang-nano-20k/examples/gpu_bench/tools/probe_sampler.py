"""Bounded isolation of the exported, unmodified cache-to-Color hit cadence."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import xml.etree.ElementTree as ET


def run(project, output, expected_interval=2, compile_only=False):
    output.mkdir(parents=True, exist_ok=False)
    xml = ET.parse(project/'cpu_v3_system.gprj')
    sources = [(project/n.attrib['path']).resolve() for n in xml.findall('.//File')
               if n.attrib['type'] == 'file.verilog']
    primitive = Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v'
    tb = Path(__file__).with_name('probe_sampler_tb.v')
    image = (output/'probe.vvp').resolve()
    command = [os.environ.get('IVERILOG_EXE', 'iverilog'), '-g2012', '-D__ICARUS__', '-s', 'tb',
               '-o', str(image), *map(str, sources), str(primitive), str(tb)]
    result = subprocess.run(command, capture_output=True, timeout=45)
    (output/'compile.log').write_bytes(result.stdout+result.stderr)
    result.check_returncode()
    source_hashes = {str(p): hashlib.sha256(p.read_bytes()).hexdigest()
                     for p in [*sources, primitive, tb]}
    if compile_only:
        (output/'compile.json').write_text(json.dumps(dict(
            scope='HDL compilation only; no simulation or cadence qualification',
            source_sha256=source_hashes), indent=2)+'\n')
        print('PASS compiled sampler probe; execution not requested')
        return
    rows = []
    for mode in (1, 2):
        result = subprocess.run([os.environ.get('VVP_EXE', 'vvp'), str(image), f'+MODE={mode}',
                                 f'+EXPECTED_INTERVAL={expected_interval}'],
                                capture_output=True, timeout=20)
        (output/f'mode-{mode}.log').write_bytes(result.stdout+result.stderr)
        result.check_returncode()
        lines = [line for line in result.stdout.decode().splitlines() if line.startswith('PASS ')]
        if len(lines) != 1:
            raise ValueError('Missing completion record')
        line = lines[0]
        rows.append({k: int(v) for k, v in (field.split('=') for field in line.split()[1:])})
        print(line)
    (output/'summary.json').write_text(json.dumps(dict(
        scope='Cache and Color only; exact exported modules, two resident sectors, no tile crossing',
        expected_group_interval=expected_interval,
        max_cycles=10000, wall_limit_seconds_per_mode=20, results=rows,
        source_sha256=source_hashes), indent=2)+'\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--project', required=True, type=Path)
    parser.add_argument('--out', required=True, type=Path)
    parser.add_argument('--expected-interval', type=int, choices=(0, 2, 3), default=2,
                        help='2: current II2; 3: explicit legacy comparison; 0: observe only')
    parser.add_argument('--compile-only', action='store_true', help='No VVP execution or throughput claim')
    args = parser.parse_args()
    run(args.project, args.out, args.expected_interval, args.compile_only)
