"""Build page-local collision assets directly from the reference FASTF101 archive."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess


def generate(reference, archive, cells, output):
    owner = Path(__file__).resolve().parent
    project = output / 'reference-tool'
    project.mkdir(parents=True, exist_ok=True)
    source = owner / 'terrain_assets.rs'
    manifest = ('[package]\nname="tankracer-terrain-assets"\nversion="0.1.0"\nedition="2024"\n'
                '[workspace]\n[[bin]]\nname="terrain-assets"\npath=' + json.dumps(source.as_posix())
                + '\n[dependencies]\ntankracer-assets={path='
                + json.dumps((reference / 'crates/assets').as_posix()) + '}\n'
                + 'tankracer-game={path=' + json.dumps((reference / 'crates/game').as_posix()) + '}\n')
    (project / 'Cargo.toml').write_text(manifest)
    with (project / 'build.log').open('w') as log:
        subprocess.run(['cargo', 'build', '--release', '--offline', '--manifest-path',
                        str(project / 'Cargo.toml'), '--target-dir', str(project / 'target')],
                       check=True, stdout=log, stderr=subprocess.STDOUT, timeout=300)
    executable = project / 'target/release' / ('terrain-assets.exe' if os.name == 'nt' else 'terrain-assets')
    subprocess.run([str(executable), str(archive), ','.join(map(str, cells)), str(output)],
                   check=True, timeout=60)
    values = list(map(int, (output / 'map.txt').read_text().split()))
    resources = []
    declarations = ['use crate::dsl_rt::*;\nmod static_assets; use static_assets::AssetBytes;\n']
    for symbol, name, alignment in [
        ('TERRAIN_PAGE_A', f'cell-{cells[0]}.bin', 32768),
        ('TERRAIN_PAGE_B', f'cell-{cells[1]}.bin', 32768),
        ('TERRAIN_QUERIES', 'queries.bin', 128),
    ]:
        path = output / name
        payload = path.read_bytes()
        declarations.append(f'#[asset(align={alignment})]\npub const {symbol}:AssetBytes='
                            f'AssetBytes::new(include_bytes!({json.dumps(path.as_posix())}));\n')
        resources.append(dict(file=name, placement='source', symbol=symbol, bytes=len(payload),
                              sha256=hashlib.sha256(payload).hexdigest()))
    if not 0 < values[4] <= 1200 or values[4] * 59 > 65535:
        raise ValueError('First-slice query count exceeds the target check counter')
    declarations.append(f'pub const TERRAIN_CELL_A:u16={cells[0]};\n'
                        f'pub const TERRAIN_CELL_B:u16={cells[1]};\n'
                        f'pub const TERRAIN_QUERY_COUNT:u16={values[4]};\n')
    (output / 'terrain_assets.rs').write_text(''.join(declarations))
    sources = [archive, source, Path(__file__).resolve(),
               reference / 'crates/assets/src/terrain.rs', reference / 'crates/assets/src/archive.rs',
               reference / 'crates/game/src/terrain.rs', reference / 'crates/game/src/geometry.rs',
               reference / 'crates/game/src/sampling.rs']
    sizes = [list(map(int, line.split(','))) for line in (output / 'cell-sizes.csv').read_text().splitlines()]
    manifest = dict(resources=resources, sector_relocations=[], cells=cells,
                    queries=values[4], query_words=66, output_words=59,
                    page_bytes=32768, used_bytes={str(cell):dict(sizes)[cell] for cell in cells},
                    largest_cell_bytes=max(size for _, size in sizes),
                    sources=[dict(file=str(p), sha256=hashlib.sha256(p.read_bytes()).hexdigest()) for p in sources])
    (output / 'assets.json').write_text(json.dumps(manifest, indent=2) + '\n')
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    reference = Path(os.environ['TANKRACER_REFERENCE']).resolve().as_posix().removeprefix('//?/')
    archive = Path(os.environ['TANKRACER_TERRAIN_ARCHIVE']).resolve().as_posix().removeprefix('//?/')
    manifest = generate(Path(reference).parent, Path(archive), [24, 32], output)
    print(json.dumps({k: manifest[k] for k in ('cells', 'queries', 'used_bytes', 'largest_cell_bytes')}))


if __name__ == '__main__':
    main()
