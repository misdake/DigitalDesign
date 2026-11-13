"""RGBA8 source pixels to the existing native texture encoder."""
from gpu_texture_layout import pack_levels, rgba4444


def _linear(channel, space):
    value = channel / 255
    if space == 'linear':
        return value
    return value / 12.92 if value <= 0.04045 else ((value + 0.055) / 1.055) ** 2.4


def _channel(value, space):
    value = max(0, min(1, value))
    if space == 'srgb':
        value = value * 12.92 if value <= 0.0031308 else 1.055 * value ** (1 / 2.4) - 0.055
    return round(value * 255)


def encode_pixels(size, pixels, pixel_format='Rgb565', mipmaps=False, color_space='srgb'):
    """Generate mips in linear premultiplied space, then encode straight alpha.

    No resizing is implicit. RGB/RGBA8 inputs must cover a square POT image.
    All mip levels derive from unquantized intermediates; storage padding and
    ordering remain exclusively owned by gpu_texture_layout.pack_levels.
    """
    if type(size) is not int or not 1 <= size <= 1024 or size & (size - 1):
        raise ValueError('texture size must be a power of two in 1..1024')
    if pixel_format not in {'Rgb565', 'Rgba4444'} or color_space not in {'srgb', 'linear'}:
        raise ValueError('unsupported texture format or color space')
    pixels = list(pixels)
    if len(pixels) != size * size:
        raise ValueError('pixel count must match the square texture size')
    linear = []
    for pixel in pixels:
        if len(pixel) not in (3, 4) or any(type(v) is not int or not 0 <= v <= 255 for v in pixel):
            raise ValueError('pixels require RGB8 or RGBA8 integer channels')
        alpha = pixel[3] if len(pixel) == 4 else 255
        if pixel_format == 'Rgb565' and alpha != 255:
            raise ValueError('RGB565 requires opaque pixels; use Rgba4444 for alpha')
        a = alpha / 255
        linear.append((*(_linear(v, color_space) * a for v in pixel[:3]), a))
    levels = {}
    side = size
    while True:
        encoded = []
        for r, g, b, a in linear:
            rgb = tuple(_channel(v / a, color_space) if a else 0 for v in (r, g, b))
            if pixel_format == 'Rgba4444':
                encoded.append(rgba4444((*rgb, round(a * 255))))
            else:
                r, g, b = rgb
                encoded.append(((r * 31 + 127) // 255 << 11) |
                               ((g * 63 + 127) // 255 << 5) | ((b * 31 + 127) // 255))
        levels[side.bit_length() - 1] = encoded
        if not mipmaps or side == 1:
            break
        half = side // 2
        linear = [tuple(sum(linear[(y * 2 + dy) * side + x * 2 + dx][c]
                            for dy in range(2) for dx in range(2)) / 4 for c in range(4))
                  for y in range(half) for x in range(half)]
        side = half
    return pack_levels(levels)
