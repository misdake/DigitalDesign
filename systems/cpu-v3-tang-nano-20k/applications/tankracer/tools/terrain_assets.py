"""Build page-local collision assets directly from the reference FASTF101 archive."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
from terrain_map_pack import pack_map


def generate(reference, archive, cells, output, whole_map=False):
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
    subprocess.run([str(executable), str(archive), 'all' if whole_map else ','.join(map(str, cells)), str(output)],
                   check=True, timeout=60)
    packed = pack_map((output / 'map-cells.bin').read_bytes(), output) if whole_map else None
    values = list(map(int, (output / 'map.txt').read_text().split()))
    resources = []
    declarations = ['use crate::dsl_rt::*;\nmod static_assets; use static_assets::AssetBytes;\n']
    specifications = [
        ('TERRAIN_PAGE_A', f'cell-{cells[0]}.bin', 32768),
        ('TERRAIN_PAGE_B', f'cell-{cells[1]}.bin', 32768),
        ('TERRAIN_QUERIES', 'queries.bin', 128),
    ]
    if whole_map:
        specifications = [('TERRAIN_MAP_CELLS', 'cells.bin', 32768),
                          ('TERRAIN_MAP_DIRECTORY', 'directory.bin', 128),
                          ('TERRAIN_QUERIES', 'queries.bin', 128)]
    for symbol, name, alignment in specifications:
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
    declarations.append(f'pub const TERRAIN_FULL_MAP:bool={str(whole_map).lower()};\n')
    if whole_map:
        declarations.append('pub const TERRAIN_PAGE_A:AssetBytes=TERRAIN_MAP_CELLS;\n'
                            'pub const TERRAIN_PAGE_B:AssetBytes=TERRAIN_MAP_CELLS;\n')
    else:
        declarations.append('pub const TERRAIN_MAP_CELLS:AssetBytes=TERRAIN_PAGE_A;\n'
                            'pub const TERRAIN_MAP_DIRECTORY:AssetBytes=TERRAIN_QUERIES;\n')
    (output / 'terrain_assets.rs').write_text(''.join(declarations))
    sources = [archive, source, Path(__file__).resolve(), owner / 'terrain_map_pack.py',
               reference / 'crates/assets/src/terrain.rs', reference / 'crates/assets/src/archive.rs',
               reference / 'crates/game/src/terrain.rs', reference / 'crates/game/src/geometry.rs',
               reference / 'crates/game/src/sampling.rs']
    sizes = [list(map(int, line.split(','))) for line in (output / 'cell-sizes.csv').read_text().splitlines()]
    manifest = dict(resources=resources, sector_relocations=[], cells=list(range(64)) if whole_map else cells,
                    terrain_map=packed,
                    queries=values[4], query_words=66, output_words=59,
                    page_bytes=32768, used_bytes={str(cell):dict(sizes)[cell] for cell in (range(64) if whole_map else cells)},
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
    manifest = generate(Path(reference).parent, Path(archive), [24, 32], output,
                        os.environ.get('TANKRACER_TERRAIN_COVERAGE', 'adjacent') == 'map')
    print(json.dumps({k: manifest[k] for k in ('cells', 'queries', 'used_bytes', 'largest_cell_bytes')}))


if __name__ == '__main__':
    main()
