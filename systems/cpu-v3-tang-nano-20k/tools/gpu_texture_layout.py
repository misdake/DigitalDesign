"""Production POT RGB565 storage: column macros, complete 512-byte mip tails."""
import struct


def level_offset(exponent, x, y):
    if not 0 <= exponent <= 10:
        raise ValueError('POT exponent must be in 0..10')
    extent = max(16, 1 << exponent)
    if not 0 <= x < extent or not 0 <= y < extent:
        raise ValueError('Texel is outside its padded level')
    macro = ((x >> 4) << max(exponent - 4, 0)) + (y >> 4)
    return macro * 512 + (y & 15) * 32 + (x & 15) * 2


def pack_levels(levels):
    """Smallest level first; repeat small logical levels through macro padding."""
    result = bytearray()
    for exponent, pixels in sorted(levels.items()):
        if not 0 <= exponent <= 10 or len(pixels) != 1 << (2 * exponent):
            raise ValueError('Invalid square POT level')
        size = 1 << exponent
        extent = max(16, size)
        start = len(result)
        for tx in range(extent // 16):
            for ty in range(extent // 16):
                for y in range(16):
                    for x in range(16):
                        result += struct.pack('<H', pixels[((ty * 16 + y) % size) * size + (tx * 16 + x) % size])
        # Decode by logical coordinate, independently of the producer traversal.
        for y in range(extent):
            for x in range(extent):
                actual = struct.unpack_from('<H', result, start + level_offset(exponent, x, y))[0]
                if actual != pixels[(y % size) * size + x % size]:
                    raise ValueError('Column texture or periodic padding mismatch')
    return bytes(result)
