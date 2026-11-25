"""Export original parent/auxiliary method chains and real ENGLAND terrain."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
from reference_bridge import prepare


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reference', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    root = Path(args.reference.resolve().parent.as_posix().removeprefix('//?/'))
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    owner = Path(__file__).resolve().parent
    game = prepare(root, out, lifecycle=True, meshes=True)
    project = out / 'reference-tool'
    project.mkdir(exist_ok=True)
    manifest = ('[package]\nname="tankracer-parent-assets"\nversion="0.1.0"\nedition="2024"\n'
                '[workspace]\n[[bin]]\nname="parent-oracle"\npath='
                + json.dumps((owner / 'parent_auxiliary_oracle.rs').as_posix())
                + '\n[dependencies]\nserde_json="=1.0.109"\ntankracer-game={path='
                + json.dumps(game.as_posix()) + '}\ntankracer-assets={path='
                + json.dumps((root / 'crates/assets').as_posix()) + '}\n')
    (project / 'Cargo.toml').write_text(manifest)
    with (project / 'build.log').open('w') as log:
        subprocess.run(['cargo', 'build', '--release', '--offline', '--manifest-path', str(project / 'Cargo.toml'),
                        '--target-dir', str(project / 'target')], check=True, timeout=300,
                       stdout=log, stderr=subprocess.STDOUT)
    exe = project / 'target/release' / ('parent-oracle.exe' if os.name == 'nt' else 'parent-oracle')
    subprocess.run([str(exe), str(out), str(root)], check=True, timeout=300)
    subprocess.run([sys.executable, str(owner / 'export_trig_assets.py'), '--reference', str(root),
                    '--output', str(out)], check=True, timeout=60, stdout=subprocess.DEVNULL)
    table = root / 'crates/game/data/cosine-angle.bin'
    (out / table.name).write_bytes(table.read_bytes())
    sources = [owner / n for n in ['parent_auxiliary_oracle.rs', 'mesh_store_snapshot.rs',
        'identity_assets.rs', 'scene_lifecycle_assets.rs', 'reference_world.rs', 'scene_export.rs',
        'reference_bridge.py', 'reference_scene.rs', 'reference_meshes.rs']]
    sources += [Path(__file__).resolve(), table]
    overlay = json.loads((out / 'reference-overlay.json').read_text())
    sources += [Path(s['file']) for s in overlay['sources']]
    sources += [root / 'assets/game' / n / 'FASTFILE.DAT' for n in ['DATA', 'INGAME', 'TANKS/BRONZE',
                'TANKS/SILVER', 'TANKS/GOLD', 'LEVELS/ENGLAND']]
    outputs = ['parent-auxiliary.json', 'parent-scene.bin', 'parent-cells.bin',
               'sine.i16.bin', 'angle.i16.bin', 'cosine-angle.bin']
    (out / 'parent-sources.json').write_text(json.dumps(dict(
        sources=[dict(file=str(p), sha256=hashlib.sha256(p.read_bytes()).hexdigest()) for p in sources],
        outputs={name: hashlib.sha256((out / name).read_bytes()).hexdigest() for name in outputs}), indent=2) + '\n')


if __name__ == '__main__':
    main()
