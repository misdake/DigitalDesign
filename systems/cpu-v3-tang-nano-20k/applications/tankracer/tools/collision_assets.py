"""Export pure collision/body fixtures from a caller-supplied reference checkout."""
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
    parser.add_argument('--car-runtime', action='store_true', help='also call the original private car entry')
    args = parser.parse_args()
    root = Path(args.reference.resolve().parent.as_posix().removeprefix('//?/'))
    owner = Path(__file__).resolve().parent
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    for path in [root / 'Cargo.toml', root / 'crates/game/Cargo.toml', root / 'crates/assets/Cargo.toml', owner / 'collision_oracle.rs']:
        if not path.is_file():
            raise FileNotFoundError(path)
    project = out / 'reference-tool'
    project.mkdir(exist_ok=True)
    game = prepare(root, out, lifecycle=True) if args.car_runtime else root / 'crates/game'
    manifest = ('[package]\nname="tankracer-collision-assets"\nversion="0.1.0"\nedition="2024"\n'
                '[workspace]\n[[bin]]\nname="collision-oracle"\npath='
                + json.dumps((owner / 'collision_oracle.rs').as_posix())
                + '\n[dependencies]\nserde_json="=1.0.109"\ntankracer-game={path='
                + json.dumps(game.as_posix()) + '}\ntankracer-assets={path='
                + json.dumps((root / 'crates/assets').as_posix()) + '}\n')
    if args.car_runtime:
        manifest += '\n[[bin]]\nname="car-tank-oracle"\npath=' + json.dumps((owner / 'car_tank_oracle.rs').as_posix()) + '\n'
        manifest += '\n[[bin]]\nname="car-deform-oracle"\npath=' + json.dumps((owner / 'car_deform_oracle.rs').as_posix()) + '\n'
        manifest += '\n[[bin]]\nname="car-obstacle-oracle"\npath=' + json.dumps((owner / 'car_obstacle_oracle.rs').as_posix()) + '\n'
        manifest += '\n[[bin]]\nname="scene-lifecycle-oracle"\npath=' + json.dumps((owner / 'scene_lifecycle_oracle.rs').as_posix()) + '\n'
    (project / 'Cargo.toml').write_text(manifest)
    with (project / 'build.log').open('w') as log:
        subprocess.run(['cargo', 'build', '--release', '--offline', '--manifest-path', str(project / 'Cargo.toml'),
                        '--target-dir', str(project / 'target')], check=True, timeout=300, stdout=log, stderr=subprocess.STDOUT)
    exe = project / 'target/release' / ('collision-oracle.exe' if os.name == 'nt' else 'collision-oracle')
    subprocess.run([str(exe), str(out), str(root)], check=True, timeout=60)
    if args.car_runtime:
        exe = project / 'target/release' / ('car-tank-oracle.exe' if os.name == 'nt' else 'car-tank-oracle')
        subprocess.run([str(exe), str(out), str(root)], check=True, timeout=60)
        fixture = json.loads((out / 'collision.json').read_text())
        fixture['cases'].extend(json.loads((out / 'car-tanks.json').read_text()))
        exe = project / 'target/release' / ('car-deform-oracle.exe' if os.name == 'nt' else 'car-deform-oracle')
        subprocess.run([str(exe), str(out), str(root)], check=True, timeout=60)
        fixture['cases'].extend(json.loads((out / 'car-deforms.json').read_text()))
        exe = project / 'target/release' / ('car-obstacle-oracle.exe' if os.name == 'nt' else 'car-obstacle-oracle')
        subprocess.run([str(exe), str(out), str(root)], check=True, timeout=300)
        fixture['cases'].extend(json.loads((out / 'car-obstacles.json').read_text()))
        exe = project / 'target/release' / ('scene-lifecycle-oracle.exe' if os.name == 'nt' else 'scene-lifecycle-oracle')
        subprocess.run([str(exe), str(out), str(root)], check=True, timeout=300)
        fixture['cases'].extend(json.loads((out / 'scene-lifecycle.json').read_text()))
        fixture['scope'] += '; original live car tank/deformable/ordinary entries, class dispatch, Fall/Bounce ticks, ordered repeated car/scenery scheduling, Stop drain, aliases and committed failure prefixes'
        fixture['scope'] += '; ordinary scene destruction, component retirement and shared identity/list failure prefixes'
        fixture['scope'] += '; original deactivation and auxiliary-completion class dispatch with parent-first queues and ordered membership removal'
        (out / 'collision.json').write_text(json.dumps(fixture))
    subprocess.run([sys.executable, str(owner / 'export_trig_assets.py'), '--reference', str(root),
                    '--output', str(out)], check=True, timeout=60, stdout=subprocess.DEVNULL)
    for source, name in [('sine.i16.bin', 'sine.bin'), ('angle.i16.bin', 'angle.bin')]:
        (out / name).write_bytes((out / source).read_bytes())
    sources = [owner / name for name in ['collision_oracle.rs', 'ground_response_oracle.rs', 'tank_response_oracle.rs', 'lifecycle_oracle.rs', 'entities_oracle.rs', 'crush_oracle.rs', 'spatial_oracle.rs', 'falling_oracle.rs', 'blast_oracle.rs']]
    sources += [root / 'crates/game/src/entities.rs', root / 'crates/game/src/scene_entity.rs']
    sources.append(Path(__file__).resolve())
    sources += [root / 'crates/game/src' / name for name in ['collision.rs', 'body.rs', 'object_query.rs', 'deformation.rs', 'deformable_hit.rs', 'damage.rs', 'impact_particles.rs', 'effect_pool.rs', 'compat/effects.rs', 'ground_collision.rs', 'tank_collision.rs', 'lifecycle.rs', 'spatial.rs', 'crush.rs', 'fall.rs', 'scene_objects/falling.rs', 'scene_objects.rs', 'scene_loading.rs', 'world_startup.rs', 'entity_factory.rs', 'random.rs', 'vector.rs', 'geometry.rs', 'math.rs', 'fixed.rs']]
    for name in ['DATA', 'INGAME', 'TANKS/BRONZE', 'TANKS/SILVER', 'TANKS/GOLD', 'LEVELS/ENGLAND']:
        sources.append(root / 'assets/game' / name / 'FASTFILE.DAT')
    sources += [root / 'crates/assets/src/mesh.rs']
    sources.append(root / 'crates/game/src/blast.rs')
    sources.append(owner / 'scene_world_oracle.rs')
    sources.append(owner / 'impact_snapshot.rs')
    sources.append(owner / 'scene_contact_oracle.rs')
    sources.append(root / 'crates/game/src/obstacle_collision.rs')
    sources.append(owner / 'fall_hit_oracle.rs')
    sources.append(root / 'crates/game/src/scene_objects/generic_hit.rs')
    sources.append(root / 'crates/game/src/scene_objects/collision.rs')
    sources.append(owner / 'bounce_hit_oracle.rs')
    sources.append(root / 'crates/game/src/bounce.rs')
    sources.append(root / 'crates/game/src/scene_objects/bouncing.rs')
    sources.append(owner / 'bounce_tick_oracle.rs')
    sources.append(owner / 'terrain_assets.rs')
    sources.append(root / 'crates/assets/src/terrain.rs')
    sources.append(root / 'crates/game/src/terrain.rs')
    sources.append(root / 'crates/game/src/scene_objects/lifecycle.rs')
    if args.car_runtime:
        sources += [owner / name for name in ['reference_bridge.py', 'reference_tanks.rs', 'reference_world.rs', 'car_tank_oracle.rs', 'car_deform_oracle.rs', 'car_obstacle_oracle.rs', 'scene_export.rs', 'scenery_assets.rs']]
        sources += [root / 'crates/game/src/vehicle_runtime/car_objects.rs']
        sources += [owner / name for name in ['scene_lifecycle_oracle.rs', 'scene_lifecycle_assets.rs', 'identity_assets.rs', 'reference_scene.rs']]
        sources += [root / 'crates/game/src/scene_objects/allocation.rs', root / 'crates/game/src/compat/dispatch.rs']
    (out / 'collision-sources.json').write_text(json.dumps(dict(
        sources=[dict(file=str(p), sha256=hashlib.sha256(p.read_bytes()).hexdigest()) for p in sources],
        outputs={name: hashlib.sha256((out / name).read_bytes()).hexdigest() for name in ['collision.json', 'sine.bin', 'angle.bin', 'collision-cells.bin'] + (['car-deform-scene.bin'] if args.car_runtime else [])}), indent=2) + '\n')
    print(json.dumps(dict(cases=len(json.loads((out / 'collision.json').read_text())['cases']))))


if __name__ == '__main__':
    main()
