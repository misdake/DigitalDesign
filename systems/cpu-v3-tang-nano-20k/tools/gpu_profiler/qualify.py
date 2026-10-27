"""Summarize one bound board/replay run, without historical ledgers or raw logs."""
import argparse
import csv
from datetime import datetime
import hashlib
import json
from pathlib import Path
import re
import sys

sys.dont_write_bytecode = True
from compare import compare
from identity import digest, preflight
from snapshot import Abi, DEFAULT_ABI, SnapshotError, encode_reference, load_snapshot


def qualify(folder, output, abi):
    folder, output = Path(folder).resolve(), Path(output).resolve()
    if output.exists():
        raise SnapshotError("Qualification output already exists")
    received = json.loads((folder / "decoded/receive.json").read_text())
    if len(received["attempts"]) != 3 or received["failures"]:
        raise SnapshotError("Qualification requires three independent complete, uncorrupted attempts")
    captures = [load_snapshot(folder / "decoded" / item["directory"], abi) for item in received["attempts"]]
    first, repeated, following = captures
    if (first.manifest()["input_sha256"] != repeated.manifest()["input_sha256"] or
            first.snapshot != repeated.snapshot or first.attempt == repeated.attempt):
        raise SnapshotError("Repeated paused dump does not preserve the same frozen input")
    identity = preflight(first, folder / "build.json", folder / "receipt.json")
    for capture in captures[1:]:
        if capture.header["manifest_id"] != identity["manifest_id"]:
            raise SnapshotError("Capture build changed during qualification")
    hardware = json.loads((folder / "hardware-identity.json").read_text())
    if hardware.get("complete") is not True or len(hardware["files"]) != 32:
        raise SnapshotError("Missing complete controlled hardware-export comparison")
    raw = (folder / "device/uart.bin").read_bytes()
    if hashlib.sha256(raw).hexdigest() != received["raw_sha256"]:
        raise SnapshotError("Raw receive evidence changed after decoding")
    timings = list(csv.DictReader((folder / "device/uart.bin.timing.csv").open(newline="")))
    offset = 0
    for row in timings:
        if int(row["offset"]) != offset or int(row["count"]) <= 0:
            raise SnapshotError("Noncontiguous byte-arrival evidence")
        offset += int(row["count"])
    if offset != len(raw):
        raise SnapshotError("Arrival evidence does not cover the raw capture")
    transfers = []
    for capture in captures:
        encoded = encode_reference(capture.descriptor, [r.words for r in capture.ranges],
            [bytes(capture.data[r.index]) for r in capture.ranges], abi,
            attempt=capture.attempt, elapsed_ms=capture.elapsed_ms)
        at = raw.find(encoded)
        if at < 0 or raw.find(encoded, at + 1) >= 0:
            raise SnapshotError("Firmware bytes do not uniquely match the independent canonical encoder")
        endpoints = []
        for point in [at, at + len(encoded) - 1]:
            row = next(r for r in timings if int(r["offset"]) <= point < int(r["offset"]) + int(r["count"]))
            # .NET emits seven fractional digits; Python3.10 accepts six.
            stamp = re.sub(r"(\.\d{6})\d+(?=[+-]|Z|$)", r"\1", row["utc"])
            endpoints.append(datetime.fromisoformat(stamp.replace("Z", "+00:00")))
        span = (endpoints[1] - endpoints[0]).total_seconds()
        if span <= 0:
            raise SnapshotError("Unusable UART arrival time span")
        transfers.append(dict(token=capture.snapshot, attempt=capture.attempt,
                              decoded_bytes=capture.total, wire_bytes=len(encoded),
                              board_dump_ms=capture.elapsed_ms, arrival_span_seconds=span,
                              observed_wire_bytes_per_second=len(encoded) / span,
                              nominal_8N1_line_fraction=len(encoded) / span / 11520,
                              independent_encoder_exact=True))
    profile = json.loads((folder / "replay/profile/profile.json").read_text())
    if profile["partial"] or len(profile["submissions"]) != 1:
        raise SnapshotError("Incomplete or unexpected replay session")
    submission = profile["submissions"][0]
    total = submission["end"] - submission["start"]
    clears = [c for c in profile["commands"] if c["opcode"] == 8]
    clear_cycles = sum(c["end"] - c["start"] for c in clears)
    comparison = compare(first, following, folder / "replay", output.with_suffix(".planes.json"))
    report = dict(schema="digitaldesign.gpu-capture-qualification", version=1,
                  complete=comparison["equal"], manifest_id=identity["manifest_id"],
                  input_sha256=identity["input_sha256"], controlled_hardware_files=32,
                  bitstream_sha256=hardware["bitstream_sha256"],
                  firmware=json.loads((folder / "receipt.json").read_text())["firmware"],
                  repeated_frozen_inputs_equal=True, independent_wire_encoder_exact=True,
                  stimulus="explicit automatic software test; physical buttons reported separately",
                  transfers=transfers, output_comparison=comparison["planes"],
                  cold_replay=dict(clock_hz=profile["clock_hz"], accepted_start_to_drained_cycles=total,
                                   observed_clear_command_cycles=clear_cycles,
                                   cycles_outside_observed_clear_commands=total - clear_cycles,
                                   commands=len(profile["commands"]), meshlets=len(profile["meshlets"]),
                                   metrics=profile["metric_defs"],
                                   note="Clear subtraction is accounting on this run, not a separate no-clear render"),
                  scope="Cold production GPU+MC, no CPU/Display contention or warm state; functional pins, not PVT",
                  artifacts=dict(profile=str(folder / "replay/profile/profiler.html"),
                                 image=str(folder / "replay/color.png"),
                                 hdmi=str(folder / "device/hdmi-corrected.png")))
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    return report


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("folder", type=Path)
    parser.add_argument("--abi", type=Path, default=DEFAULT_ABI)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    result = qualify(args.folder, args.out, Abi(args.abi))
    summary = {k: result[k] for k in ["complete", "manifest_id", "input_sha256"]}
    summary["cold_replay"] = {k: v for k, v in result["cold_replay"].items() if k != "metrics"}
    print(json.dumps(summary, indent=2))
    if not result["complete"]:
        raise SystemExit(2)


if __name__ == "__main__":
    try:
        main()
    except (SnapshotError, OSError, KeyError, TypeError, ValueError, StopIteration) as error:
        print("Qualification refused: " + str(error), file=sys.stderr)
        raise SystemExit(1)
