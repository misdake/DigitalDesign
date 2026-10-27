"""Compare a cold replay with the next frozen lease's confirmed old Front/depth."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import sys

sys.dont_write_bytecode = True
from snapshot import Abi, DEFAULT_ABI, SnapshotError, load_snapshot
from layout import ROW, declared_layout, header_layout


def compare(previous, following, replay, output):
    a, b = previous.header, following.header
    layout = header_layout(a)
    if layout != header_layout(b):
        raise SnapshotError('Adjacent leases use different storage layouts')
    if (a["manifest_id"] != b["manifest_id"] or b["submission"] != a["submission"] + 1 or
            b["token"] == a["token"] or a["color_base"] != b["front_base"] or
            a["depth_base"] != b["depth_base"] or a["target_format"] != b["front_format"]):
        raise SnapshotError("Snapshots are not adjacent leases with the prior output retired into Front")
    replay, output = Path(replay), Path(output)
    metadata = json.loads((replay / "replay.json").read_text())
    if metadata.get('command_abi',1) != 1:
        raise SnapshotError('Replay command ABI does not describe a supported row/column combination')
    if (declared_layout(metadata['layout']) if 'layout' in metadata else ROW) != layout:
        raise SnapshotError('Replay metadata and frozen leases use different layouts')
    if metadata.get("complete") is not True or metadata["input_sha256"] != previous.manifest()["input_sha256"]:
        raise SnapshotError("Replay does not belong to the captured input")
    report = dict(version=1, previous_token=a["token"], next_token=b["token"],
                  layout=layout.description(),
                  input_sha256=metadata["input_sha256"], planes={}, equal=True,
                  scope="Raw tiled RGB565/D16 memory comparison; not HDMI or board timing qualification")
    for role, base in [("color", b["front_base"]), ("depth", b["depth_base"])]:
        expected = following.read(base, b["plane_bytes"])
        actual = (replay / "capture" / (role + ".bin")).read_bytes()
        if len(actual) != len(expected) or hashlib.sha256(actual).hexdigest() != metadata[role + "_sha256"]:
            raise SnapshotError("Missing, truncated or modified replay output")
        differences = []
        maximum, count, padding = 0, 0, 0
        for index, (x, y) in enumerate(zip(struct.iter_unpack("<H", expected), struct.iter_unpack("<H", actual))):
            if x != y:
                coordinates = layout.coordinates(index * 2)
                if coordinates is None:
                    padding += 1
                    continue
                count += 1
                maximum = max(maximum, abs(x[0] - y[0]))
                if len(differences) < 16:
                    differences.append(dict(x=coordinates[0], y=coordinates[1],
                                            board_word=x[0], replay_word=y[0]))
        report["planes"][role] = dict(equal=count == 0, different_pixels=count, total_pixels=96000,
                                      physical_bytes=layout.plane_bytes,
                                      different_padding_words=padding,
                                      maximum_word_delta=maximum, examples=differences,
                                      board_sha256=hashlib.sha256(expected).hexdigest(),
                                      replay_sha256=hashlib.sha256(actual).hexdigest())
        guard = 0
        if layout.abi == 2:
            span=metadata.get('captured_plane_bytes', layout.plane_bytes)
            if span not in (layout.plane_bytes,layout.allocation_bytes):
                raise SnapshotError('Unknown compared allocation span')
            allocation = (replay / 'capture' / (role + '-allocation.bin')).read_bytes()
            if (len(allocation) != span or
                    hashlib.sha256(allocation).hexdigest() != metadata.get(role + '_allocation_sha256') or
                    allocation[:layout.plane_bytes] != actual):
                raise SnapshotError('Missing or modified full column allocation dump')
            expected_allocation = following.read(base, span)
            guard = sum(x != y for x, y in zip(expected_allocation[layout.plane_bytes:],
                                               allocation[layout.plane_bytes:]))
        report['planes'][role]['different_guard_bytes'] = guard
        report['planes'][role]['compared_guard_bytes'] = max(0,metadata.get('captured_plane_bytes',layout.plane_bytes)-layout.plane_bytes)
        report['planes'][role]['equal'] = count == padding == guard == 0
        report["equal"] &= report['planes'][role]['equal']
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
