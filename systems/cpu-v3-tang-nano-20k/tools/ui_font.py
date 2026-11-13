"""Pack the project font into the Native UI bank-swizzled RGBA5551 atlas."""
import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
FONT = ROOT / 'assets/fonts/vonwaon-bitmap16-ascii.txt'
PROVENANCE = FONT.with_name('vonwaon-bitmap16-provenance.json')
SOURCES = (Path(__file__).resolve(), FONT, PROVENANCE)


def rgba5551(red: int, green: int, blue: int, alpha: int = 255) -> int:
    """Pack native UI color: R15:11/G10:6/B5:1/A0, alpha high bit."""
    if any(value < 0 or value > 255 for value in (red, green, blue, alpha)):
        raise ValueError("RGBA channels must be bytes")
    return ((red >> 3) << 11) | ((green >> 3) << 6) | ((blue >> 3) << 1) | (alpha >> 7)


def load_rows():
    rows = {}
    for line in FONT.read_text().splitlines():
        if not line or line.startswith('#'):
            continue
        fields = line.split()
        code = int(fields[0])
        bitmap = tuple(int(value, 16) for value in fields[1:])
        if code in rows or len(bitmap) != 16 or any(not 0 <= value < 256 for value in bitmap):
            raise ValueError('Invalid native 8x16 font row: ' + line)
        rows[code] = bitmap
    if set(rows) != set(range(32, 127)):
        raise ValueError('Project font must cover printable ASCII 32..126')
    return rows


def write_glyph(data, code, rows, foreground=0xffff, background=0):
    if len(data) % 256 or not 0 <= code < min(len(data)//256, 255) or len(rows) != 16:
        raise ValueError('Expected a Native UI atlas and an 8x16 glyph, excluding skip ID 255')
    for y, bits in enumerate(rows):
        for x in range(8):
            offset = code*256 + (y*8 + (x ^ ((y & 1)*2)))*2
            struct.pack_into('<H', data, offset, foreground if bits & (128 >> x) else background)


def atlas(rows, glyph_count=256):
    if not 1 <= glyph_count <= 256:
        raise ValueError('Native UI atlas supports 1..256 glyph records')
    data = bytearray(glyph_count*256)
    for code, bitmap in rows.items():
        write_glyph(data, code, bitmap)
    return data


def base_asset(characters, extra_glyphs=0):
    """Native ASCII IDs, only the required extent; append app icons after it."""
    codes = set(characters.encode('ascii') if isinstance(characters, str) else characters)
    rows = load_rows()
    if not codes or not codes <= rows.keys():
        raise ValueError('Select printable ASCII from the project font')
    count = max(codes)+1+extra_glyphs
    if count > 255 or extra_glyphs < 0:
        raise ValueError('UI glyph ID 255 is reserved for skip')
    return atlas({code: rows[code] for code in codes}, count)


def preview(data):
    from PIL import Image
    if not len(data) or len(data) % 256 or len(data) > 65536:
        raise ValueError('Expected complete Native UI glyph records, at most 64 KiB')
    count = len(data)//256
    image = Image.new('RGBA', (128, ((count+15)//16)*16))
    for code in range(count):
        for y in range(16):
            for x in range(8):
                value = struct.unpack_from('<H', data, code*256 + (y*8 + (x ^ ((y & 1)*2)))*2)[0]
                color = tuple(((value >> shift) & 31)*255//31 for shift in (11, 6, 1))
                image.putpixel(((code % 16)*8+x, (code//16)*16+y), (*color, 255 if value & 1 else 0))
    return image
