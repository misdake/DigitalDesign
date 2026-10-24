"""Assemble the serial meshlet prologue without changing any V/T control word."""
from pathlib import Path
import argparse
import re

FIELDS = dict(l=(0, 2), r=(2, 2), u=(4, 5), cfgoff=(9, 2), swap=(11, 1),
              up=(12, 2), cfg=(15, 2), dst=(17, 4), mac=(21, 4), write=(29, 2),
              mul=(31, 1), acc=(32, 2), flow=(40, 3), targetlo=(43, 7),
              uphi=(50, 1), targethi=(51, 2), minimum=(70, 1), corner=(71, 1), check=(51, 1),
              pack=(35, 3), rfpack=(25, 4), worken=(53, 1),
              workwe=(54, 1), workaddr=(55, 6))


def program(bootstrap):
    rows = list(bootstrap)

    def emit(**fields):
        if 'up' in fields:
            fields['uphi'] = fields['up'] >> 2
            fields['up'] &= 3
        if 'target' in fields:
            fields['targetlo'] = fields['target'] & 127
            fields['targethi'] = fields.pop('target') >> 7
        word = 0
        for name, value in fields.items():
            low, width = FIELDS[name]
            assert 0 <= value < 1 << width
            word |= value << low
        rows.append(word)

    # Range taps2/7/12 check signed20 coefficients, signed25 origins and
    # signed30 translation. Only these scalar taps feed the safety latch.
    for dimension in range(3):
        emit(l=3, r=3, cfgoff=dimension >> 1)
        emit(up=3, swap=dimension & 1)
        emit(check=1, target=2)

    def dot(row, minimum=0, prepare_negative_z=False):
        emit(l=1, r=1, u=row,
             **(dict(pack=6, rfpack=10) if prepare_negative_z else {}))
        emit(up=1, corner=1, cfg=0, minimum=minimum, l=1, r=1, u=row+1)
        # The old right Q supplies Y on this edge. Do not also read the
        # work row being written; only the independent left read is needed.
        emit(write=1, dst=1, up=1, corner=1, cfg=1, minimum=minimum, swap=1,
             l=1, r=0 if prepare_negative_z else 1, u=row+2, target=1, check=1,
             **(dict(workwe=1, workaddr=0) if prepare_negative_z else {}))
        emit(write=1, dst=2, up=1, corner=1, cfg=2, minimum=minimum,
             l=1, r=1, u=row, target=1, check=1,
             **(dict(worken=1, workaddr=0) if prepare_negative_z else {}))
        # swap1 uses left Q for MAC B and right work Q for the unpacker.
        # RF3 consumes the previous endpoint while kind5 starts -Z.
        emit(write=1, dst=3, mul=1, mac=1, swap=1,
             l=1, r=1, u=row+1, check=1, target=1,
             **(dict(up=5) if prepare_negative_z else {}))
        emit(acc=1, mul=1, mac=2, l=1, r=1, u=row+2,
             **(dict(write=1, dst=11) if prepare_negative_z else {}))
        emit(acc=2, mul=1, mac=3, swap=1, l=1, r=1, u=row+3)
        emit(acc=2, mul=1, mac=0, u=row+3)
        emit(acc=2, up=3, swap=1)
        emit(check=1, target=4)

    def decision(kind=0, last=False, **fields):
        emit(flow=7, cfg=kind, minimum=int(last),
             target=7 if last else 256+len(rows)+1, **fields)

    def add_z(kind):
        emit(r=1, u=21)
        emit(mul=1, mac=11 if kind == 2 else 10, u=21)
        emit(acc=2)
        decision(kind, last=kind == 2)

    dot(8)
    decision(write=2, dst=10)
    dot(0, prepare_negative_z=True)
    add_z(1)
    dot(0, minimum=1)
    add_z(2)
    # The prologue runs alongside original V control. It must never emit V,
    # advance the vertex input or execute the V loop/end operations.
    assert all((w >> 38) & 3 == 0 and (w >> 40) & 7 in (0, 7)
               and w & 3 != 2 and (w >> 2) & 3 != 2 for w in rows)
    assert len(rows) == 55
    return rows


def render(source):
    images = [[], []]
    pattern = r"mem\[(\d+)\]=SP_COMPACT \? 72'h([0-9a-f]+) : 72'h([0-9a-f]+);"
    for _, compact, regular in re.findall(pattern, source):
        images[0].append(int(compact, 16))
        images[1].append(int(regular, 16))
    assert all(len(image) == 512 for image in images)
    # The regular T program occupies this address range; only the compact
    # meshlet composition has room for the bounds prologue.
    # V never used cfg_idx while pushing the unpacker. Direct ROM phase
    # selection must preserve that zero-phase contract. T forces phase zero.
    assert all((w >> 15) & 3 == 0 for w in images[0][:128]
               if ((w >> 12) & 3) or ((w >> 50) & 1))
    rows = program(images[0][:7])
    images[0][256:320] = rows+[0]*(64-len(rows))

    def portable(match):
        i = int(match[1])
        return f"mem[{i}]=SP_COMPACT ? 72'h{images[0][i]:018x} : 72'h{images[1][i]:018x};"

    result = re.sub(pattern, portable, source)
    for bank in range(2):
        for group in range(32, 40):
            values = [sum(((image[group*8+k] >> (bank*36)) & ((1 << 36)-1)) << (k*36)
                          for k in range(8)) for image in images]
            pattern = rf"\.INIT_RAM_{group:02X}\(SP_COMPACT \? 288'h[0-9a-f]+ : 288'h[0-9a-f]+\)"
            matches = list(re.finditer(pattern, result))
            assert len(matches) == 2
            m = matches[bank]
            text = f".INIT_RAM_{group:02X}(SP_COMPACT ? 288'h{values[0]:072x} : 288'h{values[1]:072x})"
            result = result[:m.start()]+text+result[m.end():]
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    path = Path(__file__).with_name('control_rom.v')
    source = path.read_text()
    result = render(source)
    if args.check:
        assert result == source, 'Run bounds_program.py to regenerate both ROM views'
    else:
        path.write_text(result)
    print('Bounds ROM: 55 words at256..310; V/T words unchanged')
