"""Static face schedule and immutable vertex templates; vertices are built by RCC."""

def face_key(level, face):
    x, z, direction = face & 31, (face >> 5) & 31, (face >> 10) & 7
    kind = level['cells'][z * level['width'] + x][0]
    assert kind in (1, 7)
    material = 0 if kind == 1 else 1 if direction == 0 else 2
    global_quant = level['width'] <= 15 and level['depth'] <= 15
    bucket = (0 if direction == 0 else 1) if global_quant else 1 + x // 3 + (z // 3) * ((level['width'] + 2) // 3)
    return material, bucket


def schedule(level, faces, carrier):
    gx = gz = -99
    if carrier:
        goal = next(i for i, c in enumerate(level['cells']) if c[0] == 6)
        gx, gz = goal % level['width'], goal // level['width']
    result = []
    for face in faces:
        x, z, direction = face & 31, (face >> 5) & 31, (face >> 10) & 7
        inside = abs(x - gx) <= 1 and abs(z - gz) <= 1
        merged = direction == 0 or direction == 1 and x < gx + 1 or direction == 2 and z < gz + 1 or direction == 3 and x > gx - 1 or direction == 4 and z > gz - 1
        if not (inside and merged):
            result.append(face)
    return sorted(result, key=lambda face: face_key(level, face))


def templates(w, split):
    result = []
    meshes = [w.unit_floor_face(face, variant) for face in range(5) for variant in range(4)] + split
    assert len(meshes) == 24
    for mesh in meshes:
        assert len(mesh['vertices']) == 4 and mesh['triangles'] == [[0, 1, 2], [0, 2, 3]]
        assert not mesh['cull_negative'] and mesh['color_mode'] == 0
        for position, normal, uv in mesh['vertices']:
            components = [(normal >> (axis * 10)) & 1023 for axis in range(3)]
            assert sum(v != 0 for v in components) == 1 and all(v in (0, 511, 513) for v in components)
            xyz = [mesh['config'][a] + (((position >> (a * 10)) & 1023) << 9) for a in range(3)]
            assert xyz[0] in (-65536, 65536) and xyz[2] in (-65536, 65536) and xyz[1] in (0, -26112)
            corner = int(xyz[0] > 0) | (int(xyz[1] == 0) << 1) | (int(xyz[2] > 0) << 2)
            result.extend([corner, normal & 65535, normal >> 16, uv & 65535, uv >> 16])
    return result


def fragile_templates(meshes):
    """Preserve every original face's corner, normal and marker-atlas UV."""
    result = []
    assert len(meshes) == 4
    for mesh in meshes:
        assert len(mesh['vertices']) == 20 and len(mesh['triangles']) == 10
        assert mesh['triangles'] == [t for face in range(5) for t in ([face*4, face*4+1, face*4+2], [face*4, face*4+2, face*4+3])]
        assert mesh['config'] == [-65536, -26112, -65536, 265]
        for position, normal, uv in mesh['vertices']:
            xyz = [mesh['config'][a] + (((position >> (a * 10)) & 1023) << 9) for a in range(3)]
            assert xyz[0] in (-65536, 65536) and xyz[2] in (-65536, 65536) and xyz[1] in (0, -26112)
            corner = int(xyz[0] > 0) | (int(xyz[1] == 0) << 1) | (int(xyz[2] > 0) << 2)
            result.extend([corner, normal & 65535, normal >> 16, uv & 65535, uv >> 16])
    return result
