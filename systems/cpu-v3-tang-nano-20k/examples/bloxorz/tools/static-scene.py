"""Small Bloxorz asset producer/reference using the existing native GPU oracle.

This script never generates a command buffer. RCC records every command.
"""
from pathlib import Path
import importlib.util
import json
import struct
import sys

sys.dont_write_bytecode = True

ROOT = Path(__file__).resolve().parents[5]
spec = importlib.util.spec_from_file_location(
    "bloxorz_native_oracle", ROOT / "ip/gpu-v2/examples/vliw_integration/reference/scene/oracle.py")
O = importlib.util.module_from_spec(spec)
spec.loader.exec_module(O)
OUT = None
BASE = 0x4B0000


def box(name, half, faces, shift):
    vertices, triangles = [], []
    origins = [-round(v * 65536 / (1 << shift)) * (1 << shift) for v in half]
    colors = [(110, 130, 155), (48, 63, 82), (75, 94, 119)] if name == "floor" else [
        (218, 145, 78), (173, 97, 45), (195, 115, 53),
        (145, 77, 35), (182, 104, 47), (151, 82, 36)]
    for face, (axis, sign) in enumerate(faces):
        first = len(vertices)
        for q in [(-1, -1), (1, -1), (1, 1), (-1, 1)]:
            signs = [0, 0, 0]
            signs[axis] = sign
            signs[(axis + 1) % 3], signs[(axis + 2) % 3] = q
            xyz = [0 if signs[i] < 0 else (-2 * origins[i]) >> shift for i in range(3)]
            assert all(0 <= v <= 1023 for v in xyz)
            normal = [0, 0, 0]
            normal[axis] = 511 * sign
            vertices.append([O.pack(xyz, 10), O.pack(normal, 10), O.pack(colors[face], 8)])
        triangles.extend([[first, first + 1, first + 2], [first, first + 2, first + 3]])
    return dict(name=name, group=name, vertices=vertices, triangles=triangles,
                config=origins + [shift | 256], color_mode=1,
                cull_negative=False, cfg_far_z=100 * 65536,
                cfg_tex_w_log2=0, cfg_tex_h_log2=0, cfg_max_mip=0)


def image_record(mesh, folder, uniform):
    meta, packets = O.prepare(dict(mesh, uniform=uniform), folder)
    image = {int(a): int(v, 16) for a, v in
             (line.split() for line in (folder / "dma.txt").read_text().splitlines())}
    data = b"".join(struct.pack("<Q", image.get(q, 0)) for q in range(max(q for q in image if q < 128) + 1))
    padded = (len(data) + 127) // 128 * 128
    record = b"".join(struct.pack("<Q", image[256 + q]) for q in range(4)) + bytes(96)
    return record + data + bytes(padded - len(data)), padded, packets, meta


def assets():
    meshes = [box("floor", [1, .2, 1], [(1, 1), (0, 1), (2, 1)], 8),
              box("block", [.98] * 3, [(1, 1), (0, 1), (2, 1), (1, -1), (0, -1), (2, -1)], 7)]
    uniform = [0] * 32
    for i in [0, 5, 10, 12, 16, 20, 21]:
        uniform[i] = 65536
    uniform[11] = 20 * 65536
    uniform[22] = 0xFFFFFFFF
    memory = bytearray(4224)
    cursor = 128
    for index, mesh in enumerate(meshes):
        record, padded, _, _ = image_record(mesh, OUT / f"asset-{index}", uniform)
        struct.pack_into("<HH", memory, index * 4, (BASE + cursor) // 128, padded // 128)
        memory[cursor:cursor + len(record)] = record
        mesh["record_address"] = BASE + cursor
        mesh["payload_bytes"] = padded
        cursor += len(record)
    assert cursor <= 4096
    memory[4096:4098] = b"\xff\xff"
    (OUT / "assets.bin").write_bytes(memory)
    (OUT / "assets.hex").write_text(f"@{BASE // 8:x}\n" + "".join(
        f"{int.from_bytes(memory[i:i + 8], 'little'):016x}\n" for i in range(0, len(memory), 8)))
    (OUT / "meshes.json").write_text(json.dumps(meshes, indent=2))
    print(f"BLOX_ASSETS records=2 bytes={len(memory)} vertices=36 triangles=18")


def golden():
    meshes = json.loads((OUT / "meshes.json").read_text())
    lines = (OUT / "matrices.txt").read_text().splitlines()
    assert len(lines) == 18
    color, depth = [0] * 96000, [65535] * 96000
    covered = 0
    for index, line in enumerate(lines):
        words = [int(v, 16) for v in line.split()]
        assert len(words) == 43
        kind = words[0]
        uniform = [words[1 + 2 * i] | words[2 + 2 * i] << 16 for i in range(21)]
        uniform += [65536, 0xFFFFFFFF] + [0] * 9
        _, _, packets, _ = image_record(meshes[kind], OUT / f"draw-{index}", uniform)
        for packet in packets:
            for qi, mask in packet["masks"].items():
                x, y = qi % 200 * 2, qi // 200 * 2
                h = tuple(c + 2 * nx * x + 2 * ny * y for nx, ny, c in packet["edges"])
                quad = O.M.quad(packet["triangle"], h, mask)
                for lane, pixel in quad["pixels"].items():
                    if not pixel["valid"]:
                        continue
                    p = (y + (lane >> 1)) * 400 + x + (lane & 1)
                    if pixel["Z16"] < depth[p]:
                        r, g, b = pixel["rgb"]
                        color[p] = ((r * 31 + 127) // 255 << 11) | ((g * 63 + 127) // 255 << 5) | ((b * 31 + 127) // 255)
                        depth[p] = pixel["Z16"]
                        covered += 1
        if index in [8, 17]:
            frame = 1 if index == 8 else 2
            for name, values in [("gold", color), ("depth", depth)]:
                (OUT / f"{name}{frame}.hex").write_text("".join(f"{v:04x}\n" for v in values))
            if frame == 1:
                color, depth = [0] * 96000, [65535] * 96000
    print(f"BLOX_REFERENCE draws=18 visible_updates={covered}")


def preview():
    from PIL import Image
    values = [int(v, 16) for v in (OUT / "actual-color.hex").read_text().splitlines()]
    assert len(values) == 96000
    pixels = [((v >> 11) * 255 // 31, ((v >> 5) & 63) * 255 // 63, (v & 31) * 255 // 31) for v in values]
    image = Image.new("RGB", (400, 240))
    image.putdata(pixels)
    image.save(OUT / "native-scene.png")


if __name__ == "__main__":
    OUT = Path(sys.argv[2]).resolve()
    OUT.mkdir(parents=True, exist_ok=True)
    {"assets": assets, "golden": golden, "preview": preview}[sys.argv[1]]()
