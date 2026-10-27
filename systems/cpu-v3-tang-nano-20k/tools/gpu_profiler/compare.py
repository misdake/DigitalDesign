"""Compare a cold replay with the next frozen lease's confirmed old Front/depth."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import sys

sys.dont_write_bytecode = True
from snapshot import Abi, DEFAULT_ABI, SnapshotError, load_snapshot


def compare(previous, following, replay, output):
    a, b = previous.header, following.header
    if (a["manifest_id"] != b["manifest_id"] or b["submission"] != a["submission"] + 1 or
            b["token"] == a["token"] or a["color_base"] != b["front_base"] or
            a["depth_base"] != b["depth_base"] or a["target_format"] != b["front_format"]):
        raise SnapshotError("Snapshots are not adjacent leases with the prior output retired into Front")
    replay, output = Path(replay), Path(output)
    metadata = json.loads((replay / "replay.json").read_text())
    if metadata.get("complete") is not True or metadata["input_sha256"] != previous.manifest()["input_sha256"]:
        raise SnapshotError("Replay does not belong to the captured input")
    report = dict(version=1, previous_token=a["token"], next_token=b["token"],
                  input_sha256=metadata["input_sha256"], planes={}, equal=True,
                  scope="Raw tiled RGB565/D16 memory comparison; not HDMI or board timing qualification")
    for role, base in [("color", b["front_base"]), ("depth", b["depth_base"])]:
        expected = following.read(base, b["plane_bytes"])
        actual = (replay / "capture" / (role + ".bin")).read_bytes()
        if len(actual) != len(expected) or hashlib.sha256(actual).hexdigest() != metadata[role + "_sha256"]:
            raise SnapshotError("Missing, truncated or modified replay output")
        differences = []
        maximum = 0
        for index, (x, y) in enumerate(zip(struct.iter_unpack("<H", expected), struct.iter_unpack("<H", actual))):
            if x != y:
                maximum = max(maximum, abs(x[0] - y[0]))
                if len(differences) < 16:
                    tile, local = divmod(index, 256)
                    differences.append(dict(x=(tile % 25) * 16 + local % 16,
                                            y=(tile // 25) * 16 + local // 16,
                                            board_word=x[0], replay_word=y[0]))
        count = sum(expected[i:i + 2] != actual[i:i + 2] for i in range(0, len(actual), 2))
        report["planes"][role] = dict(equal=count == 0, different_pixels=count, total_pixels=96000,
                                      maximum_word_delta=maximum, examples=differences,
                                      board_sha256=hashlib.sha256(expected).hexdigest(),
                                      replay_sha256=hashlib.sha256(actual).hexdigest())
        report["equal"] &= count == 0
    if output.exists():
        raise SnapshotError("Comparison output already exists")
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    return report


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("previous", type=Path)
    parser.add_argument("following", type=Path)
    parser.add_argument("--replay", type=Path, required=True)
    parser.add_argument("--abi", type=Path, default=DEFAULT_ABI)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    abi = Abi(args.abi)
    result = compare(load_snapshot(args.previous, abi), load_snapshot(args.following, abi), args.replay, args.out)
    print(json.dumps(result, indent=2))
    if not result["equal"]:
        raise SystemExit(2)


if __name__ == "__main__":
    try:
        main()
    except (SnapshotError, OSError, KeyError, ValueError) as error:
        print("Comparison refused: " + str(error), file=sys.stderr)
        raise SystemExit(1)
