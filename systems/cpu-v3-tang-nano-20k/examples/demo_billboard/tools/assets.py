"""Pack static stage geometry, one reusable quad and captured sprite parameters."""

import argparse
import hashlib
import json
from pathlib import Path
import struct
import sys

from PIL import Image

EXAMPLE = Path(__file__).resolve().parents[1]
REPO = EXAMPLE.parents[3]
sys.path.insert(0, str(EXAMPLE.parents[1] / "tools"))

from gpu_layout import LAYOUT, asset_regions, validate_asset_range
from gpu_mesh_layout import native_record
from gpu_texture_layout import pack_levels, rgba4444

TEXTURES = ("grass", "road", "rock", "ntree1", "ntree2", "flash", "smoke")
QUAD_UV = ((0, 0), (1, 0), (1, 1), (0, 1))
QUAD_INDICES = (0, 1, 2, 0, 2, 3)
SCENE_SCALE = 1 / 8


def record(vertices, indices, billboard=False, shift=8):
    # Opaque meshes share one Q8.8 lattice across chunk boundaries.
    # A Billboard sprite has one base point and retains the full Q16.16 origin.
    # Convert the inspection scene to the GPU's camera-space X convention.
    positions = [[round(axis * (65536 >> shift)) for axis in (-x, y, z)]
                 for (x, y, z), _, _ in vertices]
    origin = [min(xyz[k] for xyz in positions) for k in range(3)]
    packed = []
    for xyz, (_, (dx, dy), uv) in zip(positions, vertices):
        delta = [a - b for a, b in zip(xyz, origin)]
        offset = [round(-dx * 512), round(dy * 512)]
        assert all(0 <= d < 1024 for d in delta)
        assert all(-512 <= v < 512 for v in offset)
        assert all(0 <= v <= 1 for v in uv)
        u, v = [min(65535, round(axis * 65536)) for axis in uv]
        packed.append([sum(d << (10 * k) for k, d in enumerate(delta)),
                       (offset[0] & 1023) | ((offset[1] & 1023) << 10),
                       u | (v << 16)])
    return native_record(dict(config=[*(v << shift for v in origin), shift],
                              vertices=packed, billboard=billboard, cull_negative=False,
                              triangles=[indices[i:i + 3] for i in range(0, len(indices), 3)]))


def surface(y, x0, z0, x1, z1):
    return [(x0, y, z0), (x1, y, z0), (x1, y, z1), (x0, y, z1)]


def stone(x, z, width, height):
    radius = width / 2
    base = surface(0, x - radius, z - radius, x + radius, z + radius)
    radius *= 0.75
    top = surface(height, x - radius, z - radius, x + radius, z + radius)
    yield top
    for i in range(4):
        j = (i + 1) % 4
        yield [base[i], base[j], top[j], top[i]]


def stage():
    grass = [surface(0, x, z, x + 2, z + 2)
             for x in range(-10, 10, 2) for z in range(-6, 6, 2)]
    road = [surface(0.0625, x, 2.5, x + 2, 4) for x in range(-10, 10, 2)]
    base = surface(-0.6, -10, -6, 10, 6)
    top = surface(0, -10, -6, 10, 6)
    rock = [[base[i], base[(i + 1) % 4], top[(i + 1) % 4], top[i]] for i in range(4)]
    for dimensions in ((-7, -3, 1.3, .85), (6.7, -2.7, 1.8, 1.1), (7.6, 4.6, .9, .55)):
        rock.extend(stone(*dimensions))
    return grass, road, rock


def stage_records(quads):
    for start in range(0, len(quads), 16):
        vertices = []
        indices = []
        for quad in quads[start:start + 16]:
            indices.extend(len(vertices) + i for i in QUAD_INDICES)
            vertices.extend((tuple(x * SCENE_SCALE for x in xyz), (0, 0), uv)
                            for xyz, uv in zip(quad, QUAD_UV))
        yield record(vertices, indices)


def source_center(position_q10):
    x, y, z = position_q10
    return (x >> 3) / 128, (y >> 3) / 128, -(z >> 3) / 128


