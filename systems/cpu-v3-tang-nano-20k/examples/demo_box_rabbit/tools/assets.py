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
sys.path.insert(0, str(ROOT / "systems/cpu-v3-tang-nano-20k/tools"))
from gpu_layout import validate_asset_range, sectors
from gpu_layout import description as capture_description
import ui_font
import gpu_mesh_layout
from gpu_mesh_layout import native_record
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
    rows = ui_font.load_rows()
    return bytes(ui_font.atlas(rows)), rows




def texture():
    from gpu_texture_layout import pack_levels
    result = pack_levels(material_scene.texture_levels())
    assert len(result) == 100 * 128
    return result


def pack(output):
    bounds = sectors()
    atlas_base = bounds["GPU_ASSET_FIRST_SECTOR"]*128
    mesh_base = atlas_base+65536
    texture_base = mesh_base+65536
    texture_sectors = 100
    output.mkdir(parents=True, exist_ok=True)
    atlas, glyphs = font()
    scene = json.loads((EXAMPLE / "assets/scene.json").read_text())
    meshes = scene["meshes"]
    data = bytearray(128)
    entries = []
    for i, mesh in enumerate(meshes):
        address = mesh_base + len(data)
        record = native_record(mesh)
        struct.pack_into("<2H", data, 4*i, address//128, len(record)//128-1)
        data += record
        entries.append(dict(name=mesh["name"], address=address, size=len(record),
                            color_mode=mesh["color_mode"], unlit=mesh["unlit"]))
    camera_first = (mesh_base + len(data)) // 128
    for camera in scene["cameras"]:
        record = bytearray(128)
        struct.pack_into("<21I3h", record, 0,
                         *(v & 0xffffffff for v in camera["matrix"]), *camera["light"])
        data += record
    assert len(meshes)==24 and len(scene["cameras"])==256
    assert sum(len(m["triangles"]) for m in meshes)==1141
    assert mesh_base + len(data) <= texture_base
    resources = [("atlas.bin", atlas_base, atlas, "atlas"), ("room.bin", mesh_base, bytes(data), "mesh"),
                 ("texture.bin", texture_base, texture(), "texture")]
    for name, address, payload, kind in resources:
        assert address % 128 == 0 and len(payload) % 128 == 0
        validate_asset_range(address, len(payload))
        (output/name).write_bytes(payload)
    sources = [Path(gpu_mesh_layout.__file__), Path(__file__), EXAMPLE/"assets/scene.json", *ui_font.SOURCES,
               ROOT/"systems/cpu-v3-tang-nano-20k/tools/gpu_layout.py",
               ROOT/"systems/cpu-v3-tang-nano-20k/rcc/gpu_layout.rs",
               ROOT/"systems/cpu-v3-tang-nano-20k/tools/gpu_texture_layout.py",
               SCENE/"material_scene.py", SCENE/"solid_scene.py", SCENE/"assets/room-tiles.png"]
    manifest = dict(source_root='repository', hardware_profile="production",
        layout_identity="rgb565-macro16x16-column-major-pad16-cache16x8-8KiB",
        framebuffer=dict(physical_bytes=204800,reserved_bytes=262144,clear_sectors=1600),
        scene=scene["scene"], triangles=1141, provenance=scene["provenance"],
        resources=[dict(file=n,address=a,bytes=len(p),sha256=sha(output/n),kind=k,
                        **({'layout':capture_description()['texture_layout']} if k=='texture' else {}))
                   for n,a,p,k in resources],
        capture_layout=capture_description(),
        mesh=dict(directory_sector=mesh_base//128, directory_word=0, count=24,
                  first_sector=mesh_base//128,sectors=len(data)//128,entries=entries),
        texture=dict(sector=texture_base//128,sectors=texture_sectors,size_log2=6,full_mip=1),
        atlas=dict(sector=atlas_base//128,sectors=512,glyphs=sorted(glyphs)),
        orbit=dict(radius=2.8,steps=256,camera_first_sector=camera_first),
        sources={str(p.relative_to(ROOT)).replace(chr(92),"/"):sha(p) for p in sources})
    (output/"assets.json").write_text(json.dumps(manifest,indent=2)+"\n")
    roots=dict(DEMO_MESH_DIRECTORY=mesh_base//128,DEMO_MESH_COUNT=24,
        DEMO_MESH_SECTORS=len(data)//128,DEMO_ROOM_COUNT=6,DEMO_RGB_WORD=12,
        DEMO_BUNNY_WORD=14,DEMO_BUNNY_COUNT=17,DEMO_CAMERA_FIRST=camera_first,
        DEMO_TEXTURE=texture_base//128,DEMO_TEXTURE_SECTORS=texture_sectors,DEMO_ATLAS=atlas_base//128)
    (output/"demo_assets.rs").write_text(''.join(f'pub const {n}: u16 = {v};\n' for n,v in roots.items()))
    print("Built classic box+rabbit+RGB: 24 meshlets, 1141 triangles, 256 orbit poses")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    pack(args.output.resolve())
