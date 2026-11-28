"""Prepare immutable authored constructor inputs for the first-map runtime."""
import json
from pathlib import Path
import struct
import subprocess
import sys

FAMILIES = ['Obstacle', 'Crush', 'Fall', 'Bounce', 'Closer', 'Spin', 'Mover',
            'Noisy', 'Smoker', 'Trackcam', 'Bomber', 'Train', 'Flock', 'Powerup', 'WayPoint']


def build(reference: Path, output: Path):
    owner = Path(__file__).resolve().parent
    workspace = next(p for p in owner.parents if (p/'Cargo.toml').is_file() and (p/'systems').is_dir())
    # Keep MSVC build-script executable paths below its Windows path limit.
    source = workspace / 'target/scene-boot-reference'
    subprocess.run([sys.executable, str(owner/'scene_initial_assets.py'), '--reference',
                    str(reference/'Cargo.toml'), '--output', str(source)], check=True, timeout=400)
    world = json.loads((source/'scene-initial-world.json').read_text())['cases'][0]
    actors = json.loads((source/'scene-actor-initial.json').read_text())
    assert world['seed'] == 0 and len(world['records']) == 240
    train = pickup = 0
    data = [0x4353, 1, 240, 0, 16, 0, 30, 0, 12, 0, 0, 0, 0, 0, 0, 0]
    directory = []
    for object_id, row in enumerate(world['records']):
        assert row['entity'] == object_id + 1
        family = FAMILIES.index(row['class']) + 1
        metadata = row['input']
        extra = []
        if row['class'] == 'Train':
            extra = actors['trains'][train]['input']
            train += 1
        elif row['class'] == 'Powerup':
            extra = actors['pickups'][0]['rows'][pickup]['input']
            pickup += 1
        assert len(metadata) >= 37 and metadata[2] == 0
        directory.append(dict(object=object_id, family=row['class'], offset=len(data),
                              metadata_words=len(metadata), actor_words=len(extra)))
        sign = row['class'] == 'Fall' and (row['shape'].lower() == 'chevron.sm' or not row['has_shape'])
        data += [object_id, family, len(metadata), len(extra), int(sign) | (int(row['remove_rule']) << 1)] + metadata + extra
    assert train == 2 and pickup == 30
    data[5] = len(data)
    data += actors['camera_input']
    assert len(actors['camera_input']) == 30
    data[7] = len(data)
    data += actors['pickups'][0]['initial']
    data[3] = len(data)
    assert len(data) <= 16384
    payload = struct.pack('<'+'H'*len(data), *data)
    (output/'scene-boot.bin').write_bytes(payload)
    (output/'scene-boot-tilts.bin').write_bytes((source/'pickup-tilt.u16.bin').read_bytes())
    (output/'scene-boot-reference-sources.json').write_bytes((source/'scene-initial-sources.json').read_bytes())
    (output/'scene-boot-reference-overlay.json').write_bytes((source/'reference-overlay.json').read_bytes())
    (output/'scene-boot.json').write_text(json.dumps(dict(version=1, bytes=len(payload),
        scope='Immutable decoded authored inputs, original model sizes and static terrain heights. Runtime state and RNG checkpoint answers are excluded.',
        directory=directory), indent=2)+'\n')
    return source
