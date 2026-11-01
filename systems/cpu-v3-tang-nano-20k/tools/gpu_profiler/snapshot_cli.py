"""Capture/decode TX-only UART attempts, verify identities and prepare cold replay."""
import argparse
import json
from pathlib import Path
import subprocess
import sys
import time

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


def receive_uart(port, seconds, baud, out, abi):
    if not 1 <= seconds <= 1800 or baud != 115200:
        raise SnapshotError("Capture must be bounded 1..1800 seconds at the qualified 115200 baud")
    out = Path(out).resolve()
    out.mkdir(parents=True, exist_ok=False)
    raw, ready = out / "uart.bin", out / "recorder.ready"
    script = Path(__file__).resolve().parents[4] / "hardware/vendor/gowin/scripts/capture_bl616_uart.ps1"
    with (out / "transport.log").open("wb") as log:
        process = subprocess.Popen(["powershell", "-NoProfile", "-File", str(script), "-Port", port,
                                    "-Seconds", str(seconds), "-Baud", str(baud), "-Out", str(raw),
                                    "-WriteTiming", "-ReadyFile", str(ready)], stdout=log, stderr=subprocess.STDOUT)
        try:
            deadline = time.monotonic() + 30
            while not ready.is_file():
                if process.poll() is not None:
                    raise SnapshotError("Recorder stopped before ready; inspect transport.log")
                if time.monotonic() >= deadline:
                    raise SnapshotError("Recorder did not become ready within 30 seconds")
                time.sleep(0.05)
            if process.poll() is not None or not raw.is_file():
                raise SnapshotError("Recorder is not live; repeat before pressing dump")
            print("Recorder ready. Pause with Button 2, then dump with Button 1; keep paused until receive completes.",
                  flush=True)
            if process.wait(timeout=seconds + 60) != 0:
                raise SnapshotError("UART transport failed; preserve uart.bin and transport.log")
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
    return decode_file(raw, out / "decoded", abi)


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
    receive.add_argument("--rtl", type=Path, help="Replay each complete attempt through the shared cold backend")
    receive.add_argument("--backend", choices=['rtl', 'sim'], default='rtl')
    receive.add_argument("--simulator", type=Path)
    receive.add_argument("--max-work", type=int, default=64_000_000)
    receive.add_argument("--build", type=Path)
    receive.add_argument("--receipt", type=Path)
    receive.add_argument("--max-cycles", type=int)
    receive.add_argument("--timeout", type=int)
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
        if args.backend == 'rtl' and args.rtl is not None and (args.max_cycles is None or args.timeout is None or args.max_cycles <= 0 or args.timeout <= 0):
            raise SnapshotError("Automatic replay requires explicit --max-cycles and --timeout")
        if args.backend == 'rtl' and args.rtl is None and any(value is not None for value in (args.build, args.receipt, args.max_cycles, args.timeout, args.simulator)):
            raise SnapshotError("Replay options require --rtl")
        if args.backend == 'sim':
            if args.rtl is not None or args.max_cycles is not None or args.max_work < 1 or (args.timeout is not None and args.timeout <= 0):
                raise SnapshotError('Sim backend uses positive --max-work/--timeout, without RTL/cycle options')
            from simulator import executable_path
            executable_path(args.simulator)  # Fail before opening a physical port.
        report = receive_uart(args.port, args.seconds, args.baud, args.out, abi)
        if args.rtl is not None or args.backend == 'sim':
            from replay import run
            for attempt in report["attempts"]:
                run(args.out / "decoded" / attempt["directory"], args.rtl,
                    args.out / "replay" / attempt["directory"], args.max_cycles, args.timeout,
                    abi=abi, build=args.build, receipt=args.receipt, backend=args.backend,
                    simulator=args.simulator, maximum_work=args.max_work)
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
