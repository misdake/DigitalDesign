"""Compose recorded physics inputs with the accepted static scene assets."""
import hashlib
import json
import os
from pathlib import Path
import sys

import runtime_assets
import static_assets


def main():
    runtime_assets.main()
    output = Path(sys.argv[sys.argv.index('--output') + 1]).resolve()
    modules = output / 'rcc-modules'
    owner = Path(__file__).resolve().parent
    repository = owner.parents[4]
    render = output / 'render'
    os.environ['TANKRACER_RECORDING'] = 'prepared'
    os.environ['TANKRACER_PREPARED_SELECTION'] = 'coarse-sphere-incremental'
    static_assets.build(render, Path(os.environ['TANKRACER_ROUTE_INPUT']))
    manifest = json.loads((output / 'assets.json').read_text())
    scene = json.loads((render / 'assets.json').read_text())
    if len(manifest['cases']) != 1:
        raise ValueError('combined execution requires one continuous level')
    symbols = {row['symbol'] for row in manifest['resources']}
    for resource in scene['resources']:
        if resource['symbol'] in symbols:
            raise ValueError('physics and rendering resource symbols overlap')
        manifest['resources'].append(dict(resource, file='render/' + resource['file']))
    manifest['sector_relocations'] += scene['sector_relocations']
    for name, digest in scene['sources'].items():
        manifest['sources'].append(dict(file=str(repository / name), sha256=digest))
    manifest['sources'].append(dict(file=str(Path(__file__).resolve()),
        sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest()))
    manifest['rendering'] = dict(camera_cases=scene['camera_cases'],
        render_stage=scene['render_stage'], map_sha256=scene['map_sha256'],
        camera_sha256=scene['camera_sha256'],
        scope='Recorded static views; physics runs concurrently after submission. '
              'Camera playback is independent of vehicle movement.')
    # Code placement is application-owned. The SDK retains its ordinary source.
    for name in ['world', 'range_map', 'prepared_map', 'unlit_world', 'pvs', 'playback']:
        original = owner.parent / 'rcc' / (name + '.rs')
        manifest['sources'].append(dict(file=str(original), sha256=hashlib.sha256(original.read_bytes()).hexdigest()))
        source = original.read_text()
        source = source.replace('boot_resources', 'static_resources')
        (modules / (name + '.rs')).write_text('#![link_cseg = "frame"]\n' + source)
    for name in ['impact', 'tank_response', 'ground']:
        original = owner.parent / 'rcc' / (name + '.rs')
        manifest['sources'].append(dict(file=str(original), sha256=hashlib.sha256(original.read_bytes()).hexdigest()))
        source = original.read_text()
        (modules / (name + '.rs')).write_text('#![link_cseg = "frame"]\n' + source)
    source = (render / 'map_stage.rs').read_text().replace('boot_resources', 'static_resources')
    (modules / 'map_stage.rs').write_text('#![link_cseg = "frame"]\n' + source)
    (modules / 'static_resources.rs').write_bytes((render / 'tankracer_static.rs').read_bytes())
    (modules / 'frame_output.rs').write_text('mod range_map;\npub use range_map::Renderer;\n')
    (output / 'assets.json').write_text(json.dumps(manifest, indent=2) + '\n')


if __name__ == '__main__':
    main()
