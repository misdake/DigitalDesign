"""Use the canonical minimal renderer packer; no parallel binary asset encoder."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import sys
import shutil

sys.dont_write_bytecode = True
SOURCE = Path(__file__).resolve().parents[2] / "gpu_renderer/tools/assets.py"
sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'tools/gpu_profiler'))
from identity import load_build, build_layout, digest
from layout import ROW, COLUMN, COLUMN_MAP, declared_layout


def copy_column_catalog(source, output):
    """Consume a CPU-produced catalog; never reinterpret/repack row textures."""
    source, output = Path(source).resolve(), Path(output).resolve()
    manifest = json.loads((source/'assets.json').read_text())
    if declared_layout(manifest.get('capture_layout')) != COLUMN:
        raise ValueError('Column diagnostic needs an explicitly column-packed CPU asset catalog')
    names = {'assets.json', 'renderer_assets.rs'}
    lo, hi = COLUMN_MAP['assets']
    for resource in manifest['resources']:
        name = resource['file']
        if Path(name).name != name or resource.get('kind') not in ('mesh', 'texture', 'atlas', 'other'):
            raise ValueError('Unsafe or untyped column asset entry')
        if not lo <= resource['address'] < resource['address']+resource['bytes'] <= hi:
            raise ValueError('Column asset escapes its declared allocation')
        if resource['kind']=='texture' and resource.get('layout') != COLUMN.texture_layout:
            raise ValueError('Ordinary texture asset does not use column mip packing')
        path=source/name
        if path.stat().st_size != resource['bytes'] or digest(path) != resource['sha256']:
            raise ValueError('Column catalog bytes/hash mismatch: '+name)
        names.add(name)
    if 'GPU_PROFILER_AUTOMATIC_TEST' in (source/'renderer_assets.rs').read_text():
        raise ValueError('Expected a producer catalog, not a previous diagnostic output')
    output.mkdir(parents=True, exist_ok=True)
    for name in sorted(names):
        if source/name != output/name: shutil.copyfile(source/name, output/name)


def pack(output, automatic_test=None, full_scene=None, layout=ROW, column_catalog=None):
    declared = None
    declared_full = None
    if os.environ.get("GPU_PROFILER_MANIFEST"):
        value = load_build(os.environ["GPU_PROFILER_MANIFEST"])
        layout = build_layout(value)
        declared = value.get("options", {}).get("automatic_test", False)
        declared_full = value.get("options", {}).get("full_scene", False)
        if type(declared) is not bool or type(declared_full) is not bool:
            raise ValueError("automatic_test/full_scene must be explicit booleans")
    if automatic_test is None:
        automatic_test = declared is True
    if declared is not None and automatic_test != declared:
        raise ValueError("Asset constants disagree with the declared diagnostic stimulus")
    if full_scene is None:
        full_scene = declared_full is True
    if declared_full is not None and full_scene != declared_full:
        raise ValueError("Asset geometry disagrees with the declared scene")
    if layout == COLUMN:
        if declared is not None:
            # Firmware rebuilds materialize the same already-bound producer
            # output. No extra external path or runtime CP/DMA authority.
            output.mkdir(parents=True, exist_ok=True)
            manifest_path=Path(os.environ['GPU_PROFILER_MANIFEST']).resolve()
            entries={e['name'][7:]:e for e in value['files'] if e['name'].startswith('assets/')}
            if not {'assets.json','renderer_assets.rs'} <= entries.keys():
                raise ValueError('Build identity does not bind the column asset catalog')
            for name, entry in entries.items():
                if Path(name).name != name: raise ValueError('Unsafe bound asset name')
                source=(manifest_path.parent/entry['path']).resolve()
                if source != (output/name).resolve(): shutil.copyfile(source, output/name)
            return
        if column_catalog is None:
            raise ValueError('Explicit CPU-produced column catalog is required')
        copy_column_catalog(column_catalog, output)
    elif column_catalog is not None:
        raise ValueError('Column assets cannot be attached to a row build')
    elif full_scene:
        from full_assets import pack as pack_full
        pack_full(output)
    else:
        spec = importlib.util.spec_from_file_location("profiler_renderer_assets", SOURCE)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        module.pack(output)
    with (output / "renderer_assets.rs").open("a", encoding="utf-8") as stream:
        stream.write(f"pub const GPU_PROFILER_AUTOMATIC_TEST:u16={int(automatic_test)};\n")
        stream.write(f"pub const GPU_PROFILER_FULL_SCENE:u16={int(full_scene)};\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--automatic-test", action="store_const", const=True, default=None)
    parser.add_argument("--full-scene", action="store_const", const=True, default=None)
    args = parser.parse_args()
    pack(args.output, args.automatic_test, args.full_scene)
