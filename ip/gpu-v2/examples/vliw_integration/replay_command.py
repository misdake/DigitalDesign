"""Canonical native command/descriptor encoders shared by bounded RTL fixtures.

Whole-frame replay uses the system GPU profiler and its explicit column metadata.
"""
import struct

COMMAND_BASE = 0x40000


def align(value, size=128):
    return (value+size-1)//size*size


class CommandBuffer:
    OPS = dict(NOP=0, DMA=1, DMA_MESHLET=2, DRAW_MESHLET=3, WAIT=4,
               SET_GLOBAL=5, SET_RENDER=6, FLUSH=7, CLEAR=8, TEX_INVALIDATE=9, SET_TARGET=10, UI_GRID=11, MEMCPY=12, END=15)

    def __init__(self):
        self.records = []

    def emit(self, name, slot=0, a=0, b=0, c=0):
        op = self.OPS[name]
        if name == 'DMA':
            assert b in (32,64,128) and a % b == 0 and slot % 4 == 0
            word = (op<<28) | ((a//32)<<10) | ((slot//4)<<3) | ((32,64,128).index(b)<<1)
        elif name == 'DMA_MESHLET':
            assert a % 128 == 0 and b in range(128,1025,128) and slot in (0,1)
            word = (op<<28) | ((a//128)<<12) | ((b//128-1)<<1) | slot
        elif name == 'DRAW_MESHLET':
            word = (op<<28) | slot
        elif name == 'CLEAR':
            assert slot in (0,1) and 0 <= a <= 65535
            word = (op<<28) | (slot<<16) | a
        elif name == 'UI_GRID' and b:
            assert slot % 4 == 0 and 0 <= a < b <= 750
            word = (op<<28) | (1<<27) | ((b-1)<<17) | (a<<7) | (slot//4)
        elif name in ('SET_GLOBAL','SET_RENDER','SET_TARGET','UI_GRID','MEMCPY'):
            assert slot % 4 == 0
            word = (op<<28) | (slot//4)
        else:
            word = (op<<28) | (a if name == 'WAIT' else 0)
        self.records.append(dict(name=name, slot=slot, args=[a,b,c], word=word))

    def encode(self):
        return b''.join(struct.pack('<I', r['word']) for r in self.records)

    def text(self):
        return '\n'.join(f'{4*i:04x}: {r["word"]:08x} {r["name"]:<16} slot={r["slot"]:3} '
                         + ' '.join(f'{v:#x}' for v in r['args']) for i,r in enumerate(self.records))+'\n'


def target_params(color, depth):
    assert color % 65536 == 0 and depth % 65536 == 0
    assert 0 <= color <= 0x800000-204800 and 0 <= depth <= 0x800000-204800
    assert color+204800 <= depth or depth+204800 <= color
    return struct.pack('<Q', (color//512) | ((depth//512)<<14)) + bytes(24)


def memcpy_params(source, destination, size):
    """Raw copy descriptor; ownership/cache handoff remains explicit."""
    assert 0 < size <= 0x100000 and size % 512 == 0
    assert source % 512 == destination % 512 == 0
    assert 0 <= source <= 0x800000-size and 0 <= destination <= 0x800000-size
    assert source+size <= destination or destination+size <= source
    return struct.pack('<Q', source//512 | ((destination//512)<<14) | ((size//512-1)<<28)) + bytes(24)


def texture_word(base, size_log2, full_mip, filter_code, framebuffer_view=False):
    """NativeRender32 bit23 selects fixed400x240 in the same column backend."""
    assert 0 <= base < 0x800000 and base % 256 == 0
    assert 0 <= size_log2 <= 10 and filter_code in (0,1,2)
    if framebuffer_view:
        assert base % 65536 == 0 and size_log2 == 9 and not full_mip and filter_code in (0,1)
    return ((base // 128) | (size_log2 << 16) | (int(full_mip) << 20)
            | (filter_code << 21) | (int(framebuffer_view) << 23))
