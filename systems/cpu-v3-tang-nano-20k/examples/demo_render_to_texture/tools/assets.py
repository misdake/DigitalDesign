"""Two meshlets and offline-packed assets for the explicit macro-column RTT profile."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import sys

sys.dont_write_bytecode = True
SYSTEM = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(SYSTEM / "tools"))
from gpu_layout import validate_asset_range, sectors


def pack(output):
    path = SYSTEM / "applications/rendering/tools/minimal_assets.py"
    spec = importlib.util.spec_from_file_location("renderer_rtt_base_assets", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    module.pack(output)
    mesh_base = sectors()["GPU_ASSET_FIRST_SECTOR"] * 128
    mesh = bytearray(640)
    struct.pack_into("<4H", mesh, 0, (mesh_base+128)//128, 1, (mesh_base+384)//128, 1)
    for record, points, attributes, rgb, triangles in [
        (128, [(0, 0), (512, 0), (256, 512)], [0x0000ff, 0x00ff00, 0xff0000], True, [(0, 1, 2)]),
        (384, [(0, 0), (512, 0), (512, 512), (0, 512)],
         [0xffffffff, 0xffff0000, 0, 0x0000ffff], False, [(0, 1, 2), (0, 2, 3)]),
    ]:
        triangle_qword = (len(points) * 12 + 7) // 8
        header = len(points) | (len(triangles) << 8) | (20 << 16) | (triangle_qword << 24)
        struct.pack_into("<4Q", mesh, record, header, int(rgb) << 4,
                         0xffff0000 | (0xffff0000 << 32), 8 << 32)
        for i, ((x, y), attribute) in enumerate(zip(points, attributes)):
            struct.pack_into("<3I", mesh, record + 128 + i * 12,
                             x | (y << 10), 511 << 20, attribute)
        indices = sum(sum(index << (6 * k) for k, index in enumerate(triangle)) << (18 * i)
                      for i, triangle in enumerate(triangles))
        struct.pack_into("<Q", mesh, record + 128 + triangle_qword * 8, indices)
    (output / "mesh.bin").write_bytes(mesh)
    constants = (output / "renderer_assets.rs").read_text()
    constants = constants.replace("GPU_RENDERER_MESH_COUNT:u16=1;", "GPU_RENDERER_MESH_COUNT:u16=2;")
    constants = constants.replace("GPU_RENDERER_MESH_SECTORS:u16=3;", "GPU_RENDERER_MESH_SECTORS:u16=5;")
    constants = constants.replace("GPU_RENDERER_TEXTURE_SECTORS:u16=1;", "GPU_RENDERER_TEXTURE_SECTORS:u16=4;")
    (output / "renderer_assets.rs").write_text(constants)
    manifest = json.loads((output / "assets.json").read_text())
    for resource in manifest["resources"]:
        data = (output / resource["file"]).read_bytes()
        validate_asset_range(resource["address"], len(data))
        first, last = resource["address"], resource["address"] + len(data)
        assert last <= 0x600000 or first >= 0x680000, "asset aliases borrowed RTT planes"
        resource.update(bytes=len(data), sha256=hashlib.sha256(data).hexdigest())
    manifest.update(profile="production", surface="column-major-16x16-pad16",
                    triangles=3, vertices=7,
                    borrowed=dict(color=0x600000, depth=0x640000, plane_capacity=262144))
    manifest["sources"].update({p.relative_to(SYSTEM.parents[1]).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
                                for p in [Path(__file__).resolve(), SYSTEM / "tools/gpu_layout.py"]})
    (output / "assets.json").write_text(json.dumps(manifest, indent=2) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    pack(parser.parse_args().output)
