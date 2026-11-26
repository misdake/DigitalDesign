"""Export original projectile calls, complete live state and exact lookup assets."""
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
    game = prepare(root,out,driving=True,lifecycle=True)
    project = out / 'reference-tool'
    project.mkdir(exist_ok=True)
    manifest = ('[package]\nname="tankracer-projectile-assets"\nversion="0.1.0"\nedition="2024"\n'
                '[workspace]\n[[bin]]\nname="projectile-oracle"\npath='
                + json.dumps((owner / 'projectile_oracle.rs').as_posix())
                + '\n[dependencies]\nserde_json="=1.0.109"\ntankracer-game={path='
                + json.dumps(game.as_posix()) + '}\ntankracer-assets={path='
                + json.dumps((root / 'crates/assets').as_posix()) + '}\n')
    (project / 'Cargo.toml').write_text(manifest)
    with (project / 'build.log').open('w') as log:
        subprocess.run(['cargo', 'build', '--release', '--offline', '--manifest-path',
                        str(project / 'Cargo.toml'), '--target-dir', str(project / 'target')],
                       check=True, timeout=300, stdout=log, stderr=subprocess.STDOUT)
    executable = project / 'target/release' / ('projectile-oracle.exe' if os.name == 'nt' else 'projectile-oracle')
    subprocess.run([str(executable), str(root), str(out)], check=True, timeout=120)
    subprocess.run([sys.executable,str(owner/'export_trig_assets.py'),'--reference',str(root),
        '--output',str(out)],check=True,timeout=60,stdout=subprocess.DEVNULL)
    sources = [owner / n for n in ['projectile_oracle.rs', 'mine_oracle.rs', 'tank_hit_oracle.rs', 'temporary_oracle.rs', 'scene_allocation_oracle.rs', 'scene_creation_oracle.rs', 'reference_scene.rs', 'identity_assets.rs', 'scene_export.rs', 'scene_lifecycle_assets.rs', 'reference_world.rs', 'terrain_assets.rs', 'projectile_assets.py', 'export_trig_assets.py', 'reference_bridge.py', 'reference_car.rs', 'reference_tanks.rs']]
    overlay=json.loads((out/'reference-overlay.json').read_text())
    sources += [Path(row['file']) for row in overlay['sources']]
    sources += sorted((root / 'crates/game/src').rglob('*.rs'))
    sources += sorted((root / 'crates/assets/src').rglob('*.rs'))
    sources += [root / 'crates' / crate / 'Cargo.toml' for crate in ['game', 'assets']]
    sources += [root / 'crates/game/data' / n for n in ['sine.bin', 'angle.bin']]
    sources += [root / 'assets/game' / n / 'FASTFILE.DAT' for n in ['DATA', 'INGAME',
                'TANKS/BRONZE', 'TANKS/SILVER', 'TANKS/GOLD', 'LEVELS/ENGLAND']]
    outputs = ['scene-allocation.json'] + [f'scene-allocation-{i}.bin' for i in range(7)] + ['projectile.json', 'mine.json', 'tank-hit.json', 'temporary.json', 'projectile-cells.bin', 'sine.i16.bin', 'angle.i16.bin']
    outputs += ['scene-creation.json'] + [f'scene-creation-{i}.bin' for i in range(16)]
    sources += [owner / 'scene_mines_oracle.rs']
    sources += [owner / 'scene_mine_tick_oracle.rs']
    sources += [owner / 'explosion_oracle.rs']
    sources += [owner / 'explosion_blasts_oracle.rs']
    sources += [owner / 'scene_explosions_oracle.rs']
    sources += [owner / 'scene_explosion_tick_oracle.rs']
    sources += [owner / 'scene_weapon_explosion_oracle.rs']
    sources += [owner / 'scene_mine_explosion_oracle.rs']
    sources += [owner / 'scene_mine_hit_oracle.rs']
    sources += [owner / 'projectile_marks_oracle.rs']
    sources += [owner / 'scene_projectile_flight_oracle.rs']
    sources += [owner / 'scene_projectile_area_oracle.rs']
    sources += [owner / 'scene_projectile_direct_oracle.rs']
    outputs += ['scene-mines.json'] + [f'scene-mines-{i}.bin' for i in range(10)]
    outputs += ['scene-mine-tick.json'] + [f'scene-mine-tick-{i}.bin' for i in range(20)]
    outputs += ['explosion.json']
    outputs += ['explosion-blasts.json']
    outputs += ['scene-explosions.json'] + [f'scene-explosions-{i}.bin' for i in range(16)]
    outputs += ['scene-explosion-tick.json'] + [f'scene-explosion-tick-{i}.bin' for i in range(26)]
    outputs += ['scene-weapon-explosion.json'] + [f'scene-weapon-explosion-{i}.bin' for i in range(23)]
    outputs += ['scene-mine-explosion.json'] + [f'scene-mine-explosion-{i}.bin' for i in range(15)]
    outputs += ['projectile-marks.json']
    outputs += ['scene-projectile-flight.json'] + [f'scene-projectile-flight-{i}.bin' for i in range(33)]
    outputs += ['scene-projectile-direct.json'] + [f'scene-projectile-direct-{i}.bin' for i in range(31)]
    outputs += ['scene-projectile-area.json'] + [f'scene-projectile-area-{i}.bin' for i in range(28)]
    outputs += ['scene-mine-hit.json'] + [f'scene-mine-hit-{i}.bin' for i in range(29)]
    (out / 'projectile-sources.json').write_text(json.dumps(dict(
        sources=[dict(file=str(p), sha256=hashlib.sha256(p.read_bytes()).hexdigest()) for p in sources],
        outputs={n: hashlib.sha256((out / n).read_bytes()).hexdigest() for n in outputs}), indent=2) + '\n')


if __name__ == '__main__':
    main()
