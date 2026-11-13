"""Build a selected project-font base asset with coordinated UI atlas constants."""
import argparse
import hashlib
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[5]
TOOLS = ROOT / "systems/cpu-v3-tang-nano-20k/tools"
sys.path.insert(0, str(TOOLS))
import ui_font
import native_asset_source

# Native 8x16 application icons, appended to the selected shared font asset.
ICONS = {
    'PREVIOUS': [0,0,0,0x08,0x10,0x20,0x40,0x20,0x10,0x08,0,0,0,0,0,0],
    'NEXT': [0,0,0,0x10,0x08,0x04,0x02,0x04,0x08,0x10,0,0,0,0,0,0],
    'HOME': [0,0,0,0x18,0x24,0x42,0x7e,0x42,0x5a,0x5a,0x5a,0x7e,0,0,0,0],
    'ACCEPT': [0,0,0,0,0x02,0x04,0x48,0x30,0x20,0,0,0,0,0,0,0],
    'PLAY': [0,0,0,0x40,0x60,0x70,0x78,0x70,0x60,0x40,0,0,0,0,0,0],
    'PAUSE': [0,0,0,0x66,0x66,0x66,0x66,0x66,0x66,0x66,0,0,0,0,0,0],
    'RESET': [0,0,0,0x3c,0x42,0x82,0x9e,0x88,0x80,0x42,0x3c,0,0,0,0,0],
    'ADJUST': [0,0,0,0,0x24,0x42,0xff,0x42,0x24,0,0,0,0,0,0,0],
    'SELECT': [0,0,0,0x1f,0,0x5f,0x80,0x5f,0,0x1f,0,0,0,0,0,0],
    'RADIO_OFF': [0,0,0,0x3c,0x42,0x42,0x42,0x42,0x42,0x3c,0,0,0,0,0,0],
    'RADIO_ON': [0,0,0,0x3c,0x42,0x5a,0x5a,0x5a,0x42,0x3c,0,0,0,0,0,0],
}

KEY_NAMES = ('L1', 'R1', 'CROSS', 'TRIANGLE', 'START', 'DPAD_H', 'DPAD_V')


def keycaps():
    """24x16 controller keys, with native unscaled L1/R1 font pixels."""
    font = ui_font.load_rows()
    result = {}
    for name in KEY_NAMES:
        rows = [0]*16

        def pixel(x, y):
            rows[y] |= 1 << (23-x)

        if name in ('L1', 'R1', 'START'):
            for x in range(2, 22):
                pixel(x, 1)
                pixel(x, 14)
            for y in range(3, 13):
                pixel(0, y)
                pixel(23, y)
            for x, y in ((1,2),(22,2),(1,13),(22,13)):
                pixel(x, y)
            if name != 'START':
                for i, char in enumerate(name):
                    for y, bits in enumerate(font[ord(char)]):
                        rows[y] |= bits << (12-i*8)
            else:
                for y in range(4, 11):
                    for x in range(9, 16-abs(y-7)*2):
                        pixel(x, y)
        elif name in ('CROSS', 'TRIANGLE'):
            for y in range(1, 15):
                for x in range(5, 19):
                    radius = (x-12)**2+(y-7)**2
                    if 25 <= radius <= 36:
                        pixel(x, y)
            if name == 'CROSS':
                for i in range(5):
                    pixel(10+i, 5+i)
                    pixel(14-i, 5+i)
            else:
                for i in range(5):
                    pixel(12-i, 4+i)
                    pixel(12+i, 4+i)
                for x in range(8, 17):
                    pixel(x, 9)
        else:
            points = {(x,y) for y in range(3,13) for x in range(5,19)
                      if 10 <= x <= 13 or 6 <= y <= 9}
            for x, y in points:
                if any((x+dx,y+dy) not in points for dx,dy in ((1,0),(-1,0),(0,1),(0,-1))):
                    pixel(x, y)
            if name == 'DPAD_H':
                for x in range(5,19):
                    pixel(x, 7)
                    pixel(x, 8)
            else:
                for y in range(3,13):
                    pixel(11, y)
                    pixel(12, y)
        result[name] = rows
    return result


def pack(output):
    output.mkdir(parents=True, exist_ok=True)
    # Native ASCII IDs avoid a runtime character translation table.
    keys = keycaps()
    extra = len(ICONS)+3*len(keys)
    atlas = ui_font.base_asset('ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 /->', extra_glyphs=extra)
    first_icon = len(atlas)//256-extra
    for index, rows in enumerate(ICONS.values()):
        ui_font.write_glyph(atlas, first_icon+index, rows, foreground=ui_font.rgba5551(80, 120, 160))
    first_key = first_icon+len(ICONS)
    for index, rows in enumerate(keys.values()):
        for cell in range(3):
            bits = [(row >> ((2-cell)*8)) & 255 for row in rows]
            ui_font.write_glyph(atlas, first_key+index*3+cell, bits, foreground=ui_font.rgba5551(184, 208, 248))
    native = native_asset_source.Sources(output)
    native.atlas('CORE_ATLAS', atlas)
    constants = ''
    constants += ''.join(f'pub const ICON_{name}:u8={first_icon+index};\n'
                         for index,name in enumerate(ICONS))
    constants += ''.join(f'pub const KEY_{name}:u8={first_key+index*3};\n'
                         for index,name in enumerate(keys))
    generated = native.write('core_assets.rs', constants)
    sources = [Path(__file__), Path(native_asset_source.__file__), *ui_font.SOURCES]
    manifest = dict(source_root='repository', **generated,
                    sources={p.relative_to(ROOT).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest() for p in sources})
    (output/'assets.json').write_text(json.dumps(manifest, indent=2)+'\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--output', type=Path, required=True)
    pack(parser.parse_args().output)
