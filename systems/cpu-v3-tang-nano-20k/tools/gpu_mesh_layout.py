"""Native V96/T64 meshlet records; addresses belong to the surrounding package."""
import struct


def native_record(mesh):
    vertices, triangles = mesh["vertices"], mesh["triangles"]
    assert 0 < len(vertices) <= 64 and 0 < len(triangles) <= 96
    offset = (3 * len(vertices) + 1) // 2
    payload_size = ((offset + (len(triangles) + 2) // 3) * 8 + 127) // 128 * 128
    assert 128 <= payload_size <= 1024
    record = bytearray(128 + payload_size)
    origin = mesh["config"]
    cull = int(mesh.get("cull_negative", True))
    billboard = int(mesh.get("billboard", False))
    meta = [len(vertices) | (len(triangles) << 8) | (20 << 16) | (offset << 24) | (cull << 31),
            (mesh.get("color_mode", 0) << 4) | billboard,
            (origin[0] & 0xFFFFFFFF) | ((origin[1] & 0xFFFFFFFF) << 32),
            (origin[2] & 0xFFFFFFFF) | (origin[3] << 32)]
    struct.pack_into("<4Q", record, 0, *meta)
    for i, vertex in enumerate(vertices):
        assert vertex[0] < 1 << 30 and vertex[1] < 1 << 30
        assert not billboard or vertex[1] < 1 << 20
        struct.pack_into("<3I", record, 128 + i * 12, *vertex)
    for i, triangle in enumerate(triangles):
        assert len(triangle) == 3 and all(0 <= j < len(vertices) for j in triangle)
        at = 128 + (offset + i // 3) * 8
        value = struct.unpack_from("<Q", record, at)[0]
        value |= sum(index << (6 * lane) for lane, index in enumerate(triangle)) << (18 * (i % 3))
        struct.pack_into("<Q", record, at, value)
    return bytes(record)
