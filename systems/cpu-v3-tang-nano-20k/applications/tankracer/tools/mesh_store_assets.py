"""Export original stable mesh-resource operations and complete geometry."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
from reference_bridge import prepare
from reference_player import append_player_probe


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reference', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--player', action='store_true')
    args = parser.parse_args()
    root = Path(args.reference.resolve().parent.as_posix().removeprefix('//?/'))
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    owner = Path(__file__).resolve().parent
    game = prepare(root, out, meshes=True, driving=True, lifecycle=True)
    append_player_probe(root, out, game)
    project = out / 'reference-tool'
    project.mkdir(exist_ok=True)
    manifest = ('[package]\nname="tankracer-mesh-store-assets"\nversion="0.1.0"\nedition="2024"\n'
                '[workspace]\n[[bin]]\nname="mesh-store-oracle"\npath='
                + json.dumps((owner / 'mesh_store_oracle.rs').as_posix())
                + '\n[dependencies]\nserde_json="=1.0.109"\ntankracer-game={path='
                + json.dumps(game.as_posix()) + '}\ntankracer-assets={path='
                + json.dumps((root / 'crates/assets').as_posix()) + '}\n')
    (project / 'Cargo.toml').write_text(manifest)
    with (project / 'build.log').open('w') as log:
        subprocess.run(['cargo', 'build', '--release', '--offline', '--manifest-path', str(project / 'Cargo.toml'),
                        '--target-dir', str(project / 'target')], check=True, timeout=300,
                       stdout=log, stderr=subprocess.STDOUT)
    exe = project / 'target/release' / ('mesh-store-oracle.exe' if os.name == 'nt' else 'mesh-store-oracle')
    subprocess.run([str(exe), str(out), str(root)] + (['player'] if args.player else []), check=True, timeout=300)
    sources = [owner / n for n in ['mesh_store_oracle.rs', 'mesh_store_snapshot.rs', 'mesh_catalog_assets.rs', 'reference_world.rs', 'reference_bridge.py', 'reference_meshes.rs', 'player_scene_assets.rs', 'projectile_snapshot.rs', 'reference_player.py', 'reference_player.rs', 'reference_scenery_player.rs']]
    sources.append(Path(__file__).resolve())
    overlay = json.loads((out / 'reference-overlay.json').read_text())
    sources += [Path(s['file']) for s in overlay['sources']]
    sources += [root / 'assets/game' / n / 'FASTFILE.DAT' for n in ['DATA','INGAME','TANKS/BRONZE','TANKS/SILVER','TANKS/GOLD','LEVELS/ENGLAND']]
    (out / 'mesh-store-sources.json').write_text(json.dumps(dict(
        sources=[dict(file=str(p), sha256=hashlib.sha256(p.read_bytes()).hexdigest()) for p in sources],
        outputs={name: hashlib.sha256((out / name).read_bytes()).hexdigest() for name in ['mesh-store.json', 'mesh-catalog.bin', 'mesh-catalog-controls.bin', 'mesh-catalog-expected.bin', 'mesh-catalog.json']}), indent=2) + '\n')


if __name__ == '__main__':
    main()
