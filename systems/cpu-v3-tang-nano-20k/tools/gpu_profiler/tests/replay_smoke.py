"""Small cold transport/plane-preservation test, not a board rendering claim."""
import argparse
from pathlib import Path
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import snapshot as s
from replay import execute, prepare
from replay import physical
from capture import run as capture_run
from test_snapshot import fixture


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--abi", type=Path, default=s.DEFAULT_ABI)
    parser.add_argument("--rtl", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--missing-read", action="store_true")
    args = parser.parse_args()
    abi = s.Abi(args.abi)
    header, rows, blobs = fixture(abi)
    for tag, address, word in [("COLOR", 0x430000, b"\x34\x12"),
                               ("DEPTH", 0x460000, b"\xff\xff"),
                               ("FRONT", 0x400000, b"\x00\x00")]:
        row = [0] * abi["RANGE_WORDS"]
        row[abi["R_FLAGS"]] = abi["READ"] | abi["PREIMAGE"]
        row[abi["R_TAG"]] = abi["TAG_" + tag]
        row[abi["R_BASE"]:abi["R_BASE"] + 2] = [address & 65535, address >> 16]
        row[abi["R_BYTES"]:abi["R_BYTES"] + 2] = [192000 & 65535, 192000 >> 16]
        rows.append(row)
        blobs.append(word * 96000)
    header[abi["H_RANGE_COUNT"]] = len(rows)
    capture, report = s.decode_stream(s.encode_reference(header, rows, blobs, abi), abi)
    assert len(capture) == 1 and not report["failures"]
    path = prepare(capture[0], args.rtl, args.out, max_cycles=100_000, timeout=60)
    if args.missing_read:
        # A present data file is insufficient: poison a single byte's validity.
        with (args.out / "valid.hex").open("a", encoding="ascii") as stream:
            stream.write(f"@{physical(0x490000 // 4):x}\ne\n")
        try:
            capture_run(path, args.out / "capture")
        except RuntimeError:
            log = (args.out / "capture/run.log").read_text(errors="replace")
            assert "Uncaptured/uninitialized read" in log, log[-500:]
            print("PASS cold replay rejects a missing byte before consuming the native read")
            return
        raise AssertionError("Missing input unexpectedly reached a complete replay")
    result = execute(capture[0], path)
    assert (args.out / "capture/color.bin").read_bytes() == b"\x34\x12" * 96000
    assert (args.out / "capture/depth.bin").read_bytes() == b"\xff\xff" * 96000
    print("PASS cold production GPU+CL3 MC: NOP/WAIT/FLUSH/END, untouched planes, initialized reads", result)


if __name__ == "__main__":
    main()
