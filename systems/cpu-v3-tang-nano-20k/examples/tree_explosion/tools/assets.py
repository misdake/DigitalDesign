"""Pack the supplied inspection stage and captured explosion into native meshlets."""

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


def sprite_record(sprite, origin, indices):
    kind, position, (width, height), (s, c), (u0, v0, u1, v1), brightness = sprite
    extent = 128 if kind == 0 else 64
    center = [(v - base + shift) * SCENE_SCALE
              for v, base, shift in zip(source_center(position), origin, (4, 1.3, 0))]
    half_x, half_y = width / 1024, height / 1024
    vertices = []
    for (x, y), (u, v) in zip(
        ((-half_x, half_y), (half_x, half_y), (half_x, -half_y), (-half_x, -half_y)),
        ((u0, v0), (u1, v0), (u1, v1), (u0, v1)),
    ):
        offset = ((x * c + y * s) * SCENE_SCALE / 4096,
                  (-x * s + y * c) * SCENE_SCALE / 4096)
        vertices.append((center, offset, (u / extent, v / extent)))
    return record(vertices, indices, billboard=True, shift=0), center, kind, min(255, 2 * brightness)


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
    resources, constants = [], {}
    address = asset_regions()[0][0]

    def place(name, payload):
        nonlocal address
        address = (address + 255) & ~255
        validate_asset_range(address, len(payload))
        filename = name + ".bin"
        (output / filename).write_bytes(payload)
        resources.append(dict(file=filename, address=address, bytes=len(payload),
                              sha256=hashlib.sha256(payload).hexdigest()))
        constants[f"{name.upper()}_SECTOR"] = address // 128
        constants[f"{name.upper()}_SECTORS"] = len(payload) // 128
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
    starts = []
    for frame in frames:
        starts.append(len(layers) // 4)
        for sprite in frame:
            payload, center, kind, shade = sprite_record(sprite, origin, scene["indices"])
            records.append(payload)
            layer(center, kind + 2, shade)
    starts.append(len(layers) // 4)
    assert len(records) <= 5460
    mesh = bytearray((len(records) * 4 + 127) & ~127)
    for index, payload in enumerate(records):
        struct.pack_into("<2H", mesh, index * 4, (address + len(mesh)) // 128, len(payload) // 128 - 1)
        mesh.extend(payload)
    place("mesh", mesh)
    constants.update(MESH_COUNT=len(records), TICK_MS=scene["tick_ms"], FRAME_COUNT=len(frames))

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
                    frames=len(frames), sprites=len(layers) // 4 - 2)
    (output / "assets.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    rust = "use crate::dsl_rt::*;\n" + "".join(
        f"pub const SCENE_{name}: u16 = {value};\n" for name, value in constants.items()
    )
    rust += emit_buffer("SCENE_FRAME_STARTS", starts) + emit_buffer("SCENE_LAYERS", layers)
    (output / "tree_explosion_assets.rs").write_text(rust, encoding="utf-8")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--output", required=True, type=Path)
    pack(parser.parse_args().output)
