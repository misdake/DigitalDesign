"""Decode CRC-protected board records or capture a bounded UART session.

No board is accessed without --port. Saved captures can be decoded offline.
"""
import argparse
import csv
import json
import struct
import time
import zlib
from pathlib import Path

CLIENTS = ["display", "instruction", "data", "dma", "gpu_ro", "framebuffer_read", "framebuffer_write"]
MODES = ["gpu_read_row_hit", "gpu_write_row_hit", "gpu_read_sequential", "gpu_read_row_conflict", "gpu_display", "gpu_cpu", "gpu_cpu_display", "gpu_cpu_display_batch50", "saturation"]
FRAME_BYTES = 420


def decode(raw):
    records = []
    offset = 0
    while True:
        offset = raw.find(b"SDMC", offset)
        if offset < 0 or offset + FRAME_BYTES > len(raw):
            break
        frame = raw[offset:offset + FRAME_BYTES]
        words = struct.unpack("<105I", frame)
        if zlib.crc32(frame[:416]) != words[104]:
            raise ValueError(f"UART record CRC mismatch at byte {offset}")
        offset += FRAME_BYTES
        version, mode, failed = words[1] & 255, (words[1] >> 8) & 255, bool(words[1] >> 16)
        if version not in (1, 2) or mode >= len(MODES) or words[2] != 54_000_000:
            raise ValueError("unsupported board record configuration")
        if failed:
            raise ValueError(f"board pattern/protocol/watchdog failure in mode {mode}")
        window, elapsed, busy, read_bytes, write_bytes = words[3:8]
        if not window or elapsed < window or busy > elapsed or read_bytes + write_bytes > elapsed * 8:
            raise ValueError("invalid board accounting")
        record = dict(version=version, mode=mode, name=MODES[mode], clock_hz=words[2], window_cycles=window,
                      elapsed_cycles=elapsed, drain_cycles=elapsed-window, busy_cycles=busy,
                      occupancy_percent=100*busy/elapsed,
                      read_bytes=read_bytes, write_bytes=write_bytes,
                      useful_bandwidth_MB_s=(read_bytes+write_bytes)*words[2]/elapsed/1e6,
                      useful_bus_percent=100*(read_bytes+write_bytes)/(elapsed*8),
                      completed_512B_groups=words[8],
                      mean_group_cycles=(words[9] | words[10] << 32)/words[8] if words[8] else None,
                      max_group_cycles=words[11], missed_load_deadlines=words[12], clients=[])
        for i, name in enumerate(CLIENTS):
            v = words[13+i*13:26+i*13]
            requested, completed = v[:2]
            if requested != completed:
                raise ValueError(f"undrained {name} requests")
            sums = [v[n] | v[n+1] << 32 for n in [2, 4, 6]]
            client = dict(client=name, requests=requested, completed=completed,
                          mean_wait_cycles=sums[0]/completed if completed else None,
                          mean_first_cycles=sums[1]/completed if completed else None,
                          mean_complete_cycles=sums[2]/completed if completed else None,
                          max_wait_cycles=v[8], max_first_cycles=v[9], max_complete_cycles=v[10],
                          min_first_cycles=v[11] if completed else None,
                          min_complete_cycles=v[12] if completed else None)
            if completed and not (client["min_first_cycles"] <= client["mean_first_cycles"] <= client["max_first_cycles"]
                                  and client["min_complete_cycles"] <= client["mean_complete_cycles"] <= client["max_complete_cycles"]):
                raise ValueError(f"latency accounting for {name}")
            record["clients"].append(client)
        sizes = [32, 32, 32, 2, 128, 512 if version == 2 else 128, 512 if version == 2 else 128]
        if sum(c["completed"] * size for c, size in zip(record["clients"], sizes)) != read_bytes + write_bytes:
            raise ValueError("useful bytes disagree with completed client payloads")
        sectors = sum(record["clients"][i]["completed"] for i in [5, 6])
        if sectors != words[8] * (1 if version == 2 else 4):
            raise ValueError("framebuffer group accounting")
        records.append(record)
    if not records:
        raise ValueError("no complete valid CRC-protected records")
    return records


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--input", type=Path)
    source.add_argument("--port", help="explicit serial port; requires pyserial")
    parser.add_argument("--hex", action="store_true", help="input is the pin fixture's hex file")
    parser.add_argument("--seconds", type=float, default=30)
    parser.add_argument("--output", type=Path, default=Path("target/gpu-v2-sdram/board-capture"))
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    if args.port:
        if not 0 < args.seconds <= 120:
            parser.error("capture seconds must be 0..120")
        import serial
        raw = bytearray()
        start = time.monotonic()
        with serial.Serial(args.port, 115200, timeout=0.2) as port:
            while time.monotonic()-start < args.seconds and len(raw) < 1_048_576:
                raw.extend(port.read(4096))
        raw = bytes(raw)
        (args.output / "uart.bin").write_bytes(raw)
    else:
        raw = args.input.read_bytes()
        if args.hex:
            raw = bytes.fromhex(raw.decode("ascii"))
    records = decode(raw)
    (args.output / "records.json").write_text(json.dumps(records, indent=2) + "\n", encoding="utf-8")
    rows = [dict(mode=r["mode"], name=r["name"], **c) for r in records for c in r["clients"]]
    with (args.output / "latencies.csv").open("w", newline="", encoding="utf-8") as file:
        writer = csv.DictWriter(file, fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)
    for r in records:
        print(f'{r["mode"]} {r["name"]}: useful {r["useful_bus_percent"]:.2f}% / '
              f'{r["useful_bandwidth_MB_s"]:.2f} MB/s, occupied {r["occupancy_percent"]:.2f}%, '
              f'group mean {r["mean_group_cycles"]}, missed deadlines {r["missed_load_deadlines"]}')
    print(f"Decoded {len(records)} CRC-checked records into {args.output}")


if __name__ == "__main__":
    main()
