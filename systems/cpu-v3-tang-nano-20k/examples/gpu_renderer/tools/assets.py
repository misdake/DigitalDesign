"""Pack the minimal renderer's immutable native triangle, texture and atlas."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import sys

sys.dont_write_bytecode = True
SYSTEM = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(SYSTEM / "tools"))
from gpu_layout import validate_asset_range, sectors
from capture_layout import description
import ui_font

def pack(output):
    bounds=sectors()
    mesh_base=bounds['GPU_ASSET_FIRST_SECTOR']*128
    texture_base=mesh_base+4096
    atlas_base=mesh_base+65536
    texture_sectors=bounds['GPU_TEXTURE_MIN_LEVEL_SECTORS']
    output.mkdir(parents=True, exist_ok=True)
    # 3 vertices, one two-sided triangle, uniform20; unsigned delta10/F16 decode.
    mesh = bytearray(384)
    struct.pack_into("<2H", mesh, 0, (mesh_base + 128) // 128, 1)
    struct.pack_into("<4Q", mesh, 128, 3 | (1 << 8) | (20 << 16) | (5 << 24),
                     0, 0xffff0000 | (0xffff0000 << 32), 8 << 32)
    points = [(0, 0), (512, 0), (256, 512)]
    for i, (x, y) in enumerate(points):
        assert 0 <= x < 1024 and 0 <= y < 1024
        struct.pack_into("<3I", mesh, 256 + i * 12, x | (y << 10), 511 << 20, 0)
    struct.pack_into("<Q", mesh, 256 + 5 * 8, 0 | (1 << 6) | (2 << 12))
    # Independent decode of every packed vertex, index and origin/shift.
    q0, q1, origins, q3 = struct.unpack_from("<4Q", mesh, 128)
    assert q0 & 127 == 3 and (q0 >> 8) & 127 == 1 and not q0 >> 31
    assert (q0 >> 16) & 63 == 20 and q1 == 0
    assert origins == 0xffff0000 | (0xffff0000 << 32) and q3 == 8 << 32
    decoded = []
    for i in range(3):
        pos, normal, uv = struct.unpack_from("<3I", mesh, 256 + i * 12)
        decoded.append((-65536 + (pos & 1023) * 256,
                        -65536 + ((pos >> 10) & 1023) * 256, pos >> 20))
        assert normal == 511 << 20 and uv == 0
    assert decoded == [(-65536, -65536, 0), (65536, -65536, 0), (0, 65536, 0)]
    assert struct.unpack_from("<Q", mesh, 296)[0] == 0 | (1 << 6) | (2 << 12)
    assert mesh[304:] == bytes(80)
    texture = struct.pack("<H", 65535) * (texture_sectors*64)
    rows = ui_font.load_rows()
    atlas = ui_font.atlas(rows)
    resources = [("mesh.bin", mesh_base, mesh), ("texture.bin", texture_base, texture),
                 ("atlas.bin", atlas_base, atlas)]
    for name, address, payload in resources:
        assert address % 128 == 0 and len(payload) % 128 == 0
        validate_asset_range(address, len(payload))
        (output / name).write_bytes(payload)
    constants = dict(GPU_RENDERER_MESH_DIRECTORY=mesh_base // 128,
                     GPU_RENDERER_MESH_DIRECTORY_WORD=0, GPU_RENDERER_MESH_COUNT=1,
                     GPU_RENDERER_MESH_FIRST=mesh_base // 128, GPU_RENDERER_MESH_SECTORS=3,
                     GPU_RENDERER_TEXTURE=texture_base // 128, GPU_RENDERER_TEXTURE_SECTORS=texture_sectors,
                     GPU_RENDERER_TEXTURE_LOG2=0, GPU_RENDERER_TEXTURE_MIP=0)
    for axis, minimum, maximum in [("X", -65536, 65536), ("Y", -65536, 65536), ("Z", 0, 0)]:
        for label, value in [("MIN", minimum), ("MAX", maximum)]:
            constants[f"GPU_RENDERER_{label}_{axis}_LOW"] = value & 65535
            constants[f"GPU_RENDERER_{label}_{axis}_HIGH"] = (value >> 16) & 65535
    (output / "renderer_assets.rs").write_text("".join(
        f"pub const {name}:u16={value};\n" for name, value in constants.items()))
    manifest = dict(source_root='repository', capture_layout=description(),resources=[dict(file=n, address=a, bytes=len(p),
                    kind={"mesh.bin":"mesh","texture.bin":"texture","atlas.bin":"atlas"}[n],
                    **({"layout":description()["texture_layout"]} if n=="texture.bin" else {}),
                    sha256=hashlib.sha256(p).hexdigest()) for n, a, p in resources],
                    triangles=1, vertices=3, bounds=[[-1, -1, 0], [1, 1, 0]],
                    glyphs=sorted(rows), sources={p.relative_to(SYSTEM.parents[1]).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
                    for p in [Path(__file__).resolve(), *ui_font.SOURCES, SYSTEM / "tools/gpu_layout.py", SYSTEM / "rcc/gpu_layout.rs", SYSTEM / "tools/capture_layout.py", SYSTEM / "rcc/gpu_capture_abi.rs"]})
    (output / "assets.json").write_text(json.dumps(manifest, indent=2) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    args=parser.parse_args()
    pack(args.output)
