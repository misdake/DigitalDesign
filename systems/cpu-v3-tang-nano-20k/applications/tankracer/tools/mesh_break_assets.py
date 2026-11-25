"""Export original mesh-fragment answers and authored ENGLAND auxiliary meshes."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reference', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    root = Path(args.reference.resolve().parent.as_posix().removeprefix('//?/'))
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    owner = Path(__file__).resolve().parent
    project = out / 'reference-tool'
    project.mkdir(exist_ok=True)
    manifest = ('[package]\nname="tankracer-mesh-break-assets"\nversion="0.1.0"\nedition="2024"\n'
                '[workspace]\n[[bin]]\nname="mesh-break-oracle"\npath='
                + json.dumps((owner / 'mesh_break_oracle.rs').as_posix())
                + '\n[dependencies]\nserde_json="=1.0.109"\ntankracer-game={path='
                + json.dumps((root / 'crates/game').as_posix()) + '}\ntankracer-assets={path='
                + json.dumps((root / 'crates/assets').as_posix()) + '}\n')
    (project / 'Cargo.toml').write_text(manifest)
    with (project / 'build.log').open('w') as log:
        subprocess.run(['cargo', 'build', '--release', '--offline', '--manifest-path', str(project / 'Cargo.toml'),
                        '--target-dir', str(project / 'target')], check=True, timeout=300,
                       stdout=log, stderr=subprocess.STDOUT)
    exe = project / 'target/release' / ('mesh-break-oracle.exe' if os.name == 'nt' else 'mesh-break-oracle')
    subprocess.run([str(exe), str(out), str(root)], check=True, timeout=60)
    subprocess.run([sys.executable, str(owner / 'export_trig_assets.py'), '--reference', str(root),
                    '--output', str(out)], check=True, timeout=60, stdout=subprocess.DEVNULL)
    sources = [owner / 'mesh_break_oracle.rs', owner / 'fragment_snapshot.rs', owner / 'reference_world.rs', Path(__file__).resolve()]
    sources += [root / 'crates/game/src' / p for p in ['mesh_break.rs', 'fixed.rs', 'geometry.rs', 'math.rs',
                'random.rs', 'scene_meshes.rs', 'obstacle_startup.rs', 'scenery.rs', 'world_startup.rs']]
    sources += [root / 'assets/game' / p / 'FASTFILE.DAT' for p in
                ['DATA', 'INGAME', 'TANKS/BRONZE', 'TANKS/SILVER', 'TANKS/GOLD', 'LEVELS/ENGLAND']]
    outputs = ['mesh-break.json', 'sine.i16.bin', 'angle.i16.bin']
    (out / 'mesh-break-sources.json').write_text(json.dumps(dict(
        sources=[dict(file=str(p), sha256=hashlib.sha256(p.read_bytes()).hexdigest()) for p in sources],
        outputs={name: hashlib.sha256((out / name).read_bytes()).hexdigest() for name in outputs}), indent=2) + '\n')


if __name__ == '__main__':
    main()
