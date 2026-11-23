"""Emit placed-byte declarations and native descriptors from one producer run."""
import hashlib
import json
import re
from pathlib import Path
from mesh_source import PositionEncoding, Vertex, encode_mesh
import mesh_source
import mesh_inspect
import mesh_partition
import gpu_mesh_layout
import gpu_texture_layout
import texture_source

REPOSITORY = Path(__file__).resolve().parents[3]


class Sources:
    def __init__(self, output):
        self.output = Path(output)
        self.output.mkdir(parents=True, exist_ok=True)
        self.declarations = ['mod static_assets; use static_assets::*;\n',
                             'mod gpu_api; use gpu_api::*;\n']
        self.resources = []
        self.relocations = []
        self.symbols = set()
        self.sources = {Path(__file__), Path(mesh_source.__file__), Path(mesh_inspect.__file__), Path(gpu_mesh_layout.__file__),
                        Path(texture_source.__file__), Path(gpu_texture_layout.__file__), *mesh_partition.SOURCES}

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

    def bytes(self, symbol, payload):
        self._asset(symbol, payload, 'GPU_ASSET_ALIGNMENT',
                    f'AssetBytes={symbol}_BYTES')

    def font(self, symbol, characters):
        """Select the shared base font and track its producer/data inputs."""
        import ui_font
        self.atlas(symbol, ui_font.base_asset(characters))
        self.sources.update(ui_font.SOURCES)

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

    def texture_pixels(self, symbol, size, pixels, pixel_format='Rgb565', *, mipmaps=False, color_space='srgb'):
        payload = texture_source.encode_pixels(size, pixels, pixel_format, mipmaps, color_space)
        self.texture(symbol, payload, size, pixel_format, mipmaps)

    def image(self, symbol, path, pixel_format='Rgb565', *, mipmaps=False, color_space='srgb'):
        """Load a square POT image; Pillow is needed only for file decoding."""
        from PIL import Image
        path = Path(path).resolve(strict=True)
        with Image.open(path) as image:
            if image.width != image.height:
                raise ValueError('texture images must be square; resizing is explicit')
            self.texture_pixels(symbol, image.width, image.convert('RGBA').getdata(),
                                pixel_format, mipmaps=mipmaps, color_space=color_space)
        self.sources.add(path)

    def geometry(self, symbol, vertices, triangles, *, encoding=None):
        """Build source-world geometry without native bitfields or directory arithmetic."""
        payload, meshlets, relocations = encode_mesh(vertices, triangles, encoding)
        self.mesh(symbol, payload, 0, meshlets, relocations)

    def write(self, filename, extra=''):
        (self.output / filename).write_text(''.join(self.declarations)+extra, encoding='utf-8')
        return dict(resources=self.resources, sector_relocations=self.relocations)

    def write_manifest(self, filename, *, sources=(), extra=''):
        """Publish declarations and the loader manifest with complete SDK dependencies."""
        inputs = self.sources | {Path(source) for source in sources}
        hashes = {path.resolve().relative_to(REPOSITORY).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
                  for path in sorted(inputs)}
        manifest = dict(source_root='repository', **self.write(filename, extra), sources=hashes)
        (self.output/'assets.json').write_text(json.dumps(manifest, indent=2)+'\n', encoding='utf-8')
        return manifest
