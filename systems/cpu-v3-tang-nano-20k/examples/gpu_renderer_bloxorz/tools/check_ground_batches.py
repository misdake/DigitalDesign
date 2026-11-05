"""Independent wire-decoded legacy-grid reference for the RCC load-time builder."""
import argparse
import importlib.util
import json
from pathlib import Path
import struct


def reference(assets_path):
    spec = importlib.util.spec_from_file_location('assets_reference', Path(__file__).with_name('assets.py'))
    a = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(a)
    metadata = json.loads(assets_path.read_text())
    wire = (assets_path.parent / 'mesh.bin').read_bytes()
    base = next(r['address'] for r in metadata['resources'] if r['kind'] == 'mesh')
    catalog = {r['name']: r for r in metadata['catalog']}

    def decode(index):
        sector, payload = struct.unpack_from('<HH', wire, index * 4)
        at = sector * 128 - base
        q0, q1 = struct.unpack_from('<QQ', wire, at)
        x, y, z, shift = struct.unpack_from('<iiiI', wire, at + 16)
        assert (q0 & 127) == 4 and ((q0 >> 8) & 127) == 2
        assert not (q0 >> 31) & 1 and q1 == 0
        result = []
        for i in range(4):
            p, normal, uv = struct.unpack_from('<III', wire, at + 128 + i * 12)
            xyz = [origin + (((p >> (axis * 10)) & 1023) << (shift & 31)) for axis, origin in enumerate([x, y, z])]
            result.append(xyz + [normal, uv])
        return result

    result = []
    for number in range(1, 34):
        level = a.B.levels()['b' + str(number)]
        width, depth = level['width'], level['depth']
        old_faces = a.W.visible_floor_faces(level)
        carrier = a.C.applicable(level)
        goal = next(i for i, c in enumerate(level['cells']) if c[0] == 6)
        gx, gz = goal % width, goal // width
        keep = []
        for face in old_faces:
            x, z, direction = face & 31, (face >> 5) & 31, (face >> 10) & 7
            inside = gx - 1 <= x <= gx + 1 and gz - 1 <= z <= gz + 1
            neighbor = {1: (x + 1, z), 2: (x, z + 1), 3: (x - 1, z), 4: (x, z - 1)}.get(direction)
            internal = neighbor is not None and gx - 1 <= neighbor[0] <= gx + 1 and gz - 1 <= neighbor[1] <= gz + 1
            if carrier and inside and (direction == 0 or internal):
                continue
            split = level['cells'][z * width + x][0] == 7
            material = 1 if split and direction == 0 else 2 if split else 0
            global_quant = width <= 15 and depth <= 15
            bucket = (0 if direction == 0 else 1) if global_quant else 1 + x // 3 + z // 3 * ((width + 2) // 3)
            variant = face >> 13
            index = catalog['MARKED_SPLIT']['first'] + variant if material == 1 else catalog['SHARED_FLOOR']['first'] + variant * 5 + direction
            vertices = decode(index)
            for vertex in vertices:
                vertex[0] += (2 * x - width + 1) * 65536
                vertex[2] += (2 * z - depth + 1) * 65536
                if global_quant:
                    vertex[1] *= 4
            keep.append(dict(face=face, material=material, bucket=bucket, vertices=vertices))
        keep.sort(key=lambda row: (row['material'], row['bucket']))
        assert [row['face'] for row in keep] == a.F.schedule(level, old_faces, carrier)
        result.append(dict(level=number, width=width, depth=depth, original_faces=len(old_faces), faces=keep))
    return result


if __name__ == '__main__':
    p = argparse.ArgumentParser()
    p.add_argument('--assets', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    args = p.parse_args()
    args.output.write_text(json.dumps(reference(args.assets), separators=(',', ':')) + '\n')
