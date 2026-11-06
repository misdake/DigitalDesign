# Reference inputs

- `reference.txt`: schema-1 packed levels and original-engine transition vectors.
- `provenance.json`: hashes of the exact reference files read during import.
- `atlas-low.png`: unchanged 512×512 texture atlas used by the reference pixel mode.
- `geometry-reference.txt`: original Three.js camera/pose/shadow expectations.
- `geometry-provenance.json`: identities of the original geometry inputs and Three.js package.

The source atlas builder identifies its photographs as CC0 Poly Haven assets:
[marble_01](https://polyhaven.com/a/marble_01) and
[rusty_metal_sheet](https://polyhaven.com/a/rusty_metal_sheet).
The imported atlas retains the reference's colors, padding and UV regions.
