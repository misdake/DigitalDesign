"""Compose canonical complete-scene assets into typed CPU capture catalogs."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import tempfile

sys.dont_write_bytecode = True
SYSTEM = Path(__file__).resolve().parents[1]
ROOT = SYSTEM.parents[1]
sys.path.insert(0, str(SYSTEM / 'tools'))
from capture_layout import description


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def pack(output, profile):
    fixture = SYSTEM / 'tests/fixtures/capture_full_scene'
    origin = json.loads((fixture / 'origin.json').read_text())
    if hashlib.sha256((fixture / 'full_scene.rs').read_bytes()).hexdigest() != origin['sha256']:
        raise ValueError('Formal full-scene source snapshot differs from its pinned origin')
    demo = module('capture_full_demo', SYSTEM / 'examples/gpu_demo/tools/assets.py')
    demo.pack(output, profile)
    minimal = module('capture_full_font', SYSTEM / 'examples/gpu_renderer/tools/assets.py')
    with tempfile.TemporaryDirectory(dir=output) as temp:
        minimal.pack(Path(temp), profile)
        atlas = (Path(temp) / 'atlas.bin').read_bytes()
        font_manifest = json.loads((Path(temp) / 'assets.json').read_text())
    (output / 'atlas.bin').write_bytes(atlas)
    catalog = json.loads((output / 'assets.json').read_text())
    catalog['atlas']['glyphs'] = font_manifest['glyphs']
    for resource in catalog['resources']:
        resource['kind'] = {'room.bin': 'mesh', 'texture.bin': 'texture', 'atlas.bin': 'atlas'}[resource['file']]
        if resource['kind'] == 'texture':
            resource['layout'] = description(profile)['texture_layout']
        resource['sha256'] = hashlib.sha256((output / resource['file']).read_bytes()).hexdigest()
    catalog['capture_layout'] = description(profile)
    # Normalize both canonical packers' source names to repository-relative paths.
    sources = dict(catalog['sources'])
    sources.update({(SYSTEM / name).relative_to(ROOT).as_posix(): sha
                    for name, sha in font_manifest['sources'].items()})
    source_paths = [Path(__file__), SYSTEM / 'tests/capture_draw.rs',
                    *sorted((SYSTEM / 'tests/fixtures/capture_full_scene').rglob('*'))]
    for path in source_paths:
        if path.is_file():
            sources[path.relative_to(ROOT).as_posix()] = hashlib.sha256(path.read_bytes()).hexdigest()
    catalog.update(source_root='repository', sources=sources, application_origin=origin,
                   capture_scene='canonical-full-scene')
    scene = json.loads((SYSTEM / 'examples/gpu_demo/assets/scene.json').read_text())
    meshes = scene['meshes']
    assert len(meshes) == 24 and sum(len(m['triangles']) for m in meshes) == 1141
    assert sum(len(m['triangles']) for m in meshes[7:]) == 948
    points = [tuple(m['config'][axis] + (((v[0] >> (axis * 10)) & 1023) << (m['config'][3] & 31))
                    for axis in range(3)) for m in meshes for v in m['vertices']]
    mesh, texture = catalog['mesh'], catalog['texture']
    constants = dict(GPU_RENDERER_MESH_DIRECTORY=mesh['directory_sector'],
                     GPU_RENDERER_MESH_DIRECTORY_WORD=mesh['directory_word'],
                     GPU_RENDERER_MESH_COUNT=mesh['count'], GPU_RENDERER_MESH_FIRST=mesh['first_sector'],
                     GPU_RENDERER_MESH_SECTORS=mesh['sectors'], GPU_RENDERER_TEXTURE=texture['sector'],
                     GPU_RENDERER_TEXTURE_SECTORS=texture['sectors'], GPU_RENDERER_TEXTURE_LOG2=texture['size_log2'],
                     GPU_RENDERER_TEXTURE_MIP=texture['full_mip'])
    for axis, index in zip('XYZ', range(3)):
        for label, value in [('MIN', min(p[index] for p in points)), ('MAX', max(p[index] for p in points))]:
            constants[f'GPU_RENDERER_{label}_{axis}_LOW'] = value & 65535
            constants[f'GPU_RENDERER_{label}_{axis}_HIGH'] = (value >> 16) & 65535
    (output / 'renderer_assets.rs').write_text(''.join(f'pub const {key}:u16={value};\n' for key, value in constants.items()))
    (output / 'assets.json').write_text(json.dumps(catalog, indent=2) + '\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--profile', choices=['default', 'column16'], required=True)
    args = parser.parse_args()
    pack(args.output.resolve(), args.profile)
