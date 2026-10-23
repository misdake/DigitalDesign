"""Bounded fixture audit using the existing native host record/numerical encoder.

No command buffer is generated here: RCC alone records the submitted CB.
"""
import importlib.util
import json
from pathlib import Path
import struct
import sys

ROOT = Path(__file__).resolve().parents[4]
OUT = Path(sys.argv[1]).resolve()
spec = importlib.util.spec_from_file_location(
    "rcc_d2_native_oracle",
    ROOT / "ip/gpu-v2/examples/vliw_integration/reference/scene/oracle.py",
)
oracle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(oracle)

memory = {}
address = 0
for line in (OUT / "assets.hex").read_text().splitlines():
    if line.startswith("@"):
        address = int(line[1:], 16) * 8
    else:
        for i, b in enumerate(int(line, 16).to_bytes(8, "little")):
            memory[address + i] = b
        address += 8

reports = []
for index in range(5):
    base = 0x4B0080 + index * 256
    raw = bytes(memory[base + i] for i in range(256))
    uniform = [0] * 32
    for i in (0, 5, 10, 12, 16, 20, 21):
        uniform[i] = 65536
    uniform[22] = 0xFFFFFFFF
    vertices = [list(struct.unpack_from("<3I", raw, 128 + 12 * i)) for i in range(3)]
    instance = dict(
        name=f"asset{index}", group=0, slot=0, uniform=uniform,
        config=[(0, 8192, 16384, 0, 8192)[index], 0, 65536, 264],
        vertices=vertices, triangles=[[0, 1, 2]], color_mode=index < 3,
        cull_negative=0, scale_s=0, cfg_far_z=100 * 65536,
        cfg_tex_w_log2=0, cfg_tex_h_log2=0, cfg_max_mip=0,
    )
    folder = OUT / "native-assets" / f"record{index}"
    meta, packets = oracle.prepare(instance, folder)
    encoded = {int(a): int(v, 16) for a, v in
               (line.split() for line in (folder / "dma.txt").read_text().splitlines())}
    for q in range(4):
        assert encoded[256 + q] == int.from_bytes(raw[q * 8:q * 8 + 8], "little")
    for q in range(6):
        assert encoded[q] == int.from_bytes(raw[128 + q * 8:136 + q * 8], "little")
    assert raw[32:128] == bytes(96) and raw[176:] == bytes(80)
    assert meta["packets"] == 1 and meta["coverage_pixels"] == 120
    # Exercise the existing independent fixed reciprocal/depth/attribute path.
    for packet in packets:
        for qi in packet["masks"]:
            result = oracle.expected_quad(packet, qi)
            assert all(abs(z - 256) <= 1 for z in result["depth"].values())
    reports.append(meta)
(OUT / "asset-audit.json").write_text(json.dumps(dict(status="PASS", records=reports), indent=2))
print("D2_NATIVE_ASSET_PASS records=5 coverage_per_triangle=120")
