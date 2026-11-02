# Background input

`background-rgb565-column16.bin` is the original ordered-dither background,
204800 bytes, SHA256
`5f4be20a8bf7708d54ff2bde923210964c147eeb53c43c0b2e6ded71f9dc8047`.
It retains the accepted asset's original bytes rather than re-encoding a PNG.

The visible image is 400x240, with 16 padded rows in the physical 400x256 plane.
For pixel coordinates (x,y), its little-endian RGB565 offset is
`2*((x>>4)*4096+(y>>4)*256+(y&15)*16+(x&15))`. The source uses sRGB-coded
components and the application selects ordinary `DisplayFormat::Rgb565`.
Applying the linear-RGB565 Display lookup would apply an extra transfer curve.

The producer reserves byte 0x580000 for this immutable source and excludes the
complete 0x600000/0x640000 offscreen allocations. Every scene restoration uses
the current public `copy_color`, then color Load and independent depth Clear.
SELECT toggles this intentional gradient and the Web pixel-mode solid #2c2620.
The low atlas comes from the existing Bloxorz asset directory; no second atlas
is maintained here.

`web-shadow-reference.json` is an independent, generated Web geometry fixture:
actual world matrices and clipped receiver polygons for standing, rising,
tipping and released falling. Its source-file hashes bind the reference input;
it does not contain native shader output or a second level/Flash image.
