"""Current 5-row V / 16-item T transport over the frozen mathematical oracle.

Only packing changes here. The oracle still computes positions, normal MACs,
quantization, coverage, UV, Z and LOD independently of the candidate RTL.
"""
from pathlib import Path


def normal_word(words, normal18, math):
    raw = [words[0] & ((1 << 18)-1), words[0] >> 18, words[1]]
    values = [v-(1 << 18) if v & (1 << 17) else v for v in raw]
    if not normal18:
        values = [v*4 for v in values]
    return sum((math.normal12_from18(v) & 4095) << (12*i) for i,v in enumerate(values))


def adapt_oracle(oracle):
    original = oracle.prepare
    def prepare(instance, out):
        meta, packets = original(instance, out)
        out = Path(out)
        mode = bool(instance['config'][3] & 256)
        old = [int(s,16) for s in (out/'vertices.hex').read_text().splitlines()]
        values = []
        for i in range(0,len(old),6):
            values += old[i:i+3] + [normal_word(old[i+3:i+5],mode,oracle.M),old[i+5]]
        (out/'vertices.hex').write_text(''.join(f'{v:09x}\n' for v in values))
        for packet in packets:
            old = packet['words']
            assert len(old) == 19
            packet['words'] = tuple(old[:10]) + tuple(v for i in range(10,19,3)
                for v in (normal_word(old[i:i+2],mode,oracle.M),old[i+2]))
        (out/'words.hex').write_text(''.join(f'{v:014x}\n' for p in packets for v in p['words']) or '0\n')
        return meta, packets
    oracle.prepare = prepare
    return oracle
