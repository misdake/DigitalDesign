"""Use the canonical minimal renderer packer; no parallel binary asset encoder."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import sys

sys.dont_write_bytecode = True
SOURCE = Path(__file__).resolve().parents[2] / "gpu_renderer/tools/assets.py"


def pack(output, automatic_test=None, full_scene=None):
    declared = None
    declared_full = None
    if os.environ.get("GPU_PROFILER_MANIFEST"):
        value = json.loads(Path(os.environ["GPU_PROFILER_MANIFEST"]).read_text(encoding="utf-8"))
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
    if full_scene:
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
