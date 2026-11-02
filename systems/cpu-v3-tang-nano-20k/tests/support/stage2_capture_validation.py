"""Actual Stage2 raw inputs through the existing capture validator; no oracle masks."""
import copy
import json
from pathlib import Path
import struct
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "tools/gpu_profiler"))
import identity
import snapshot as s
from system_dump import materialize

root = Path(sys.argv[1])
validation = Path(sys.argv[2]) if len(sys.argv) > 2 else root / "validation"
validation.mkdir()  # A retry must use another output; the actual raw input stays frozen.
abi = s.Abi()


def words(path):
    data = path.read_bytes()
    return list(struct.unpack("<" + "H" * (len(data) // 2), data))


header = words(root / "header.bin")
flat = words(root / "ranges.bin")
rows = [flat[i:i + 8] for i in range(0, len(flat), 8)]
blobs = [(root / f"range-{i:03}.bin").read_bytes() for i in range(len(rows))]


def decode(h, r, b):
    raw = s.encode_reference(h, r, b, abi)
    return s.decode_stream(raw, abi)


captures, report = decode(header, rows, blobs)
assert not report["failures"] and len(captures) == 1, report
preflight = identity.input_preflight(captures[0])
assert not preflight["build_identity_verified"]  # Fixture records provenance separately.
materialize(root, validation / "snapshot", {"qualification": "Stage2 actual functional-prefix capture"}, abi)
tails = [i for i, r in enumerate(rows) if r[1] in (abi["TAG_COLOR"], abi["TAG_DEPTH"])
         and not r[0] & abi["WRITE"]]
planes = [i for i, r in enumerate(rows) if r[1] in (abi["TAG_COLOR"], abi["TAG_DEPTH"])
          and r[0] & abi["WRITE"]]
assert len(tails) == len(planes) == 2
results = {"actual_capture": "PASS", "plane_rows": planes, "read_only_tail_rows": tails,
           "wire_format": "existing explicit-v2", "build_identity_verified": False,
           "negative_cases": {}}
for case in ("tail-write", "tail-full-clear", "plane-crosses-guard", "conflicting-alias"):
    h, r, b = copy.deepcopy((header, rows, blobs))
    tail = tails[0]
    if case == "tail-write":
        r[tail][0] |= abi["WRITE"]
    elif case == "tail-full-clear":
        r[tail][0] |= abi["FULL_CLEAR"]
    elif case == "plane-crosses-guard":
        plane = planes[0]
        size = r[plane][4] | r[plane][5] << 16
        size += 128
        r[plane][4:6] = [size & 65535, size >> 16]
        b[plane] += b[tail][:128]
    else:
        r.append(copy.deepcopy(r[tail]))
        conflicting = bytearray(b[tail])
        conflicting[0] ^= 1
        b.append(bytes(conflicting))
        h[abi["H_RANGE_COUNT"]] += 1
    try:
        directory = validation / ("negative-" + case)
        directory.mkdir()
        wire = s.encode_reference(h, r, b, abi)
        (directory / "mutated-wire.bin").write_bytes(wire)
        found, failed = s.decode_stream(wire, abi)
        assert not found and failed["failures"], f"mutation accepted: {case}"
        results["negative_cases"][case] = failed["failures"]
    except s.SnapshotError as error:
        results["negative_cases"][case] = str(error)
    (directory / "rejection.json").write_text(json.dumps(results["negative_cases"][case], indent=2), encoding="utf-8")
(validation / "validator-result.json").write_text(json.dumps(results, indent=2), encoding="utf-8")
print("PASS actual v2 capture, two exact visible planes/tails, four semantic mutations rejected")
