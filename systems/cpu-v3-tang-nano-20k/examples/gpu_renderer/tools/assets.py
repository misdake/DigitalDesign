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
from gpu_layout import validate_asset_range

MESH, TEXTURE, ATLAS = 0x4b0000, 0x4b1000, 0x4c0000


def pack(output):
    output.mkdir(parents=True, exist_ok=True)
    # 3 vertices, one two-sided triangle, uniform20; unsigned delta10/F16 decode.
    mesh = bytearray(384)
    struct.pack_into("<2H", mesh, 0, (MESH + 128) // 128, 1)
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
    texture = struct.pack("<64H", *([65535] * 64))
    atlas = bytearray(65536)
    font = SYSTEM / "examples/gpu_demo/assets/font-8x16.txt"
    rows = {}
    for line in font.read_text().splitlines():
        if line and not line.startswith("#"):
            values = line.split()
            rows[int(values[0])] = [int(x, 16) for x in values[1:]]
    # New semantic OBS label, without changing the classic demo font.
    for glyph, small in {79: [60, 102, 102, 102, 102, 102, 60, 0],
                         66: [124, 102, 102, 124, 102, 102, 124, 0]}.items():
        rows[glyph] = [v for v in small for _ in range(2)]
    assert set(b"OBS --.0123456789") <= rows.keys()
    for glyph, bitmap in rows.items():
        assert 0 <= glyph < 255 and len(bitmap) == 16
        for y, bits in enumerate(bitmap):
            for x in range(8):
                at = glyph * 256 + (y * 8 + (x ^ ((y & 1) * 2))) * 2
                struct.pack_into("<H", atlas, at, 65535 if bits & (128 >> x) else 0)
    assert atlas[255 * 256:] == bytes(256)
    resources = [("mesh.bin", MESH, mesh), ("texture.bin", TEXTURE, texture),
                 ("atlas.bin", ATLAS, atlas)]
    for name, address, payload in resources:
        assert address % 128 == 0 and len(payload) % 128 == 0
        validate_asset_range(address, len(payload))
        (output / name).write_bytes(payload)
    constants = dict(GPU_RENDERER_MESH_DIRECTORY=MESH // 128,
                     GPU_RENDERER_MESH_DIRECTORY_WORD=0, GPU_RENDERER_MESH_COUNT=1,
                     GPU_RENDERER_MESH_FIRST=MESH // 128, GPU_RENDERER_MESH_SECTORS=3,
                     GPU_RENDERER_TEXTURE=TEXTURE // 128, GPU_RENDERER_TEXTURE_SECTORS=1,
                     GPU_RENDERER_TEXTURE_LOG2=0, GPU_RENDERER_TEXTURE_MIP=0)
    for axis, minimum, maximum in [("X", -65536, 65536), ("Y", -65536, 65536), ("Z", 0, 0)]:
        for label, value in [("MIN", minimum), ("MAX", maximum)]:
            constants[f"GPU_RENDERER_{label}_{axis}_LOW"] = value & 65535
            constants[f"GPU_RENDERER_{label}_{axis}_HIGH"] = (value >> 16) & 65535
    (output / "renderer_assets.rs").write_text("".join(
        f"pub const {name}:u16={value};\n" for name, value in constants.items()))
    manifest = dict(resources=[dict(file=n, address=a, bytes=len(p),
                    sha256=hashlib.sha256(p).hexdigest()) for n, a, p in resources],
                    triangles=1, vertices=3, bounds=[[-1, -1, 0], [1, 1, 0]],
                    glyphs=sorted(rows), sources={str(p.relative_to(SYSTEM)): hashlib.sha256(p.read_bytes()).hexdigest()
                    for p in [Path(__file__).resolve(), font, SYSTEM / "tools/gpu_layout.py", SYSTEM / "rcc/gpu_layout.rs"]})
    (output / "assets.json").write_text(json.dumps(manifest, indent=2) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    pack(parser.parse_args().output)
