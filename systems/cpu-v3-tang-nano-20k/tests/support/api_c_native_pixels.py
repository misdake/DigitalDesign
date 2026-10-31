"""Check bounded public UI/RTT fixtures' completed native output planes.

This checks target lifetime/content and representative nearest samples. Detailed
sampler arithmetic remains covered by the independent GPU RTT numerical tests.
"""
import argparse
import hashlib
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'tools/gpu_profiler'))
from layout import COLUMN
from snapshot import Abi, load_snapshot


def check_rtt(root):
    receipts = []
    previous = None
    for profile in ('release', 'debug'):
        folder = root / profile
        result = json.loads((folder / 'result.json').read_text())['execution']
        assert result['halted'] and result['submissions'] == 1
        assert result['confirmed_fronts'] == [0x220000]
        assert result['gate_publications'] == result['start_publications'] == [1]
        native = folder / 'native-0'
        handback = json.loads((native / 'memory-handback.json').read_text())
        assert handback['complete'] and 0 < handback['cycles'] <= 500_000
        planes = {}
        for row in handback['writes']:
            data = Path(row['file']).read_bytes()
            assert len(data) == row['bytes'] == COLUMN.plane_bytes
            assert hashlib.sha256(data).hexdigest() == row['sha256']
            planes[row['address']] = data
        assert set(planes) == {0x600000, 0x640000, 0x440000, 0x480000}
        if previous is not None:
            assert planes == previous, 'Debug profile changed rendered output'
        previous = planes
        source, source_depth, color, depth = [COLUMN.words(planes[base]) for base in
                                             (0x600000, 0x640000, 0x440000, 0x480000)]
        assert set(source_depth) == {767, 65535}
        assert any(value != 0xf800 for value in source)
        palette = set(source)
        samples = 0
        for y in range(240):
            for x in range(400):
                at = y * 400 + x
                inside = 175 <= x < 225 and 105 <= y < 135
                assert depth[at] == (255 if inside else 65535)
                assert color[at] in palette if inside else color[at] == 0
                samples += int(inside and color[at] != 0xf800)
        assert samples > 0, 'Consumer did not retain producer pattern contents'
        # Known 1/8-scale geometry: four corners and the centre use nearest
        # source coordinates (3 + 8*i, 3 + 8*j). This is a representative check,
        # not a claim of a new bit-exact interpolation oracle for every pixel.
        for x, y in [(175, 105), (224, 105), (175, 134), (224, 134), (200, 120)]:
            assert color[y * 400 + x] == source[(3 + 8 * (y - 105)) * 400 + 3 + 8 * (x - 175)]
        for base, data in planes.items():
            expected = 65535 if base in (0x640000, 0x480000) else 0xf800 if base == 0x600000 else 0
            for offset in range(0, len(data), 2):
                if COLUMN.coordinates(offset) is None:
                    assert int.from_bytes(data[offset:offset + 2], 'little') == expected
        receipts.append(dict(profile=profile, cycles=handback['cycles'],
                             consumer_pixels=1500, pattern_pixels=samples,
                             writable_bytes=sum(map(len, planes.values()))))
    return dict(complete=True, profiles=receipts,
                scope='Actual same-CB producer/consumer, full planes and representative nearest samples; cold GPU/MC, no physical CPU cache or Display scanout')


def check_ui(root):
    receipts = []
    previous = None
    for profile in ('release', 'debug'):
        folder = root / profile
        result = json.loads((folder / 'result.json').read_text())['execution']
        assert result['halted'] and result['submissions'] == 1
        assert result['confirmed_fronts'] == [0x220000]
        native = folder / 'native-0'
        capture = load_snapshot(native / 'frozen/snapshot.json', Abi())
        assert capture.header['has_ui'] and capture.read(capture.header['ui_base'], 1) == b'7'
        atlas_range, = [r for r in capture.ranges if r.tag == capture.abi['TAG_ATLAS']]
        atlas = capture.data[atlas_range.index]
        replay = json.loads((native / 'replay/replay.json').read_text())
        assert replay['complete'] and 0 < replay['cold_submission_cycles'] <= 500_000
        color = (native / 'replay/capture/color.bin').read_bytes()
        depth = (native / 'replay/capture/depth.bin').read_bytes()
        assert len(color) == len(depth) == COLUMN.plane_bytes
        assert depth == b'\xff\xff' * (COLUMN.plane_bytes // 2)
        expected = bytearray(b'\x11\x11' * (COLUMN.plane_bytes // 2))
        visible = 0
        for y in range(16):
            for x in range(8):
                # Native RGBA5551 atlas: odd rows exchange two-pixel pairs.
                at = ord('7') * 256 + y * 16 + (x ^ (2 if y & 1 else 0)) * 2
                word = int.from_bytes(atlas[at:at + 2], 'little')
                if word & 1:
                    green = (word >> 6) & 31
                    pixel = (word & 0xf800) | (((green << 1) | (green >> 4)) << 5) | ((word >> 1) & 31)
                    offset = COLUMN.offset(x, y)
                    expected[offset:offset + 2] = pixel.to_bytes(2, 'little')
                    visible += 1
        assert visible > 0 and color == expected, 'Native UI differs from the independent atlas oracle'
        if previous is not None:
            assert previous == (color, depth)
        previous = (color, depth)
        receipts.append(dict(profile=profile, cycles=replay['cold_submission_cycles'],
                             glyph_pixels=visible, checked_bytes=len(color) + len(depth)))
    return dict(complete=True, profiles=receipts,
                scope='Actual public UI capture and full Clear; independent RGBA5551 atlas oracle, complete physical planes; cold GPU/MC, no physical scanout')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('mode', choices=('rtt', 'ui'))
    parser.add_argument('result', type=Path)
    args = parser.parse_args()
    receipt = (check_rtt if args.mode == 'rtt' else check_ui)(args.result)
    (args.result / 'pixel-check.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(receipt))
