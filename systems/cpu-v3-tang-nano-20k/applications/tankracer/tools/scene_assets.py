"""Build initial scene collision assets from a caller-supplied reference checkout."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys


def main():
    root = Path(os.environ['TANKRACER_REFERENCE']).resolve().parent
    root = Path(root.as_posix().removeprefix('//?/'))
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    out = parser.parse_args().output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    owner = Path(__file__).resolve().parent
    for path in [root / 'Cargo.toml', root / 'crates/game/Cargo.toml', root / 'crates/assets/Cargo.toml']:
        if not path.is_file():
            raise FileNotFoundError(path)
    project = out / 'reference-tool'
    project.mkdir(exist_ok=True)
    manifest = ('[package]\nname="tankracer-scene-assets"\nversion="0.1.0"\nedition="2024"\n'
                '[workspace]\n[[bin]]\nname="scene-assets"\npath='
                + json.dumps((owner / 'scene_assets.rs').as_posix())
                + '\n[dependencies]\nserde_json="=1.0.109"\n'
                + 'tankracer-assets={path=' + json.dumps((root / 'crates/assets').as_posix()) + '}\n'
                + 'tankracer-game={path=' + json.dumps((root / 'crates/game').as_posix()) + '}\n')
    (project / 'Cargo.toml').write_text(manifest)
    with (project / 'build.log').open('w') as log:
        subprocess.run(['cargo', 'build', '--release', '--offline', '--manifest-path', str(project / 'Cargo.toml'),
                        '--target-dir', str(project / 'target')], check=True, stdout=log, stderr=subprocess.STDOUT, timeout=300)
    exe = project / 'target/release' / ('scene-assets.exe' if os.name == 'nt' else 'scene-assets')
    subprocess.run([str(exe), str(root), str(out)], check=True, timeout=60)
    data = json.loads((out / 'scene.json').read_text())['metadata']
    subprocess.run([sys.executable, str(owner / 'export_trig_assets.py'), '--reference', str(root),
                    '--output', str(out)], check=True, timeout=60, stdout=subprocess.DEVNULL)
    (out / 'trig_lookup.rs').write_bytes((owner.parent / 'rcc/trig_asset_lookup.rs').read_bytes())
    declarations = ['use crate::dsl_rt::*;mod static_assets;use static_assets::AssetBytes;\n']
    resources = []
    for symbol, name in [('DATA', 'scene.bin'), ('RECORDS', 'records.bin'), ('QUERIES', 'queries.bin'),
                         ('HIT_INITIAL', 'hit-initial.bin'), ('HITS', 'hits.bin'),
                         ('FALL_TICKS', 'fall-ticks.bin'), ('CELLS', 'cells.bin'),
                         ('RUNTIME_LISTS', 'runtime-lists.bin'),
                         ('BOUNCE_INITIAL', 'bounce-initial.bin'), ('BOUNCE_UPDATES', 'bounce-updates.bin'),
                         ('SINE', 'sine.i16.bin'), ('ANGLE', 'angle.i16.bin')]:
        path = out / name
        alignment = 32768 if symbol == 'CELLS' else 128
        declarations.append(f'#[asset(align={alignment})] pub static SCENE_{symbol}:AssetBytes='
                            f'AssetBytes::new(include_bytes!({json.dumps(path.as_posix())}));\n')
        resources.append(dict(file=name, placement='source', symbol='SCENE_' + symbol,
                              bytes=path.stat().st_size, sha256=hashlib.sha256(path.read_bytes()).hexdigest()))
    declarations.append(f'pub const SCENE_QUERY_COUNT:u16={data["query_count"]};\n')
    declarations.append(f'pub const SCENE_HIT_COUNT:u16={data["hit_count"]};\n')
    declarations.append(f'pub const SCENE_FALL_TICK_COUNT:u16={data["fall_tick_count"]};\n')
    declarations.append(f'pub const SCENE_BOUNCE_UPDATE_COUNT:u16={data["bounce_update_count"]};\n')
    (out / 'scene_assets.rs').write_text(''.join(declarations))
    sources = [Path(__file__).resolve(), owner / 'scene_assets.rs', owner / 'scene_export.rs', owner / 'scene_hits.rs', owner / 'scene_bounce.rs']
    sources += [owner / 'export_trig_assets.py', owner / 'terrain_assets.rs', owner.parent / 'rcc/trig_asset_lookup.rs',
                root / 'src/frame_math.c', root / 'src/frame_math_tables.h']
    sources += [root / 'crates/game/src' / name for name in ['obstacle_collision.rs', 'scene_objects/collision.rs',
                'scene_objects/generic_hit.rs', 'scene_objects/lifecycle.rs', 'scene_objects/falling.rs',
                'breakable.rs', 'fall.rs', 'lifecycle.rs', 'terrain.rs', 'spatial.rs']]
    sources.append(root / 'crates/assets/src/terrain.rs')
    sources += [root / 'crates/game/src' / name for name in ['bounce.rs', 'scene_objects/bouncing.rs']]
    sources += [root / 'crates/game/src' / name for name in ['world_startup.rs', 'scene_loading.rs', 'scene_startup.rs', 'obstacle_startup.rs', 'object_query.rs', 'collision.rs', 'math.rs', 'fixed.rs', 'random.rs']]
    for name in ['DATA', 'INGAME', 'TANKS/BRONZE', 'TANKS/SILVER', 'TANKS/GOLD', 'LEVELS/ENGLAND']:
        sources.append(root / 'assets/game' / name / 'FASTFILE.DAT')
    (out / 'assets.json').write_text(json.dumps(dict(resources=resources, sector_relocations=[], metadata=data,
        sources=[dict(file=str(p), sha256=hashlib.sha256(p.read_bytes()).hexdigest()) for p in sources]), indent=2) + '\n')


if __name__ == '__main__':
    main()
