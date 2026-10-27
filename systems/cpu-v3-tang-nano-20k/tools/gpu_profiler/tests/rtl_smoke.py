"""A/B/A and CE0 on an exported production GPU; explicit 20k-cycle/30s bounds."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from capture import run
from model import build_capture, write_report


def main():
    p = argparse.ArgumentParser(__doc__)
    p.add_argument("--rtl", type=Path, required=True)
    p.add_argument("--out", type=Path, required=True)
    p.add_argument("--primitive", type=Path)
    args = p.parse_args()
    root = args.out.resolve()
    root.mkdir(parents=True, exist_ok=False)
    primitive = args.primitive or Path(os.environ["GOWIN_HOME"]) / "IDE/simlib/gw2a/prim_sim.v"
    sources = [(args.rtl / name).resolve() for name in json.loads((args.rtl / "rtl.json").read_text())]
    sources.append(primitive.resolve())
    instructions = [0, (4 << 28) | 3, 7 << 28, 15 << 28] + [0] * 28
    lines = []
    for base in (0x1000, 0x2000):
        lines.append("@" + format(base // 8, "x"))
        lines.extend(f"{instructions[i] | (instructions[i+1] << 32):016x}" for i in range(0, 32, 2))
    memory = root / "commands.hex"
    memory.write_text("\n".join(lines) + "\n")
    manifest = dict(version=1, title="Production GPU A/B/A observer smoke", clock_hz=100_000_000,
                    max_cycles=20000, timeout_seconds=30, submissions=3, top="tb",
                    testbench=str(Path(__file__).with_name("rtl_smoke_tb.v").resolve()),
                    sources=[{"path": str(s), "sha256": hashlib.sha256(s.read_bytes()).hexdigest()} for s in sources],
                    inputs={"commands.hex": str(memory)},
                    submission_labels={str(i): {"frame_id": i, "name": name}
                                       for i, name in enumerate(("A", "B", "A reused"), 1)})
    path = root / "manifest.json"
    path.write_text(json.dumps(manifest, indent=2))
    run(path, root / "capture")
    data = build_capture(root / "capture")
    assert [s["base"] for s in data["submissions"]] == [4096, 8192, 4096]
    assert len(data["commands"]) == 12
    assert len({c["id"] for c in data["commands"]}) == 12
    assert [c["pc"] for c in data["commands"]] == [0, 4, 8, 12] * 3
    assert any(span["start"] < sub["end"] and sub["start"] < span["end"]
               for span in data["cpu"][1:] for sub in data["submissions"])
    write_report(data, root / "report")
    print("PASS production GPU: A/B/A, repeated PC, CE0 SEND and CPU marker overlap")


if __name__ == "__main__":
    main()
