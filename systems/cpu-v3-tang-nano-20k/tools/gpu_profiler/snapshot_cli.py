"""Capture/decode TX-only UART attempts, verify identities and prepare cold replay."""
import argparse
import json
from pathlib import Path
import subprocess
import sys

sys.dont_write_bytecode = True
from snapshot import Abi, DEFAULT_ABI, SnapshotError, decode_stream, load_snapshot
from identity import create_build, preflight


def decode_file(source, output, abi):
    source, output = Path(source).resolve(), Path(output).resolve()
    if output.exists() and any(output.iterdir()):
        raise SnapshotError("Decode output must be new/empty")
    output.mkdir(parents=True, exist_ok=True)
    # A bounded serial capture can contain console text and several retries.
    if source.stat().st_size > 128 * 1024 * 1024:
        raise SnapshotError("Raw capture exceeds the 128-MiB bound")
    completed, report = decode_stream(source.read_bytes(), abi)
    report["source"] = str(source)
    report["attempts"] = []
    for index, capture in enumerate(completed):
        name = f"complete-{index:03d}-s{capture.snapshot}-a{capture.attempt}"
        metadata = capture.save(output / name)
        report["attempts"].append(dict(directory=name, snapshot=capture.snapshot,
                                       attempt=capture.attempt, input_sha256=metadata["input_sha256"],
                                       decoded_bytes=capture.total, board_dump_ms=capture.elapsed_ms))
    (output / "receive.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(dict(complete=len(completed), failed=len(report["failures"]),
                         ignored_bytes=report["ignored_bytes"], report=str(output / "receive.json"))))
    if not completed:
        raise SnapshotError("No complete valid attempt; preserve raw capture and repeat while paused")
    return report


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--abi", type=Path, default=DEFAULT_ABI)
    commands = parser.add_subparsers(dest="command", required=True)
    decode = commands.add_parser("decode")
    decode.add_argument("raw", type=Path)
    decode.add_argument("--out", type=Path, required=True)
    receive = commands.add_parser("receive", help="BL616 route/capture; never sends a resume to the GPU")
    receive.add_argument("--port", required=True)
    receive.add_argument("--seconds", type=int, required=True)
    receive.add_argument("--out", type=Path, required=True)
    receive.add_argument("--baud", type=int, default=115200)
    check = commands.add_parser("preflight")
    check.add_argument("snapshot", type=Path)
    check.add_argument("--build", type=Path, required=True)
    check.add_argument("--receipt", type=Path, required=True)
    check.add_argument("--out", type=Path, required=True)
    build = commands.add_parser("build-manifest")
    build.add_argument("spec", type=Path)
    build.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "build-manifest":
        value = create_build(args.spec, args.out)
        print(json.dumps(dict(manifest_id=value["manifest_id"], out=str(args.out.resolve()))))
        return
    abi = Abi(args.abi)
    if args.command == "decode":
        decode_file(args.raw, args.out, abi)
    elif args.command == "receive":
        if not 1 <= args.seconds <= 1800 or args.baud != 115200:
            raise SnapshotError("Capture must be bounded 1..1800 seconds at the qualified 115200 baud")
        out = args.out.resolve()
        out.mkdir(parents=True, exist_ok=False)
        raw = out / "uart.bin"
        script = Path(__file__).resolve().parents[4] / "hardware/vendor/gowin/scripts/capture_bl616_uart.ps1"
        with (out / "transport.log").open("wb") as log:
            subprocess.run(["powershell", "-NoProfile", "-File", str(script), "-Port", args.port,
                            "-Seconds", str(args.seconds), "-Baud", str(args.baud), "-Out", str(raw)],
                           stdout=log, stderr=subprocess.STDOUT, check=True, timeout=args.seconds + 90)
        decode_file(raw, out / "decoded", abi)
    elif args.command == "preflight":
        result = preflight(load_snapshot(args.snapshot, abi), args.build, args.receipt)
        if args.out.exists():
            raise SnapshotError("Preflight output already exists")
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
        print(json.dumps(dict(input_sha256=result["input_sha256"], out=str(args.out.resolve()))))


if __name__ == "__main__":
    try:
        main()
    except (SnapshotError, OSError, KeyError, TypeError, ValueError, subprocess.SubprocessError) as error:
        print("Snapshot refused: " + str(error), file=sys.stderr)
        raise SystemExit(1)
