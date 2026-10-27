"""Bounded cold replay of verified frozen bytes on production GPU+streaming MC."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import struct
import sys
import zlib

sys.dont_write_bytecode = True
from capture import run as capture_run
from identity import digest, load_build, preflight
from model import build_capture, write_report
from snapshot import Abi, DEFAULT_ABI, SnapshotError, load_snapshot


def physical(logical_word):
    return (((logical_word >> 5) & 3) << 19) | (logical_word & 31) | ((logical_word >> 7) << 5)


def seed_files(capture, output):
    """Merge aliases by physical bytes, preserving validity of partial dwords."""
    words, masks = {}, {}
    for region in capture.ranges:
        blob = capture.data[region.index]
        for offset in range(0, len(blob), 2):
            address = region.address + offset
            index, shift = physical(address // 4), (address & 3) * 8
            value = blob[offset] | blob[offset + 1] << 8
            mask = 65535 << shift
            words[index] = (words.get(index, 0) & ~mask) | (value << shift)
            masks[index] = masks.get(index, 0) | (3 << (address & 3))
    for name, values, width in [("memory.hex", words, 8), ("valid.hex", masks, 1),
                                 ("marked.hex", {i: 1 for i in masks}, 1)]:
        with (output / name).open("w", encoding="ascii", newline="\n") as stream:
            previous = -2
            for address in sorted(values):
                if address != previous + 1:
                    stream.write(f"@{address:x}\n")
                stream.write(f"{values[address]:0{width}x}\n")
                previous = address


def preview(plane, output, linear):
    """Native tiled RGB565 -> lossless PNG preview. Raw plane remains authoritative."""
    if len(plane) != 192000:
        raise SnapshotError("Unexpected output plane length")
    rows = []
    for y in range(240):
        row = bytearray([0])
        for x in range(400):
            tile = (y // 16) * 25 + x // 16
            at = (tile * 256 + (y % 16) * 16 + x % 16) * 2
            word, = struct.unpack_from("<H", plane, at)
            channels = [(word >> 11) / 31, ((word >> 5) & 63) / 63, (word & 31) / 31]
            if linear:
                # Viewing aid only; exact output comparisons use the raw plane.
                channels = [12.92 * v if v <= 0.0031308 else 1.055 * v ** (1 / 2.4) - 0.055
                            for v in channels]
            row.extend(round(max(0, min(1, v)) * 255) for v in channels)
        rows.append(row)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xffffffff)

    Path(output).write_bytes(b"\x89PNG\r\n\x1a\n" +
        chunk(b"IHDR", struct.pack(">IIBBBBB", 400, 240, 8, 2, 0, 0, 0)) +
        chunk(b"IDAT", zlib.compress(b"".join(rows))) + chunk(b"IEND", b""))


def prepare(capture, rtl, output, max_cycles, timeout, preflight_result=None):
    """Prepare one isolated run; a test fixture may explicitly omit board identity."""
    rtl, output = Path(rtl).resolve(), Path(output).resolve()
    if not 0 < max_cycles <= 20_000_000 or not 1 <= timeout <= 3600:
        raise SnapshotError("Replay needs explicit cycle and wall-clock bounds")
    output.mkdir(parents=True, exist_ok=False)
    sources = [(rtl / name).resolve() for name in json.loads((rtl / "rtl.json").read_text())]
    combined = "\n".join(p.read_text(encoding="utf-8") for p in sources)
    for required in ["module GowinSdramCombination", ".CAS(3)", ".REGISTER_READ_RETURN(1)",
                     "module gpu_v2_vliw_command", "reg read_event=0,read_visible=0,read_seen=0",
                     "Ordered two-descriptor endpoint"]:
        if required not in combined:
            raise SnapshotError("Export is not the selected production GPU/stream CL3 MC: " + required)
    primitive = Path(os.environ["GOWIN_HOME"]) / "IDE/simlib/gw2a/prim_sim.v"
    if not primitive.is_file():
        raise SnapshotError("Required vendor primitive model is missing")
    seed_files(capture, output)
    template = Path(__file__).with_name("replay_tb.v").read_text(encoding="utf-8")
    header = capture.header
    for name, value in [("COUNT", header["count_minus_one"]), ("BASE_LINE", header["cb_base"] // 128),
                        ("COLOR_BASE", header["color_base"]), ("DEPTH_BASE", header["depth_base"])]:
        template = template.replace("@" + name + "@", str(value))
    bench = output / "replay-input-tb.v"
    bench.write_text(template, encoding="utf-8")
    manifest = dict(version=1, title="Frozen pre-submit cold GPU+MC replay", clock_hz=54_000_000,
                    max_cycles=max_cycles, timeout_seconds=timeout, submissions=1, top="tb",
                    finish_on_complete=False, testbench=str(bench),
                    sources=[dict(path=str(p), sha256=digest(p)) for p in sources + [primitive]],
                    inputs={name: str(output / name) for name in ["memory.hex", "valid.hex", "marked.hex"]},
                    input_sha256=capture.manifest()["input_sha256"], preflight=preflight_result,
                    functional_pin_model=dict(cas=3, tac_ns=6, toh_ns=2, fixed_return_delay_ns=4,
                                              sdram_phase_degrees=157.5,
                                              note="Functional replay assumptions; not measured PVT bounds"),
                    replay_scope="cold GPU+stream CL3 MC; no CPU/Display contention; functional pins",
                    submission_labels={"1": dict(frame_id=header["frame"], name="Frozen prepared CB")})
    path = output / "replay-manifest.json"
    path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    return path


def execute(capture, manifest_path):
    output = Path(manifest_path).parent
    capture_run(manifest_path, output / "capture")
    profile = build_capture(output / "capture")
    write_report(profile, output / "profile")
    log = (output / "capture/run.log").read_text(encoding="utf-8", errors="replace")
    match = re.search(r"REPLAY_COMPLETE cycles=(\d+) initialized_reads=checked", log)
    if not match:
        raise SnapshotError("Replay did not reach its complete output/initialized-read boundary")
    color, depth = [output / "capture" / (name + ".bin") for name in ["color", "depth"]]
    if any(p.stat().st_size != capture.header["plane_bytes"] for p in [color, depth]):
        raise SnapshotError("Replay has incomplete output planes")
    preview(color.read_bytes(), output / "color.png", capture.header["target_format"] == 1)
    result = dict(complete=True, input_sha256=capture.manifest()["input_sha256"],
                  cold_submission_cycles=int(match.group(1)), color_sha256=digest(color),
                  depth_sha256=digest(depth), profile=str(output / "profile/profiler.html"),
                  scope="No CPU/Display contention or warm cache; not board timing/PVT qualification")
    (output / "replay.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    return result


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("snapshot", type=Path)
    parser.add_argument("--abi", type=Path, default=DEFAULT_ABI)
    parser.add_argument("--rtl", type=Path, required=True)
    parser.add_argument("--build", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--max-cycles", type=int, required=True)
    parser.add_argument("--timeout", type=int, required=True)
    args = parser.parse_args()
    capture = load_snapshot(args.snapshot, Abi(args.abi))
    identity = preflight(capture, args.build, args.receipt)
    build = load_build(args.build)
    recorded = {entry["sha256"] for entry in build["files"]}
    for name in json.loads((args.rtl / "rtl.json").read_text()):
        if digest(args.rtl / name) not in recorded:
            raise SnapshotError("Unbound RTL export; generate a matching build declaration before firmware")
    path = prepare(capture, args.rtl, args.out, args.max_cycles, args.timeout, identity)
    print(json.dumps(execute(capture, path), indent=2))


if __name__ == "__main__":
    try:
        main()
    except (SnapshotError, OSError, KeyError, ValueError, RuntimeError) as error:
        print("Replay refused: " + str(error), file=sys.stderr)
        raise SystemExit(1)
