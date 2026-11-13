"""Emit placed-byte declarations and native descriptors from one producer run."""
import hashlib
import json
import re
from pathlib import Path


class Sources:
    def __init__(self, output):
        self.output = Path(output)
        self.output.mkdir(parents=True, exist_ok=True)
        self.declarations = ['mod static_assets; use static_assets::*;\n',
                             'mod gpu_api; use gpu_api::*;\n']
        self.resources = []
        self.relocations = []
        self.symbols = set()

    def _asset(self, symbol, payload, alignment, description):
        if not re.fullmatch(r'[A-Z][A-Z0-9_]*', symbol) or symbol in self.symbols:
            raise ValueError(f'invalid or duplicate native asset symbol: {symbol}')
        if not payload:
            raise ValueError(f'empty native asset: {symbol}')
        self.symbols.add(symbol)
        filename = symbol.lower() + '.bin'
        path = self.output / filename
        path.write_bytes(payload)
        included = json.dumps(path.resolve().as_posix())
        self.declarations += [f'#[asset(align={alignment})]\n',
                              f'pub const {symbol}_BYTES:AssetBytes=AssetBytes::new(include_bytes!({included}));\n',
                              f'pub const {symbol}:{description};\n']
        self.resources.append(dict(file=filename, placement='source', symbol=symbol+'_BYTES',
                                   bytes=len(payload), sha256=hashlib.sha256(payload).hexdigest()))

    def atlas(self, symbol, payload):
        if len(payload) % 256 or not 0 < len(payload) // 256 <= 255:
            raise ValueError('native atlas must contain 1..255 complete glyphs')
        self._asset(symbol, payload, 'UI_ATLAS_ALIGNMENT',
                    f'UiAtlasDesc=unsafe {{ UiAtlasDesc::from_asset({symbol}_BYTES) }}')

    def texture(self, symbol, payload, size, pixel_format='Rgb565', mipmaps=False):
        if size < 1 or size > 1024 or size & (size-1):
            raise ValueError('native texture size must be a power of two in 1..1024')
        if pixel_format not in {'Rgb565', 'Rgba4444'}:
            raise ValueError('unsupported native texture format')
        sides = [size]
        if mipmaps:
            while sides[-1] > 1:
                sides.append(sides[-1] // 2)
        expected = sum(max(512, side * side * 2) for side in sides)
        if len(payload) != expected:
            raise ValueError(f'native texture footprint is {len(payload)}, expected {expected}')
        mip = 'Full' if mipmaps else 'BaseOnly'
        self._asset(symbol, payload, 'GPU_TEXTURE_ALIGNMENT',
                    f'TextureDesc=unsafe {{ TextureDesc::from_asset({symbol}_BYTES, '
                    f'TextureLayout::square({size},TextureFormat::{pixel_format},Mipmaps::{mip})) }}')

    def mesh(self, symbol, payload, directory_offset, meshlets, sector_pointers):
        if not 0 < meshlets <= 5460 or directory_offset < 0 or directory_offset & 1:
            raise ValueError('invalid native mesh directory')
        if directory_offset + meshlets * 4 > len(payload):
            raise ValueError('native mesh directory exceeds payload')
        records = []
        offsets = set()
        for offset, addend in sector_pointers:
            if offset < 0 or offset & 1 or offset + 2 > len(payload) or offset in offsets:
                raise ValueError('invalid or repeated native mesh sector pointer')
            if addend < 0 or addend & 127 or addend >= len(payload) or payload[offset:offset+2] != b'\0\0':
                raise ValueError('native mesh pointer requires a zero placeholder and in-payload sector target')
            offsets.add(offset)
            records.append(dict(asset=symbol+'_BYTES', offset=offset,
                                target=symbol+'_BYTES', addend=addend))
        self._asset(symbol, payload, 'GPU_ASSET_ALIGNMENT',
                    f'MeshDesc=unsafe {{ MeshDesc::from_asset({symbol}_BYTES,{directory_offset},{meshlets}) }}')
        self.relocations.extend(records)

    def write(self, filename, extra=''):
        (self.output / filename).write_text(''.join(self.declarations)+extra, encoding='utf-8')
        return dict(resources=self.resources, sector_relocations=self.relocations)
