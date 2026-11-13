"""Native mesh/texture/font qualification through the public producer helpers."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import sys

sys.dont_write_bytecode = True
SYSTEM = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(SYSTEM / 'tools'))
from gpu_mesh_layout import native_record
from gpu_texture_layout import pack_levels
from native_asset_source import Sources
import ui_font


def produce(output):
    native = Sources(output)
    mesh = dict(vertices=[[0, 511 << 20, 0], [512, 511 << 20, 65535],
                          [256 | (512 << 10), 511 << 20, 32768 | (65535 << 16)]],
                triangles=[[0, 1, 2]], config=[-65536, -65536, 0, 8], color_mode=0)
    record = native_record(mesh)
    payload = bytearray(128) + record
    struct.pack_into('<H', payload, 2, len(record)//128-1)
    native.mesh('TRIANGLE', payload, 0, 1, [(0, 128)])
    levels = {}
    for exponent in range(5):
        side = 1 << exponent
        levels[exponent] = [0xf800 if ((x >> max(0, exponent-2)) ^ (y >> max(0, exponent-2))) & 1 else 0x07e0
                            for y in range(side) for x in range(side)]
    native.texture('CHECKER', pack_levels(levels), 16, 'Rgb565', mipmaps=True)
    native.atlas('FONT', ui_font.base_asset('SDK'))
    manifest = native.write('native_assets.rs')
    manifest['raw_sha256'] = {r['file']: hashlib.sha256((output/r['file']).read_bytes()).hexdigest()
                              for r in manifest['resources']}
    (output/'assets.json').write_text(json.dumps(manifest, indent=2)+'\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--output', type=Path, required=True)
    produce(parser.parse_args().output)
