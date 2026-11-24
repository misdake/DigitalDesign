"""Assemble the billboard calendar into its own shared V/T ROM pages."""

import argparse
from pathlib import Path

from t32_program import read_image, render_image

BOOT = 320
KERNEL = 384
FIELDS = dict(l=(0, 2), r=(2, 2), u=(4, 5), swap=(11, 1), up=(12, 2),
              dst=(17, 4), mac=(21, 4), pack_rf=(25, 4), wr=(29, 2), mul=(31, 1),
              acc=(32, 2), pack=(35, 3), emit=(38, 2), flow=(40, 3),
              up_hi=(50, 1), keep=(70, 1))


def encode(target=None, **fields):
    word = 0
    for key, value in fields.items():
        shift, width = FIELDS[key]
        assert 0 <= value < 1 << width, (key, value)
        word |= value << shift
    if target is not None:
        assert 0 <= target < 512
        word |= (target & 127) << 43 | (target >> 7) << 51
    return word


def assemble():
    code, labels, branches = [], {}, {}

    def emit(target=None, **fields):
        if target is not None:
            branches[len(code)] = target
        code.append(fields)

    def issue(reg, swap, **fields):
        emit(mul=1, mac=reg, swap=swap, **fields)

    def vertex(parity):
        labels[f'v{parity}'] = KERNEL + len(code)
        # V96 parity alternates which scratchpad Q holds each word. Unpack XYZ
        # into RF1..3, signed offsets into RF4..5 and the attribute into RF10.
        # Issue the X dot in the order allowed by the live Qs and old RF values.
        if parity == 0:
            emit(l=2)
            emit(up=1, r=2)
            emit(wr=1, dst=1, l=1, u=0)
            issue(1, 1, wr=1, dst=2, l=1, u=2)
            emit(wr=1, dst=3, up=2, swap=1, acc=1)
            issue(3, 1, wr=1, dst=4, r=1, u=1)
            issue(2, 0, wr=1, dst=5, acc=2, l=2, r=1, u=3)
            issue(0, 0, up=3, acc=2, l=1, u=24)
            issue(4, 1, wr=1, dst=10, r=1, u=25, acc=2)
            issue(5, 0, l=1, u=4, acc=2)
        else:
            emit(r=2)
            emit(up=1, swap=1, l=2)
            emit(wr=1, dst=1, r=1, u=3)
            issue(0, 0, wr=1, dst=2, r=1, u=1)
            issue(2, 0, wr=1, dst=3, up=2, acc=1, l=1, u=0)
            issue(1, 1, wr=1, dst=4, acc=2, l=1, u=2)
            issue(3, 1, wr=1, dst=5, acc=2, l=1, r=2, u=24)
            issue(4, 1, up=3, r=1, u=25, acc=2)
            issue(5, 0, wr=1, dst=10, l=1, u=4, acc=2)

        # Compute Y/Z with the same MAC while capturing and emitting the
        # previous complete component via RF7 and the ordinary result packer.
        terms = []
        for row in (1, 2):
            terms.extend((4 * row + col, rf) for col, rf in enumerate((1, 2, 3, 0)))
            terms.extend(((24 + 2 * row, 4), (25 + 2 * row, 5)))
        for i, (coefficient, rf) in enumerate(terms):
            phase = i % 6
            fields = dict(acc=1 if phase == 1 else 2)
            if i + 1 < len(terms):
                next_coefficient = terms[i + 1][0]
                fields['r' if next_coefficient % 2 else 'l'] = 1
                fields['u'] = next_coefficient
            if phase == 1:
                fields.update(wr=2, dst=7)
            if phase == 2:
                fields.update(pack=1, pack_rf=7)
            if phase == 3:
                fields['emit'] = 1
            issue(rf, int(coefficient % 2 == 0), **fields)

        emit(acc=2)
        emit(wr=2, dst=7)
        emit(pack=1, pack_rf=7)
        emit(emit=1, pack=1, pack_rf=12)  # RF12 supplies the unlit N36.
        emit(emit=2, pack=1, pack_rf=10)
        emit(emit=1, flow=2, target='halt')
        emit(flow=3, target=f'v{1 - parity}')

    vertex(0)
    vertex(1)
    labels['halt'] = KERNEL + len(code)
    emit(flow=4)
    assert len(code) <= 64
    return [encode(labels[branches[i]] if i in branches else None, **fields)
            for i, fields in enumerate(code)]


def render(source):
    image = read_image(source)
    # Load RF0 and the position decode config with the original bootstrap.
    # The final keep retires into the same attribute epoch handoff as Bounds.
    bootstrap = [image[i][0] for i in range(7)] + [
        # The resident World calendar supplies only the matrix and basis.
        # Form the unlit normal directly, independent of reserved uniform words.
        encode(), encode(up_hi=1),
        encode(KERNEL, wr=1, dst=12, flow=7, keep=1),
    ]
    for start, words in ((BOOT, bootstrap), (KERNEL, assemble())):
        for address in range(start, start + 64):
            word = words[address - start] if address - start < len(words) else 0
            image[address] = (word, word)
    return render_image(source, image)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    path = Path(__file__).with_name('control_rom.v')
    source = path.read_text()
    result = render(source)
    if args.check:
        assert result == source, 'Regenerate both ROM views with billboard_program.py'
    else:
        path.write_text(result)
