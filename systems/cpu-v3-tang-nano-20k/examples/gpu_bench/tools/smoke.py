"""Small bounded production-GPU correctness check; no performance claim."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import xml.etree.ElementTree as ET


def run(project, assets, output, selected, compile_only=False):
    output.mkdir(parents=True, exist_ok=False)
    manifest = json.loads((assets/'assets.json').read_text())
    resource = manifest['resources'][0]
    payload = (assets/resource['file']).read_bytes()
    assert hashlib.sha256(payload).hexdigest() == resource['sha256']
    with (output/'bench.hex').open('w') as f:
        f.write(f'@{resource["address"]//8:x}\n')
        for at in range(0, len(payload), 8):
            f.write(f'{int.from_bytes(payload[at:at+8], "little"):016x}\n')
    xml = ET.parse(project/'cpu_v3_system.gprj')
    sources = [(project/n.attrib['path']).resolve() for n in xml.findall('.//File')
               if n.attrib['type'] == 'file.verilog']
    primitive = Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v'
    tb = Path(__file__).with_name('smoke_tb.v')
    command = [os.environ.get('IVERILOG_EXE', 'iverilog'), '-g2012', '-D__ICARUS__', '-s', 'tb',
               '-o', str((output/'smoke.vvp').resolve()), *map(str, sources), str(primitive), str(tb)]
    result = subprocess.run(command, capture_output=True, timeout=45)
    (output/'compile.log').write_bytes(result.stdout+result.stderr)
    if result.returncode:
        raise RuntimeError('Smoke compile failed; see compile.log')
    source_hashes = {str(p): hashlib.sha256(p.read_bytes()).hexdigest()
                     for p in [*sources, primitive, tb]}
    if compile_only:
        (output/'compile.json').write_text(json.dumps(dict(
            scope='HDL compilation only; no simulation or physical MC qualification',
            source_sha256=source_hashes), indent=2)+'\n')
        print('PASS compiled GPU smoke; execution not requested')
        return
    records = []
    for name in selected:
        case = next(c for c in manifest['cases'] if c['name'] == name)
        expected = (0 if name in ['fill_rgb_depth_less_reject', 'cull_behind'] else
                    96000 if case['family'] in ['fill', 'lighting'] else -1)
        # Rejected geometry needs no plane preimage or GPU CLEAR. Omitting the
        # separately timed setup keeps this optional correctness gate small.
        setup_count = 0 if case['name'] == 'cull_behind' else case['setup_instructions']
        args = [os.environ.get('VVP_EXE', 'vvp'), 'smoke.vvp',
                f'+SETUP_BASE={case["setup_base"]}', f'+SETUP_COUNT={setup_count}',
                f'+WORK_BASE={case["work_base"]}', f'+WORK_COUNT={case["work_instructions"]}',
                f'+COLOR_BASE={manifest["targets"]["color_base"]}', f'+EXPECTED_PIXELS={expected}']
        log_path = output/(name+'.log')
        with log_path.open('wb') as stream:
            result = subprocess.run(args, cwd=output, stdout=stream, stderr=subprocess.STDOUT, timeout=90)
        log = log_path.read_bytes()
        if result.returncode or b'PASS pixels=' not in log:
            raise RuntimeError('Smoke failed: '+name+'; see '+str(output/(name+'.log')))
        records.append(dict(case=name, result=log.decode('utf8', errors='replace').strip()))
        print(records[-1]['result'], flush=True)
    (output/'summary.json').write_text(json.dumps(dict(
        purpose='functional correctness only, independent delayed memory peer, not MC timing',
        asset_sha256=resource['sha256'],
        source_sha256=source_hashes,
        max_cycles=3000000, wall_seconds_per_case=90, cases=records), indent=2)+'\n')


if __name__ == '__main__':
    p = argparse.ArgumentParser(__doc__)
    p.add_argument('--project', type=Path, required=True)
    p.add_argument('--assets', type=Path, required=True)
    p.add_argument('--out', type=Path, required=True)
    p.add_argument('--cases', nargs='+', default=['cull_behind'])
    p.add_argument('--compile-only', action='store_true', help='No VVP execution or performance claim')
    a = p.parse_args()
    run(a.project, a.assets, a.out, a.cases, a.compile_only)
