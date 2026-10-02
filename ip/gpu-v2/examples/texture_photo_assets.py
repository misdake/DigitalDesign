"""Fetch USC-SIPI photographs and produce independent coherent RAW565 mip assets.

Requires Pillow and NumPy. Downloads and generated data stay outside git.
"""
import concurrent.futures
import hashlib
import json
from pathlib import Path
import sys
import urllib.request

import numpy as np
from PIL import Image, __version__ as pillow_version

PHOTOS = {'mandrill': '4.2.03', 'airplane': '4.2.05',
          'sailboat': '4.2.06', 'peppers': '4.2.07'}


def prepare(root, name, image_id):
    url = f'https://sipi.usc.edu/database/download.php?img={image_id}&vol=misc'
    source = root / f'{name}.tiff'
    if not source.exists():
        with urllib.request.urlopen(url, timeout=45) as response:
            source.write_bytes(response.read())
    image = Image.open(source).convert('RGB')
    if image.size != (512, 512):
        raise ValueError(f'{name}: expected original 512x512 color photo')
    levels = {9: image}
    for n in range(8, -1, -1):
        levels[n] = levels[n + 1].resize((1 << n, 1 << n), Image.Resampling.BOX)
    payload = bytearray()
    for n in range(10):
        levels[n].save(root / f'{name}-mip{n}.png')
        rgb = np.asarray(levels[n], dtype=np.float64)
        # Quantize each coherent RGB mip independently to nearest RAW565 code.
        r = np.rint(rgb[:, :, 0] * 31 / 255).astype(np.uint16)
        g = np.rint(rgb[:, :, 1] * 63 / 255).astype(np.uint16)
        b = np.rint(rgb[:, :, 2] * 31 / 255).astype(np.uint16)
        words = (r << 11) | (g << 5) | b
        side = max(8, 1 << n)
        if n < 3:
            words = np.tile(words, (8 // (1 << n),) * 2)
        tiles = words.reshape(side // 8, 8, side // 8, 8).transpose(0, 2, 1, 3)
        payload.extend(tiles.astype('<u2').tobytes())
    (root / f'{name}.raw565').write_bytes(payload)
    return {'name': name, 'id': image_id, 'url': url,
            'source_sha256': hashlib.sha256(source.read_bytes()).hexdigest(),
            'asset_sha256': hashlib.sha256(payload).hexdigest(),
            'asset_bytes': len(payload), 'original_size': list(image.size)}


def main():
    root = Path(sys.argv[1] if len(sys.argv) > 1 else 'target/gpu-v2-texture-photos/assets')
    root.mkdir(parents=True, exist_ok=True)
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        futures = [pool.submit(prepare, root, name, image_id) for name, image_id in PHOTOS.items()]
        sources = [future.result() for future in futures]
    manifest = {'source_index': 'https://sipi.usc.edu/database/database.php?volume=misc',
                'pillow_version': pillow_version, 'numpy_version': np.__version__,
                'mip_filter': 'recursive RGB8 code-space BOX; then per-mip RNE RAW565',
                'sources': sources}
    (root / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
    for source in sources:
        print(source['name'], source['source_sha256'], source['asset_bytes'])


if __name__ == '__main__':
    main()
