# Original texture photographs

- `marble_01.jpg`: original1024×1024 photo, SHA256 `b10dabea976d68baa976a0d4e8dac58789df8d26a1b50697f320106f1b00a229`.
- `rusty_metal_sheet.jpg`: original1024×1024 photo, SHA256 `ecfacdea466b43162b63867b419576b7d35d34ea37c96969cf05e5a07faeb705`.

These are byte-for-byte copies of the user's read-only BloxorzClone
`textures/src` inputs. Its `scripts/make-atlas.py` (SHA256
`4083f8acafb7d7d5e46ebbecd2a9bbaeb4a99b0a3558b55c35319325e192707f`)
identifies them as CC0 Poly Haven photographs:
[marble_01](https://polyhaven.com/a/marble_01) and
[rusty_metal_sheet](https://polyhaven.com/a/rusty_metal_sheet), under the
[source license](https://polyhaven.com/license).
`tools/source_photos.py` preserves that source identity and the original crop/tone
definitions. Each final material is resized directly from its photo crop; no old
atlas, mip, quantized texture or preview is an intermediate source.
