"""Assemble the optional meshlet prologue into unused shared control ROM rows.

Vertex/triangle words are retained verbatim. Native and portable images must
agree; run this file with --check to audit without changing the ROM.
"""
from pathlib import Path
import argparse
import re

FIELDS = dict(l=(0, 2), r=(2, 2), u=(4, 5), swap=(11, 1), up=(12, 2),
              cfg=(15, 2), dst=(17, 4), mac=(21, 4), write=(29, 2),
              mul=(31, 1), acc=(32, 2), up_hi=(50, 1), minimum=(70, 1), check=(71, 1))


def program():
    rows = []

    def emit(**fields):
        if 'up' in fields:
            fields['up_hi'] = fields['up'] >> 2
            fields['up'] &= 3
        value = 0
        for key, item in fields.items():
            lo, width = FIELDS[key]
            assert 0 <= item < 1 << width
            value |= item << lo
        rows.append(value)

    def dot(row, minimum=0):
        emit(l=1, r=1, u=row)
        emit(up=6, cfg=0, minimum=minimum, l=1, r=1, u=row+1)
        emit(write=1, dst=1, minimum=minimum, up=6, cfg=1, swap=1, l=1, r=1, u=row+2)
        emit(write=1, dst=2, minimum=minimum, up=6, cfg=2, l=1, r=1, u=row)
        emit(write=1, dst=3, mul=1, mac=1, swap=1, l=1, r=1, u=row+1)
        emit(acc=1, mul=1, mac=2, l=1, r=1, u=row+2)
        emit(acc=2, mul=1, mac=3, swap=1, l=1, r=1, u=row+3)
        emit(acc=2, mul=1, mac=0, u=row+3)
        emit(acc=2)

    def add_z(register, check):
        emit(r=1, u=21)  # Existing Instance word21 = 65536.
        emit(mul=1, mac=register, u=21, minimum=int(check == 2))
        emit(acc=2)
        emit(check=1, cfg=check)

    dot(8)                              # Camera Z maximum.
    emit(check=1, cfg=0, write=2, dst=10)
    dot(0)                              # Camera X maximum, then Xmax + Zmax.
    add_z(10, 1)
    dot(0, minimum=1)                   # Camera X minimum, then Xmin - Zmax.
    add_z(10, 2)
    emit(check=1, cfg=3)
    assert len(rows) == 37
    # Min-corner capture and the -1 constant issue must not share a MAC edge.
    for word in rows:
        if word >> 70 & 1 and word >> 31 & 1:
            assert word >> 29 & 3 == 0 and word >> 12 & 3 == 0 and word >> 50 & 1 == 0
    return rows


def render(source):
    images = [[], []]
    for _, compact, regular in re.findall(r"mem\[(\d+)\]=SP_COMPACT \? 72'h([0-9a-f]+) : 72'h([0-9a-f]+);", source):
        images[0].append(int(compact, 16))
        images[1].append(int(regular, 16))
    assert all(len(image) == 512 for image in images)
    for image in images:
        rows=image[:7]+program()
        image[256:320] = rows+[0]*(64-len(rows))
    def portable(match):
        index = int(match[1])
        return f"mem[{index}]=SP_COMPACT ? 72'h{images[0][index]:018x} : 72'h{images[1][index]:018x};"
    result = re.sub(r"mem\[(\d+)\]=SP_COMPACT \? 72'h[0-9a-f]+ : 72'h[0-9a-f]+;", portable, source)
    for bank in range(2):
        for group in range(32, 40):
            init = [sum(((image[group*8+k] >> (bank*36)) & ((1 << 36)-1)) << (k*36)
                        for k in range(8)) for image in images]
            pattern = rf"\.INIT_RAM_{group:02X}\(SP_COMPACT \? 288'h[0-9a-f]+ : 288'h[0-9a-f]+\)"
            replacement = f".INIT_RAM_{group:02X}(SP_COMPACT ? 288'h{init[0]:072x} : 288'h{init[1]:072x})"
            matches = list(re.finditer(pattern, result))
            assert len(matches) == 2
            match = matches[bank]
            result = result[:match.start()] + replacement + result[match.end():]
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    path = Path(__file__).with_name('control_rom.v')
    source = path.read_text()
    result = render(source)
    if args.check:
        assert result == source, 'Run bounds_program.py to update the ROM'
    else:
        path.write_text(result)
    print('Bounds ROM: rows256..299, 44 words; vertex/triangle rows unchanged')
