"""Pack immutable native demo assets only; the CPU driver builds every CB."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import sys

sys.dont_write_bytecode = True
EXAMPLE = Path(__file__).resolve().parents[1]
ROOT = EXAMPLE.parents[3]
SCENE = ROOT / "ip/gpu-v2/examples/vliw_integration"
sys.path.insert(0, str(SCENE))
import material_scene
import solid_scene

ATLAS = 0x4B0000
MESH = 0x4C0000
TEXTURE = 0x4D0000


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def font():
    rows = {}
    atlas = bytearray(65536)
    for line in (EXAMPLE / "assets/font-8x16.txt").read_text().splitlines():
        if not line or line.startswith("#"):
            continue
        fields = line.split()
        glyph = int(fields[0])
        assert glyph not in rows and 0 <= glyph < 255 and len(fields) == 17
        rows[glyph] = [int(v, 16) for v in fields[1:]]
        for y, row in enumerate(rows[glyph]):
            assert 0 <= row <= 255
            for x in range(8):
                # UI source-bank ordering is static; alpha controls a full word.
                at = glyph * 256 + (y * 8 + (x ^ ((y & 1) * 2))) * 2
                struct.pack_into("<H", atlas, at, 65535 if row & (128 >> x) else 0)
    assert set(b"FRAME 0123456789-MS.") <= rows.keys()
    assert atlas[255 * 256:] == bytes(256)
    return bytes(atlas), rows


def native_record(mesh):
    vertices, triangles = mesh["vertices"], mesh["triangles"]
    assert 0 < len(vertices) <= 64 and 0 < len(triangles) <= 96
    offset = (3 * len(vertices) + 1) // 2
    payload_size = ((offset + (len(triangles) + 2) // 3) * 8 + 127) // 128 * 128
    assert 128 <= payload_size <= 1024
    record = bytearray(128 + payload_size)
    origin = mesh["config"]
    meta = [len(vertices) | (len(triangles) << 8) | (20 << 16) | (offset << 24) | (1 << 31),
            mesh.get("color_mode", 0) << 4, (origin[0] & 0xFFFFFFFF) | ((origin[1] & 0xFFFFFFFF) << 32),
            (origin[2] & 0xFFFFFFFF) | (origin[3] << 32)]
    struct.pack_into("<4Q", record, 0, *meta)
    for i, vertex in enumerate(vertices):
        assert vertex[0] < 1 << 30 and vertex[1] < 1 << 30
        struct.pack_into("<3I", record, 128 + i * 12, *vertex)
    for i, triangle in enumerate(triangles):
        assert len(triangle) == 3 and all(0 <= j < len(vertices) for j in triangle)
        at = 128 + (offset + i // 3) * 8
        value = struct.unpack_from("<Q", record, at)[0]
        value |= sum(index << (6 * lane) for lane, index in enumerate(triangle)) << (18 * (i % 3))
        struct.pack_into("<Q", record, at, value)
    return bytes(record)


def texture():
    levels = material_scene.texture_levels()
    result = bytearray()
    for exponent in range(7):
        size = 1 << exponent
        for ty in range(max(1, size // 8)):
            for tx in range(max(1, size // 8)):
                for y in range(8):
                    for x in range(8):
                        pixel = levels[exponent][((ty * 8 + y) % size) * size + (tx * 8 + x) % size]
                        result += struct.pack("<H", pixel)
    assert len(result) == 88 * 128
    # Independent random-access decode of every texel and every periodic pad.
    prefixes = [0, 1, 2, 3, 4, 8, 24]
    for exponent, prefix in enumerate(prefixes):
        size = 1 << exponent
        side = max(1, size // 8)
        for y in range(max(8, size)):
            for x in range(max(8, size)):
                at = 128 * (prefix + (y // 8) * side + x // 8) + 2 * ((y % 8) * 8 + x % 8)
                assert struct.unpack_from("<H", result, at)[0] == levels[exponent][(y % size) * size + x % size]
    return bytes(result)


def pack(output):
    output.mkdir(parents=True, exist_ok=True)
    atlas, glyphs = font()
    scene = json.loads((EXAMPLE / "assets/scene.json").read_text())
    meshes = scene["meshes"]
    data = bytearray(128)
    entries = []
    for i, mesh in enumerate(meshes):
        address = MESH + len(data)
        record = native_record(mesh)
        struct.pack_into("<2H", data, 4*i, address//128, len(record)//128-1)
        data += record
        entries.append(dict(name=mesh["name"], address=address, size=len(record),
                            color_mode=mesh["color_mode"], unlit=mesh["unlit"]))
    camera_first = (MESH + len(data)) // 128
    for camera in scene["cameras"]:
        record = bytearray(128)
        struct.pack_into("<21I3h", record, 0,
                         *(v & 0xffffffff for v in camera["matrix"]), *camera["light"])
        data += record
    assert len(meshes)==24 and len(scene["cameras"])==256
    assert sum(len(m["triangles"]) for m in meshes)==1141
    assert MESH + len(data) <= TEXTURE
    resources = [("atlas.bin", ATLAS, atlas), ("room.bin", MESH, bytes(data)),
                 ("texture.bin", TEXTURE, texture())]
    for name, address, payload in resources:
        assert address % 128 == 0 and len(payload) % 128 == 0
        assert 0x4b0000 <= address < address+len(payload) <= 0x800000
        (output/name).write_bytes(payload)
    sources = [Path(__file__), EXAMPLE/"assets/scene.json", EXAMPLE/"assets/font-8x16.txt",
               SCENE/"material_scene.py", SCENE/"solid_scene.py", SCENE/"assets/room-tiles.png"]
    manifest = dict(scene=scene["scene"], triangles=1141, provenance=scene["provenance"],
        resources=[dict(file=n,address=a,bytes=len(p),sha256=sha(output/n)) for n,a,p in resources],
        mesh=dict(directory_sector=MESH//128, directory_word=0, count=24,
                  first_sector=MESH//128,sectors=len(data)//128,entries=entries),
        texture=dict(sector=TEXTURE//128,sectors=88,size_log2=6,full_mip=1),
        atlas=dict(sector=ATLAS//128,sectors=512,glyphs=sorted(glyphs)),
        orbit=dict(radius=2.8,steps=256,camera_first_sector=camera_first),
        sources={str(p.relative_to(ROOT)).replace(chr(92),"/"):sha(p) for p in sources})
    (output/"assets.json").write_text(json.dumps(manifest,indent=2)+"\n")
    roots=dict(DEMO_MESH_DIRECTORY=MESH//128,DEMO_MESH_COUNT=24,
        DEMO_MESH_SECTORS=len(data)//128,DEMO_ROOM_COUNT=6,DEMO_RGB_WORD=12,
        DEMO_BUNNY_WORD=14,DEMO_BUNNY_COUNT=17,DEMO_CAMERA_FIRST=camera_first,
        DEMO_TEXTURE=TEXTURE//128,DEMO_TEXTURE_SECTORS=88,DEMO_ATLAS=ATLAS//128)
    (output/"demo_assets.rs").write_text(''.join(f'pub const {n}: u16 = {v};\n' for n,v in roots.items()))
    print("Built classic box+rabbit+RGB: 24 meshlets, 1141 triangles, 256 orbit poses")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--output", required=True, type=Path)
    pack(parser.parse_args().output.resolve())
