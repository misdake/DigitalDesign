"""Compose existing canonical scene/font packers into the renderer root contract."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile

SYSTEM = Path(__file__).resolve().parents[3]


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


def pack(output):
    demo = module("profiler_full_demo_assets", SYSTEM / "examples/gpu_demo/tools/assets.py")
    demo.pack(output)
    minimal = module("profiler_full_font_assets", SYSTEM / "examples/gpu_renderer/tools/assets.py")
    # The minimal packer adds the OBS glyphs using the same native atlas encoder.
    with tempfile.TemporaryDirectory() as temp:
        minimal.pack(Path(temp))
        atlas = (Path(temp) / "atlas.bin").read_bytes()
        font_manifest = json.loads((Path(temp) / "assets.json").read_text())
    (output / "atlas.bin").write_bytes(atlas)
    manifest = json.loads((output / "assets.json").read_text())
    for resource in manifest["resources"]:
        if resource["file"] == "atlas.bin":
            resource["sha256"] = hashlib.sha256(atlas).hexdigest()
    manifest["atlas"]["glyphs"] = font_manifest["glyphs"]
    manifest["sources"].update(font_manifest["sources"])
    (output / "assets.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    scene = json.loads((SYSTEM / "examples/gpu_demo/assets/scene.json").read_text())
    points = [tuple(mesh["config"][axis] +
                    (((vertex[0] >> (axis * 10)) & 1023) << (mesh["config"][3] & 31))
                    for axis in range(3)) for mesh in scene["meshes"] for vertex in mesh["vertices"]]
    minimum = [min(p[axis] for p in points) for axis in range(3)]
    maximum = [max(p[axis] for p in points) for axis in range(3)]
    mesh, texture = manifest["mesh"], manifest["texture"]
    constants = dict(GPU_RENDERER_MESH_DIRECTORY=mesh["directory_sector"],
                     GPU_RENDERER_MESH_DIRECTORY_WORD=mesh["directory_word"],
                     GPU_RENDERER_MESH_COUNT=mesh["count"], GPU_RENDERER_MESH_FIRST=mesh["first_sector"],
                     GPU_RENDERER_MESH_SECTORS=mesh["sectors"], GPU_RENDERER_TEXTURE=texture["sector"],
                     GPU_RENDERER_TEXTURE_SECTORS=texture["sectors"],
                     GPU_RENDERER_TEXTURE_LOG2=texture["size_log2"], GPU_RENDERER_TEXTURE_MIP=texture["full_mip"])
    assert mesh["count"] == 24 and sum(len(m["triangles"]) for m in scene["meshes"]) == 1141
    assert sum(len(m["triangles"]) for m in scene["meshes"][7:]) == 948
    for axis, lo, hi in zip("XYZ", minimum, maximum):
        for label, value in [("MIN", lo), ("MAX", hi)]:
            constants[f"GPU_RENDERER_{label}_{axis}_LOW"] = value & 65535
            constants[f"GPU_RENDERER_{label}_{axis}_HIGH"] = (value >> 16) & 65535
    (output / "renderer_assets.rs").write_text("".join(
        f"pub const {name}:u16={value};\n" for name, value in constants.items()), encoding="utf-8")
