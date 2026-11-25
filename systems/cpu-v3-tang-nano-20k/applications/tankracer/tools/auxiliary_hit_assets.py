"""Export actual auxiliary-contact methods, resources and ENGLAND terrain."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
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
    manifest = ('[package]\nname="tankracer-auxiliary-hit-assets"\nversion="0.1.0"\nedition="2024"\n'
                '[workspace]\n[[bin]]\nname="auxiliary-hit-oracle"\npath='
                + json.dumps((owner / 'auxiliary_hit_oracle.rs').as_posix())
                + '\n[dependencies]\nserde_json="=1.0.109"\ntankracer-game={path='
                + json.dumps(game.as_posix()) + '}\ntankracer-assets={path='
                + json.dumps((root / 'crates/assets').as_posix()) + '}\n')
    (project / 'Cargo.toml').write_text(manifest)
    with (project / 'build.log').open('w') as log:
        subprocess.run(['cargo', 'build', '--release', '--offline', '--manifest-path', str(project / 'Cargo.toml'),
                        '--target-dir', str(project / 'target')], check=True, timeout=300,
                       stdout=log, stderr=subprocess.STDOUT)
    exe = project / 'target/release' / ('auxiliary-hit-oracle.exe' if os.name == 'nt' else 'auxiliary-hit-oracle')
    subprocess.run([str(exe), str(out), str(root)], check=True, timeout=300)
    sources = [owner / n for n in ['auxiliary_hit_oracle.rs', 'mesh_store_snapshot.rs', 'reference_world.rs',
        'reference_bridge.py', 'reference_meshes.rs', 'reference_scene.rs', 'scene_export.rs', 'terrain_assets.rs',
        'identity_assets.rs', 'scene_lifecycle_assets.rs']]
    sources.append(Path(__file__).resolve())
    overlay = json.loads((out / 'reference-overlay.json').read_text())
    sources += [Path(s['file']) for s in overlay['sources']]
    sources += [root / 'assets/game' / n / 'FASTFILE.DAT' for n in ['DATA','INGAME','TANKS/BRONZE','TANKS/SILVER','TANKS/GOLD','LEVELS/ENGLAND']]
    outputs = ['auxiliary-hit.json', 'auxiliary-cells.bin']
    (out / 'auxiliary-hit-sources.json').write_text(json.dumps(dict(
        sources=[dict(file=str(p), sha256=hashlib.sha256(p.read_bytes()).hexdigest()) for p in sources],
        outputs={n: hashlib.sha256((out / n).read_bytes()).hexdigest() for n in outputs}), indent=2) + '\n')


if __name__ == '__main__':
    main()