def sprite_values(sprite, origin):
    kind, position, (width, height), (sine, cosine), uv, brightness = sprite
    center = [(v - base + shift) * SCENE_SCALE
              for v, base, shift in zip(source_center(position), origin, (4, 1.3, 0))]
    fixed = [round(v * 65536) for v in (-center[0], center[1], center[2])]
    assert all(0 < size < 32768 for size in (width, height))
    assert all(-4096 <= value <= 4096 for value in (sine, cosine))
    extent = 128 if kind == 0 else 64
    assert all(0 <= value <= extent for value in uv)
    for x, y in ((-width, height), (width, height), (width, -height), (-width, -height)):
        dx = round(-(x * cosine + y * sine) / 65536)
        dy = round((-x * sine + y * cosine) / 65536)
        assert -512 <= dx < 512 and -512 <= dy < 512
    values = [(kind + 2) * 256 + min(255, 2 * brightness)]
    for value in fixed:
        values.extend((value & 65535, (value >> 16) & 65535))
    values.extend((width, height, sine & 65535, cosine & 65535,
                   uv[0] | (uv[1] << 8), uv[2] | (uv[3] << 8)))
    assert len(values) == 13
    return values


def texture_pixels(image, name):
    if name in ("flash", "smoke"):
        # Convert emissive RGB into coverage of the original hue for SrcOver.
        for r, g, b, a in image.getdata():
            peak = max(r, g, b)
            if not peak or not a:
                yield 0
            else:
                scaled = tuple((v * 255 + peak // 2) // peak for v in (r, g, b))
                yield rgba4444((*scaled, (peak * a + 127) // 255))
    elif name in ("ntree1", "ntree2"):
        yield from map(rgba4444, image.getdata())
    else:
        for r, g, b, _ in image.getdata():
            yield ((r * 31 + 127) // 255 << 11) | ((g * 63 + 127) // 255 << 5) | ((b * 31 + 127) // 255)


def emit_buffer(name, values):
    rows = ("    " + ", ".join(map(str, values[start:start + 16])) + ",\n"
            for start in range(0, len(values), 16))
    return f"pub static {name}: Buf<u16, {len(values)}> = Buf::new([\n{''.join(rows)}]);\n"


def pack(output):
    output.mkdir(parents=True, exist_ok=True)
    assets = EXAMPLE / "assets"
    document = assets / "scene.json"
    scene = json.loads(document.read_text(encoding="utf-8"))
    frames = scene["frames"]
    assert len(frames) == 199 and all(len(f) <= 10 for f in frames) and not frames[-1]
    resources, constants, placements = [], {}, {}
    address = asset_regions()[0][0]

    def place(name, payload):
        nonlocal address
        address = (address + 255) & ~255
        validate_asset_range(address, len(payload))
        filename = name + ".bin"
        (output / filename).write_bytes(payload)
        resources.append(dict(file=filename, address=address, bytes=len(payload),
                              sha256=hashlib.sha256(payload).hexdigest()))
        placements[name] = (address // 128, len(payload))
        address += len(payload)

    records = []
    for name, quads in zip(TEXTURES[:3], stage()):
        first = len(records)
        records.extend(stage_records(quads))
        constants[f"{name.upper()}_FIRST"] = first
        constants[f"{name.upper()}_COUNT"] = len(records) - first
    constants["TRANSPARENT_FIRST"] = len(records)
    layers = []

    def layer(center, kind, shade):
        x, y, z = center
        layers.extend([kind * 256 + shade, *(round(v * 256) & 65535 for v in (-x, y, z))])

    for kind, part in enumerate(scene["tree"]):
        vertices = [((x * SCENE_SCALE - .5, y * SCENE_SCALE, z * SCENE_SCALE),
                     (side * SCENE_SCALE, 0), uv) for (x, y, z), side, uv in part["vertices"]]
        center = [sum(v[0][i] for v in vertices) / len(vertices) for i in range(3)]
        records.append(record(vertices, part["indices"], billboard=True))
        layer(center, kind, part["shade"])

    origin = source_center(frames[0][0][1])
    starts, animation = [], []
    for frame in frames:
        starts.append(len(animation) // 13)
        for sprite in frame:
            animation.extend(sprite_values(sprite, origin))
    starts.append(len(animation) // 13)
    assert len(records) <= 5460
    mesh = bytearray((len(records) * 4 + 127) & ~127)
    for index, payload in enumerate(records):
        struct.pack_into("<2H", mesh, index * 4, (address + len(mesh)) // 128, len(payload) // 128 - 1)
        mesh.extend(payload)
    place("mesh", mesh)
    quad = record([((0, 0, 0), (0, 0), uv) for uv in QUAD_UV],
                  scene["indices"], billboard=True, shift=0)
    place("quad", quad)
    animation_bytes = struct.pack(f"<{len(animation)}H", *animation)
    place("animation", animation_bytes + bytes((-len(animation_bytes)) % 128))
    constants.update(TICK_MS=scene["tick_ms"], FRAME_COUNT=len(frames))
    constants.update(SPRITE_CAPACITY=max(map(len, frames)),
                     SPRITE_STORAGE_BYTES=128 + max(map(len, frames)) * len(quad))

    paths = [Path(__file__), LAYOUT, document,
             EXAMPLE.parents[1] / "tools/gpu_mesh_layout.py",
             EXAMPLE.parents[1] / "tools/gpu_texture_layout.py",
             EXAMPLE.parents[1] / "tools/gpu_layout.py"]
    for name in TEXTURES:
        path = assets / (name + ".png")
        paths.append(path)
        with Image.open(path) as image:
            assert image.width == image.height and image.width.bit_count() == 1
            constants[f"{name.upper()}_SIZE"] = image.width
            exponent = image.width.bit_length() - 1
            pixels = list(texture_pixels(image.convert("RGBA"), name))
            place(name, pack_levels({exponent: pixels}))
    manifest = dict(source_root="repository", scene="tree_explosion",
                    resources=resources, sources={
                        p.resolve().relative_to(REPO).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
                        for p in paths}, export_sha256=scene["export_sha256"],
                    frames=len(frames), sprites=len(animation) // 13)
    (output / "assets.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    rust = ("use crate::dsl_rt::*;\nmod gpu_api; use gpu_api::*;\n"
            "mod static_assets; use static_assets::*;\n") + "".join(
        f"pub const SCENE_{name}: u16 = {value};\n" for name, value in constants.items()
        if not name.endswith("_SIZE")
    )
    for name, (sector, length) in placements.items():
        symbol = f"SCENE_{name.upper()}"
        rust += (f"const {symbol}_BYTES: AssetBytes = AssetBytes {{ first_sector: {sector}, "
                 f"sector_count: {length // 128}, byte_len_lo: {length & 65535}, "
                 f"byte_len_hi: {length >> 16} }};\n")
        if name == "mesh":
            rust += (f"pub const {symbol}: MeshDesc = unsafe {{ "
                     f"MeshDesc::from_asset({symbol}_BYTES, 0, {len(records)}) }};\n")
        elif name in TEXTURES:
            size = constants[f"{name.upper()}_SIZE"]
            fmt = "Rgb565" if name in TEXTURES[:3] else "Rgba4444"
            rust += (f"pub const {symbol}: TextureDesc = unsafe {{ TextureDesc::from_asset("
                     f"{symbol}_BYTES, TextureLayout::square({size}, TextureFormat::{fmt}, "
                     "Mipmaps::BaseOnly)) };\n")
        else:
            rust += (f"pub const {symbol}: GpuMemory = unsafe {{ "
                     f"GpuMemory::from_asset({symbol}_BYTES) }};\n")
    rust += emit_buffer("SCENE_FRAME_STARTS", starts) + emit_buffer("SCENE_LAYERS", layers)
    (output / "tree_explosion_assets.rs").write_text(rust, encoding="utf-8")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--output", required=True, type=Path)
    pack(parser.parse_args().output)
