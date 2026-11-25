"""Build bounded original scene-retirement cases for normal CB06 execution."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess
from reference_bridge import prepare


def binary(path, values):
    path.write_bytes(struct.pack('<' + 'H' * len(values), *values))


def words(values):
    return [word for value in values for word in [value & 65535, (value >> 16) & 65535]]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    out = parser.parse_args().output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    owner = Path(__file__).resolve().parent
    root = Path(os.environ['TANKRACER_REFERENCE']).resolve().parent
    root = Path(root.as_posix().removeprefix('//?/'))
    project = out / 'reference-tool'
    project.mkdir(exist_ok=True)
    game = prepare(root, out, lifecycle=True)
    manifest = ('[package]\nname="tankracer-retirement-assets"\nversion="0.1.0"\nedition="2024"\n'
                '[workspace]\n[[bin]]\nname="scene-lifecycle-oracle"\npath='
                + json.dumps((owner / 'scene_lifecycle_oracle.rs').as_posix())
                + '\n[dependencies]\nserde_json="=1.0.109"\ntankracer-game={path=' + json.dumps(game.as_posix())
                + '}\ntankracer-assets={path=' + json.dumps((root / 'crates/assets').as_posix()) + '}\n')
    (project / 'Cargo.toml').write_text(manifest)
    with (project / 'build.log').open('w') as log:
        subprocess.run(['cargo', 'build', '--release', '--offline', '--manifest-path', str(project / 'Cargo.toml'),
                        '--target-dir', str(project / 'target')], check=True, stdout=log, stderr=subprocess.STDOUT, timeout=300)
    exe = project / 'target/release' / ('scene-lifecycle-oracle.exe' if os.name == 'nt' else 'scene-lifecycle-oracle')
    subprocess.run([str(exe), str(out), str(root)], check=True, timeout=300)
    all_cases = json.loads((out / 'scene-lifecycle.json').read_text())
    first = {kind: next(c['object'] for c in all_cases if c['tag'] == kind) for kind in [1, 2]}
    cases = [c for c in all_cases if c['action'] <= 2 and c['object'] == first[c['tag']] and
             ((c['variant'] == 0 and c['limit'] == 100000) or
              (c['variant'] == 9 and c['limit'] == 100000 and c['action'] == 0) or
              (c['variant'] == 10 and c['limit'] == 100000 and c['action'] == 1) or
              (c['variant'] == 4 and c['limit'] == 100000 and c['action'] == 1) or
              (c['variant'] == 0 and c['limit'] == 1 and c['action'] == 0))]
    cases += [c for c in all_cases if c.get('completion') and c['tag'] in first and
              c['object'] == first[c['tag']] and c['limit'] == 100000 and c['variant'] == 0]
    cases += [c for c in all_cases if c.get('completion') and c['object'] == first[1] and c['action'] == 3 and
              ((c['variant'] == 9 and c['limit'] == 100000) or (c['variant'] == 0 and c['limit'] == 1))]
    assert len(cases) == 20
    inputs, expected, directory = [], [], []
    for c in cases:
        inp, exp = words(c['input']), words(c['expected'])
        directory.extend([len(inputs) & 65535, len(inputs) >> 16, len(inp), len(expected) & 65535, len(expected) >> 16, len(exp)])
        inputs.extend(inp)
        expected.extend(exp)
    binary(out / 'inputs.bin', inputs)
    binary(out / 'expected.bin', expected)
    binary(out / 'directory.bin', directory)
    declarations = ['use crate::dsl_rt::*;mod static_assets;use static_assets::AssetBytes;\n']
    resources = []
    for symbol, name in [('SCENE', 'scene-lifecycle.bin'), ('INPUTS', 'inputs.bin'), ('EXPECTED', 'expected.bin'), ('DIRECTORY', 'directory.bin')]:
        path = out / name
        declarations.append(f'#[asset(align=128)] pub static RETIREMENT_{symbol}:AssetBytes=AssetBytes::new(include_bytes!({json.dumps(path.as_posix())}));\n')
        resources.append(dict(file=name, placement='source', symbol='RETIREMENT_' + symbol,
                              bytes=path.stat().st_size, sha256=hashlib.sha256(path.read_bytes()).hexdigest()))
    maximum_input = max(len(c['input']) * 2 for c in cases)
    maximum_output = max(len(c['expected']) * 2 for c in cases)
    assert maximum_output <= maximum_input
    declarations.append(f'pub const RETIREMENT_CASES:u16={len(cases)};\n')
    declarations.append(f'pub struct RetirementStorage{{pub input:Buf<u16,{maximum_input}>,pub output:Buf<u16,{maximum_output}>}}\n')
    declarations.append(f'pub fn retirement_storage()->RetirementStorage{{RetirementStorage{{input:Buf::new([0;{maximum_input}]),output:Buf::new([0;{maximum_output}])}}}}\n')
    (out / 'retirement_assets.rs').write_text(''.join(declarations))
    imports = 'use crate::dsl_rt::*;use crate::{attitude,entities,identity_capture,lifecycle,ordered,scene_lifecycle,scene_query,scene_world,spatial,static_assets};\n'
    (out / 'retirement_check.rs').write_text(imports + (owner / 'scene_lifecycle_case.rs').read_text())
    sources = [owner / n for n in ['retirement_assets.py', 'scene_lifecycle_oracle.rs', 'scene_lifecycle_assets.rs',
        'scene_lifecycle_case.rs', 'identity_assets.rs', 'identity_capture.rs', 'reference_bridge.py', 'reference_scene.rs',
        'reference_world.rs', 'reference_tanks.rs', 'scene_export.rs']]
    overlay = json.loads((out / 'reference-overlay.json').read_text())
    sources += [Path(s['file']) for s in overlay['sources']]
    sources += [root / 'assets/game' / n / 'FASTFILE.DAT' for n in ['DATA','INGAME','TANKS/BRONZE','TANKS/SILVER','TANKS/GOLD','LEVELS/ENGLAND']]
    metadata = dict(cases=[{k:c[k] for k in ['object','tag','variant','action','limit']} for c in cases],
                    checked_i32_values=sum(len(c['expected']) for c in cases),
                    scope='Original scene Stop, destruction, deactivation/completion class dispatch, component retirement, free slots and shared identity/list state; normal immutable asset loading')
    (out / 'assets.json').write_text(json.dumps(dict(resources=resources, sector_relocations=[], metadata=metadata,
        sources=[dict(file=str(p), sha256=hashlib.sha256(p.read_bytes()).hexdigest()) for p in sources]), indent=2) + '\n')


if __name__ == '__main__':
    main()
