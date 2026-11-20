"""Expose the existing native classic assets through public demo root names."""
import argparse
import importlib.util
import json
import hashlib
from pathlib import Path
import sys

sys.dont_write_bytecode = True


def pack(output):
    output = Path(output)
    source = Path(__file__).with_name('assets.py')
    spec = importlib.util.spec_from_file_location('classic_native_assets', source)
    native = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(native)
    native.pack(output)
    manifest = json.loads((output / 'assets.json').read_text())
    mesh, texture = manifest['mesh'], manifest['texture']
    constants = dict(
        GPU_RENDERER_MESH_DIRECTORY=mesh['directory_sector'],
        GPU_RENDERER_MESH_DIRECTORY_WORD=mesh['directory_word'],
        GPU_RENDERER_MESH_COUNT=mesh['count'],
        GPU_RENDERER_MESH_FIRST=mesh['first_sector'],
        GPU_RENDERER_MESH_SECTORS=mesh['sectors'],
        GPU_RENDERER_TEXTURE=texture['sector'],
        GPU_RENDERER_TEXTURE_SECTORS=texture['sectors'],
        GPU_RENDERER_TEXTURE_LOG2=texture['size_log2'],
        GPU_RENDERER_TEXTURE_MIP=texture['full_mip'])
    repo = Path(__file__).resolve().parents[5]
    manifest['sources'][Path(__file__).resolve().relative_to(repo).as_posix()] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    (output / 'assets.json').write_text(json.dumps(manifest, indent=2) + '\n')
    (output / 'renderer_assets.rs').write_text(''.join(
        f'pub const {name}:u16={value};\n' for name, value in constants.items()))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--output', type=Path, required=True)
    pack(parser.parse_args().output)
