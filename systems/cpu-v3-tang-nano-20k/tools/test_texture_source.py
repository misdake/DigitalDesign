"""Source conversion checks independently decode native pixels and mip tails."""
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
from gpu_texture_layout import level_offset
from texture_source import encode_pixels


class TextureSource(unittest.TestCase):
    def test_base_and_complete_mips_have_native_padding_and_quantization(self):
        pixels = [(255, 0, 0), (0, 255, 0), (0, 0, 255), (255, 255, 255)]
        data = encode_pixels(2, pixels, mipmaps=True, color_space='linear')
        self.assertEqual(len(data), 1024)
        self.assertEqual(struct.unpack_from('<H', data, 0)[0], 0x8410)
        for (x, y), expected in zip([(0,0), (1,0), (0,1), (1,1)], [0xf800,0x07e0,0x001f,0xffff]):
            self.assertEqual(struct.unpack_from('<H', data, 512 + level_offset(1, x, y))[0], expected)

    def test_srgb_mips_filter_in_linear_light_and_alpha_avoids_dark_fringe(self):
        data = encode_pixels(2, [(255,0,0)] * 2 + [(0,0,0)] * 2, mipmaps=True)
        self.assertEqual(struct.unpack_from('<H', data)[0], 0xb800)
        data = encode_pixels(2, [(255,0,0,255)] * 2 + [(0,0,255,0)] * 2,
                             'Rgba4444', mipmaps=True)
        self.assertEqual(struct.unpack_from('<H', data)[0], 0xf008)

    def test_invalid_dimensions_count_channels_and_alpha_are_rejected(self):
        for size, pixels in [(3, [(0,0,0)]*9), (2, []), (1, [(256,0,0)]), (1, [(0,0,0,0)])]:
            with self.subTest(size=size), self.assertRaises(ValueError):
                encode_pixels(size, pixels)

    def test_launcher_supports_relocated_producer_with_local_and_sdk_imports(self):
        tools = Path(__file__).resolve().parent
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root/'local.py').write_text('VALUE = 7\n')
            (root/'producer.py').write_text('from local import VALUE\nfrom texture_source import encode_pixels\nassert VALUE == 7\nassert len(encode_pixels(1,[(255,0,0)])) == 512\n')
            result = subprocess.run([sys.executable, '-B', str(tools/'asset_producer.py'), str(root/'producer.py')],
                                    cwd=root, capture_output=True, text=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_image_decoder_tracks_input_and_matches_pixels_entry(self):
        from PIL import Image
        from native_asset_source import Sources
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            image = root/'source.png'
            pixels = [(255,0,0,255), (0,255,0,128), (0,0,255,0), (255,255,255,255)]
            source = Image.new('RGBA', (2,2))
            source.putdata(pixels)
            source.save(image)
            assets = Sources(root/'output')
            assets.image('PICTURE', image, 'Rgba4444', mipmaps=True)
            self.assertEqual((root/'output/picture.bin').read_bytes(), encode_pixels(2, pixels, 'Rgba4444', True))
            self.assertIn(image.resolve(), assets.sources)


if __name__ == '__main__':
    unittest.main()
