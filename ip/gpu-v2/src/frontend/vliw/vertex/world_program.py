"""Assemble resident View/World preparation without changing the Vertex calendar."""
import argparse
from pathlib import Path

from t32_program import read_image, render_image

FIELDS = {
    'l': (0, 2), 'r': (2, 2), 'u': (4, 5), 'swap': (11, 1),
    'up': (12, 2), 'dst': (17, 4), 'mac': (21, 4), 'wr': (29, 2),
    'mul': (31, 1), 'acc': (32, 2), 'flow': (40, 3),
    'view': (61, 1), 'store': (62, 1), 'index': (63, 5),
    'billboard': (68, 1), 'round': (70, 1), 'finish': (71, 1),
}


def address(tick):
    return 192 + tick if tick < 64 else 448 + tick - 64


def assemble():
    events = {}

    def event(tick, **fields):
        slot = events.setdefault(tick, {})
        for name, value in fields.items():
            assert name not in slot or slot[name] == value, (tick, name)
            slot[name] = value

    def read(tick, offset, view):
        event(tick, l=1, r=1, u=offset, view=int(view))

    # RF0 holds homogeneous one. Each View row then occupies RF1..4 until
    # its last World-column product consumes the old RF values.
    read(0, 12, False)
    event(1, up=3, swap=0)
    event(2, wr=1, dst=0)
    start = 1
    for normal in (False, True):
        for row in range(3):
            first = 12 + 3 * row if normal else 4 * row
            components = 3 if normal else 4
            read(start, first, True)
            for k in range(components):
                tick = start + k + 1
                offset = first + k
                event(tick, up=3, swap=offset & 1)
                event(tick + 1, wr=1, dst=k + 1)
                if k + 1 < components and (offset + 1) // 2 != offset // 2:
                    read(tick, offset + 1, True)
            issue = start + components + 1
            for column in range(3 if normal else 4):
                terms = [(k + 1, 16 + 3 * k + column if normal else 4 * k + column)
                         for k in range(3)]
                if not normal and column == 3:
                    terms.append((4, 12))
                for k, (reg, offset) in enumerate(terms):
                    read(issue - 1, offset, False)
                    event(issue, mul=1, mac=reg, swap=int(offset % 2 == 0))
                    event(issue + 1, acc=1 if k == 0 else 2)
                    issue += 1
                index = 12 + 3 * row + column if normal else 4 * row + column
                event(issue + 1, store=1, index=index)
            # Read the next View row while the previous row's final product
            # consumes the independent, already-held input Q.
            start = issue - 1

    end = max(events) + 1
    read(end, 12, False)
    event(end + 1, mul=1, mac=0, swap=1)
    event(end + 2, acc=1)
    event(end + 3, store=1, index=21)
    # Hide both intensity loads in the final normal dot's drain. Synchronous Q
    # is consumed before the next read replaces it; RF5/6 are free throughout.
    read(end - 3, 28, True)
    event(end - 2, up=3, swap=0)
    event(end - 1, up=3, swap=1, wr=1, dst=5)
    event(end, wr=1, dst=6)
    # Pipeline the two products behind the homogeneous-one product. The existing
    # ACC clear path supplies just bit23 for exact half-up Q8 rounding.
    read(end + 1, 13, False)
    event(end + 2, mul=1, mac=5, swap=0)
    read(end + 2, 14, False)
    event(end + 3, mul=1, mac=6, swap=1, acc=1, round=1)
    event(end + 4, store=1, index=24, acc=1, round=1)
    event(end + 5, store=1, index=25, finish=1)
    # Position11 is committed before selecting the unlit Billboard continuation.
    # Both continuations are aligned so facing selects just address bit4.
    event(54, billboard=1, target=482)
    event(63, flow=1, target=448)
    assert max(events) + 1 == 97
    assert sorted(v['index'] for v in events.values() if v.get('store')) == list(range(22)) + [24, 25]

    code = []
    for tick in range(97):
        word = 0
        for name, value in events.get(tick, {}).items():
            if name == 'target':
                word |= (value & 127) << 43 | (value >> 7) << 51
            else:
                shift, width = FIELDS[name]
                assert 0 <= value < 1 << width, (name, value)
                word |= value << shift
        code.append(word)
    return code


def render(source):
    image = read_image(source)
    for tick in range(128):
        at = address(tick)
        image[at] = (0, image[at][1])
    for tick, word in enumerate(assemble()):
        at = address(tick)
        image[at] = (word, image[at][1])
    for vertical in (False, True):
        # World12=one, World25=zero. All camera-dependent basis coefficients
        # belong to View; the old Billboard bootstrap reads the derived basis.
        values = [(12, False, 21),
                  (22 if vertical else 31, True, 24), (25, False, 25),
                  (23, True, 26) if vertical else (25, False, 26),
                  (25, False, 27) if vertical else (31, True, 27),
                  (30, True, 28) if vertical else (25, False, 28),
                  (25, False, 29)]
        code = [0] * 10
        for i, (offset, view, index) in enumerate(values):
            code[i] |= 1 | (1 << 2) | (offset << 4) | (int(view) << 61)
            code[i+1] |= (1 << 31) | (int(offset % 2 == 0) << 11)
            code[i+2] |= 1 << 32
            code[i+3] |= (1 << 62) | (index << 63)
        code[-1] |= 1 << 71
        for i, word in enumerate(code):
            at = 482 + 16 * int(vertical) + i
            image[at] = (word, image[at][1])
    return render_image(source, image)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    path = Path(__file__).with_name('control_rom.v')
    source = path.read_text()
    result = render(source)
    if args.check:
        assert result == source, 'Regenerate both ROM views with world_program.py'
    else:
        path.write_text(result)
    print('World ROM: 97 enabled edges, 22 matrix + 2 light-response writes; legacy V/T unchanged')
