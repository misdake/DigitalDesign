"""Build raw-map driving qualification assets and unmodified reference answers."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
from reference_bridge import prepare


def words(values):
    out = []
    for v in values:
        out.extend([v & 65535, (v >> 16) & 65535])
    return out


def binary(path, values):
    path.write_bytes(struct.pack('<' + 'H' * len(values), *values))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    out = parser.parse_args().output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    owner = Path(__file__).resolve().parent
    root = Path(os.environ['TANKRACER_REFERENCE']).resolve().parent
    root = Path(root.as_posix().removeprefix('//?/'))
    steps = int(os.environ.get('DRIVING_STEPS', '14'))
    if not 1 <= steps <= 96:
        raise ValueError('driving steps must be in 1..96')
    project = out / 'reference-tool'
    project.mkdir(exist_ok=True)
    game = prepare(root, out, driving=True, lifecycle=True, meshes=True, symbols=True)
    manifest = ('[package]\nname="tankracer-driving-assets"\nversion="0.1.0"\nedition="2024"\n'
                '[workspace]\n[[bin]]\nname="driving-oracle"\npath='
                + json.dumps((owner / 'driving_oracle.rs').as_posix())
                + '\n[dependencies]\nserde_json="=1.0.109"\n'
                + 'tankracer-assets={path=' + json.dumps((root / 'crates/assets').as_posix()) + '}\n'
                + 'tankracer-game={path=' + json.dumps(game.as_posix()) + '}\n'
                + '[[bin]]\nname="paired-driving-oracle"\npath='
                + json.dumps((owner / 'paired_driving_oracle.rs').as_posix()) + '\n')
    (project / 'Cargo.toml').write_text(manifest)
    with (project / 'build.log').open('w') as log:
        subprocess.run(['cargo', 'build', '--release', '--offline', '--manifest-path', str(project / 'Cargo.toml'),
                        '--target-dir', str(project / 'target')], check=True, stdout=log, stderr=subprocess.STDOUT, timeout=300)
    exe = project / 'target/release' / ('driving-oracle.exe' if os.name == 'nt' else 'driving-oracle')
    subprocess.run([str(exe), str(root), str(out), str(steps)], check=True, timeout=60)
    data = json.loads((out / 'driving.json').read_text())
    exe = project / 'target/release' / ('paired-driving-oracle.exe' if os.name == 'nt' else 'paired-driving-oracle')
    subprocess.run([str(exe), str(root), str(out), str(steps)], check=True, timeout=60)
    paired = json.loads((out / 'paired.json').read_text())
    catalog_reference = out / 'mesh-catalog-reference'
    subprocess.run([sys.executable, str(owner / 'mesh_store_assets.py'), '--reference',
                    str(root / 'Cargo.toml'), '--output', str(catalog_reference)], check=True, timeout=400)
    (out / 'mesh-catalog.bin').write_bytes((catalog_reference / 'mesh-catalog.bin').read_bytes())
    catalog = json.loads((catalog_reference / 'mesh-catalog.json').read_text())
    catalog_words = 2560 + catalog['source_metadata_bytes'] // 2 + catalog['compact_index_bytes'] // 2
    parents = json.loads((out / 'parent-bindings.json').read_text())
    assert len(parents) == 13
    parent_words = [0x4150, 1, len(parents), 26, 8, 0, 0, 0]
    motion_layout = {row[0]: row[1:] for row in catalog['motion_layout']}
    for row in parents:
        faces, offset = motion_layout.get(row['object'], [0,0])
        name = row['name'].lower()
        parent_words += [row['object'], row['kind'], catalog['auxiliary_sources'].get(name,65535),
                         catalog['names'].index(name) if name else 65535, faces, offset]
        parent_words += words([row['current_mesh'], row['private_mesh']])
        parent_words += [row['active'] & 65535, row['motion_count']]
        parent_words += words([row['gravity']]) + row['gains']
        parent_words += [int(row['reference_alias']), row['attached_count']]
    pickup_words=[]
    pickups=json.loads((out/'pickup-bindings.json').read_text())
    assert len(pickups)==30
    for row in pickups:
        faces, offset = motion_layout.get(row['object'], [0,0])
        name = row['name'].lower()
        pickup_words += [row['object'], row['kind'], catalog['auxiliary_sources'].get(name,65535),
                         catalog['names'].index(name) if name else 65535, faces, offset]
        pickup_words += words([row['current_mesh'], row['private_mesh']])
        pickup_words += [row['active'] & 65535, row['motion_count']]
        pickup_words += words([row['gravity']]) + row['gains']
        pickup_words += [row['visibility'], row['attached_count']]
    binary(out/'pickup-bindings.bin',pickup_words)
    pickup_config=json.loads((out/'pickup-config.json').read_text())
    assert len(pickup_config)==46
    binary(out/'pickup-config.bin',words(pickup_config[:5])+pickup_config[5:]+words([5,6])+words([0,0,0,4096,0,0,0,4096,0,0,0,4096]))
    binary(out / 'parent-bindings.bin', parent_words)
    binary(out / 'auxiliary-spans.bin', catalog['auxiliary_spans'])
    binary(out / 'motion-layout.bin', [v for row in catalog['motion_layout'] for v in row])
    inverse = root / 'crates/game/data/cosine-angle.bin'
    (out / inverse.name).write_bytes(inverse.read_bytes())

    for case in data['cases']:
        assert len(case['initial']) == 1779
        supplement = [0] * 4 + case['initial'][:295] + [0] * 18 + case['initial'][1596:1779]
        assert len(supplement) == 500
        case['initial'] += supplement + [0, -1, 0, 0, -1, 0, 0] + [0] * 4
        case['cars'], case['entities'], case['tank_contacts'] = 1, [1, 2], 0
        for frame in case['frames']:
            seed = frame['expected'].pop()
            frame['expected'] += supplement + [0, -1, 0, 0, -1, 0, 0] + [0] * 4 + [seed]
    directory = struct.unpack('<64H', (out / 'directory.bin').read_bytes())
    collision = (out / 'cells.bin').read_bytes()
    cells = {cell: collision[page*32768:(page+1)*32768] for cell, page in enumerate(directory) if page != 65535}
    collision = (out / 'paired-cells.bin').read_bytes()
    for page, cell in enumerate(paired['cells']):
        block = collision[page*32768:(page+1)*32768]
        if cell in cells and cells[cell] != block:
            raise ValueError(f'paired collision cell differs: {cell}')
        cells[cell] = block
    directory = [65535] * 64
    for page, cell in enumerate(sorted(cells)):
        directory[cell] = page
    (out / 'cells.bin').write_bytes(b''.join(cells[cell] for cell in sorted(cells)))
    binary(out / 'directory.bin', directory)
    environment=data['cases'][0]['environment']
    data['cases'] += paired['cases']
    qualification_scope=os.environ.get('TANKRACER_DRIVING_SCOPE','full')
    if qualification_scope=='startup':
        data['cases']=[c for c in data['cases'] if c['name']=='complete-startup']
    elif qualification_scope=='debris':
        data['cases']=[c for c in data['cases'] if c['name'] in ['debris-169','debris-peer-blast','debris-retire']]
    elif qualification_scope=='fast-controls':
        data['cases']=[c for c in data['cases'] if c['name']=='complete-fast-controls']
    elif qualification_scope=='controls':
        data['cases']=[c for c in data['cases'] if c['name']=='complete-controls']
    elif qualification_scope=='visibility':
        data['cases']=[c for c in data['cases'] if c['name']=='complete-visibility']
    elif qualification_scope=='effect':
        data['cases']=[c for c in data['cases'] if c['name']=='complete-effect']
    elif qualification_scope=='complete':
        data['cases']=[c for c in data['cases'] if c['name'].startswith('complete-')]
    elif qualification_scope=='prefix':
        data['cases']=[c for c in data['cases'] if c['name'].startswith('prefix-')]
    elif qualification_scope=='pickup':
        data['cases']=[c for c in data['cases'] if c['name'].startswith('pickup-')]
    elif qualification_scope!='full':
        raise ValueError('unknown driving qualification scope')
    data['scope'] = ('persistent real terrain contact, suspension/power, ordered ordinary/tank/deformable response, '
                     'ground reflection/impact/damage, motion, articulation, lean, live gun/body vertices, '
                     'contact hull, shared impact animation/pool and RNG; original ordered physical '
                     'car/scenery ticks with complete live-world comparisons, constructed mesh owner, '
                     'shared Crush hit/sampling vertices and resource revisions, authored Fall/Closer '
                     'fragment hit/tick/resource retirement on the same owner, complete vehicle prepare, '
                     'pickup flight/collection, inventory and shared quad/light state; '
                     'unrelated actors frozen in qualifier initial state')
    state_values = len(data['cases'][0]['initial'])
    initial_words = state_values * 2 + 18
    frame_words = 60 + (state_values + 1) * 2
    initial, frames, metadata = [], [], []
    for case_index, case in enumerate(data['cases']):
        assert len(case['initial']) == state_values, case['name']
        initial.extend(words(case['initial']))
        initial.extend(words([case['seed_after_noise']]))
        initial.extend(case['noise'])
        metadata.append(case['cars'])
        metadata.extend(words(case['entities']))
        metadata.extend([1, int(case['cars'] == 2 or case.get('scene', False))])
        metadata.extend(case.get('racer_slots', [0, 1 if case['cars'] == 2 else 65535] + [65535] * 7))
        metadata.extend(words([case['initial'][2285]]))
        metadata.extend([int(case.get('scene', False))] * 2)
        metadata.extend([len(case['frames']), int(case.get('scenery', False))])
        metadata.extend([case.get('retire_object', 0), case.get('retire_frame', 65535)])
        group = []
        for frame in case['frames']:
            control = list(frame['control'])
            assert len(control) == 30 and len(frame['expected']) == state_values + 1, case['name']
            if case.get('scene'):
                assert len(frame['surface']) == 749, case['name']
            # Only independent input reaches the target. Sampling and boost publish these slots.
            control[5:19] = [0] * 14
            group.extend(words(control))
            group.extend(words(frame['expected']))
        binary(out / f'frames-{case_index}.bin', group)
        if case.get('scene'):
            binary(out / f'surfaces-{case_index}.bin', words([v for frame in case['frames'] for v in frame['surface']]))
        if case.get('scenery'):
            binary(out / f'scenery-{case_index}.bin', [v for frame in case['frames'] for v in frame['scenery']])
        if case.get('scene'):
            assert all(len(frame['resource_versions']) == 28 for frame in case['frames'])
            binary(out / f'mesh-versions-{case_index}.bin', [v for frame in case['frames'] for v in frame['resource_versions']])
        if case['name'].startswith(('auxiliary-', 'debris-', 'prefix-', 'pickup-')) or case['name'] in ['complete-wall','complete-prepare']:
            binary(out / f'parents-{case_index}.bin', [v for frame in case['frames'] for v in frame['parents']])
        if case['name'].startswith('debris-'):
            binary(out / f'debris-{case_index}.bin', [v for frame in case['frames'] for v in frame['debris']])
        if case['prefix_initial']:
            binary(out / f'prefix-{case_index}.bin',[v for frame in case['frames'] for v in frame['prefix']])
            binary(out / f'prefix-initial-{case_index}.bin',case['prefix_initial'])
        if case['name'].startswith('complete-'):
            binary(out/f'complete-initial-{case_index}.bin',case['complete_initial'])
            binary(out/f'complete-{case_index}.bin',[v for frame in case['frames'] for v in frame['complete']])
        if case['name'].startswith('pickup-'):
            binary(out/f'pickup-{case_index}.bin',[v for frame in case['frames'] for v in frame['pickup']])
            binary(out/f'pickup-initial-{case_index}.bin',case['pickup_initial'])
        frames.extend(group)
    config = words(data['spring']) + words(data['drive']) + data['attitude'] + words(data['pose'])
    config += data['bumps'] + words(data['load']) + words(environment)
    config += words([v for p in data['auxiliary'] for v in p])
    config += words(data['body']) + [v & 65535 for v in data['gun_binding']]
    assert len(config) == 695
    assert len(initial) == len(data['cases']) * initial_words
    assert len(metadata) == len(data['cases']) * 24
    updates = sum(len(case['frames']) for case in data['cases'])
    checked_scenery_words = 0
    for case in data['cases']:
        if case.get('scenery'):
            for frame in case['frames']:
                live = frame['scenery']
                checked_scenery_words += 8 + live[6] + live[8] * 2 + live[3] * (6 + 76)
                for i in range(live[3]):
                    kind = live[live[4] + i * 8]
                    checked_scenery_words += 18 if kind in [1, 3] else 44 if kind == 2 else 0
                cursor = live[9]
                for _ in range(10):
                    count = live[cursor]
                    checked_scenery_words += 1 + count * 2
                    cursor += 1 + count * 2
                checked_scenery_words += 64 + sum(live[live[10]:live[10] + 64]) * 2
    assert len(frames) == updates * frame_words
    binary(out / 'car-bindings.bin', [v for c in data['cases'] for v in c.get('car_bindings',[0]*38)])
    binary(out / 'initial.bin', initial)
    binary(out / 'config.bin', config)
    binary(out / 'body.bin', [v & 65535 for v in data['body_data']])
    binary(out / 'cases.bin', metadata)
    binary(out / 'binding.bin', words(data['collision_binding']))
    subprocess.run([sys.executable, str(owner / 'export_trig_assets.py'), '--reference', str(root), '--output', str(out)], check=True)
    (out / 'trig_lookup.rs').write_bytes((owner.parent / 'rcc/trig_asset_lookup.rs').read_bytes())
    for module in ['scene_parent', 'scene_fragments', 'scene_meshes', 'mesh_storage',
                   'mesh_catalog', 'mesh_break', 'scene_auxiliaries', 'closer', 'inverse_basis', 'shatter', 'debris_catalog', 'scene_debris']:
        source = (owner.parent / 'rcc' / f'{module}.rs').read_text()
        (out / f'{module}.rs').write_text('#![link_cseg = "parents"]\n' + source)
    for module in ['prefix','spin','vehicle_inactive','player_control']:
        (out / f'{module}.rs').write_text('#![link_cseg = "prefix"]\n' + (owner.parent / 'rcc' / f'{module}.rs').read_text())
    for module in ['powerup','inventory','quad_pool','pickup_light','scene_pickups']:
        (out/f'{module}.rs').write_text('#![link_cseg = "pickups"]\n'+(owner.parent/'rcc'/f'{module}.rs').read_text())
    (out/'vehicle_startup.rs').write_text('#![link_cseg = "prefix"]\n'+(owner.parent/'rcc/vehicle_startup.rs').read_text())
    for module in ['car_motion','car_effects','vehicle_pose','vehicle_smoke','vehicle_debris','vehicle_shield','track_effects','temporary']:
        (out/f'{module}.rs').write_text('#![link_cseg = "pickups"]\n'+(owner.parent/'rcc'/f'{module}.rs').read_text())
    declarations = ['use crate::dsl_rt::*; mod static_assets;use static_assets::AssetBytes;\n']
    resources = []
    resource_specs = [('CELLS', 'cells.bin', 32768), ('DIRECTORY', 'directory.bin', 128), ('INITIAL', 'initial.bin', 128),
                                ('CONFIG', 'config.bin', 128),
                                ('SINE', 'sine.i16.bin', 128), ('ANGLE', 'angle.i16.bin', 128),
                                ('MESH', 'body.bin', 128), ('CASE_META', 'cases.bin', 128),
                                ('BINDING', 'binding.bin', 128), ('SCENE', 'scene.bin', 128), ('DEFORM', 'deform-bound.bin', 128),
                                ('MESH_CATALOG', 'mesh-catalog.bin', 128), ('DEFORM_IDS', 'deform-mesh-ids.bin', 128),
                                ('AUXILIARY_SPANS','auxiliary-spans.bin',128), ('DEBRIS', 'debris.bin', 128), ('SYMBOLS', 'symbols.bin', 128), ('CAR_BINDINGS','car-bindings.bin',128), ('PARENTS', 'parent-bindings.bin', 128), ('MOTION_LAYOUT', 'motion-layout.bin', 128),
                                ('INVERSE_COSINE', 'cosine-angle.bin', 128),
                                ('SCENERY', 'scenery.bin', 128), ('IDENTITY', 'identity.bin', 128), ('LIFETIME', 'lifetime.bin', 128)]
    resource_specs += [(f'FRAME_{i}', f'frames-{i}.bin', 128) for i in range(len(data['cases']))]
    surface_cases = [i for i,c in enumerate(data['cases']) if c.get('scene')]
    resource_specs += [(f'SURFACE_{i}', f'surfaces-{i}.bin', 128) for i in surface_cases]
    resource_specs += [(f'MESH_VERSIONS_{i}', f'mesh-versions-{i}.bin', 128) for i in surface_cases]
    parent_cases = [i for i,c in enumerate(data['cases']) if c['name'].startswith(('auxiliary-', 'debris-', 'prefix-', 'pickup-')) or c['name'] in ['complete-wall','complete-prepare']]
    resource_specs += [(f'PARENT_FRAMES_{i}', f'parents-{i}.bin', 128) for i in parent_cases]
    debris_cases = [i for i,c in enumerate(data['cases']) if c['name'].startswith('debris-')]
    resource_specs += [(f'DEBRIS_FRAMES_{i}', f'debris-{i}.bin', 128) for i in debris_cases]
    scenery_cases = [i for i,c in enumerate(data['cases']) if c.get('scenery')]
    resource_specs += [(f'SCENERY_{i}', f'scenery-{i}.bin', 128) for i in scenery_cases]
    prefix_cases=[i for i,c in enumerate(data['cases']) if c['prefix_initial']]
    spin_cases=[i for i,c in enumerate(data['cases']) if c['name'].startswith('prefix-') or c['name']=='complete-prepare']
    resource_specs.append(('PREFIX_CONFIG','prefix-config.bin',128))
    if prefix_cases:
        resource_specs += [(f'PREFIX_{i}',f'prefix-{i}.bin',128) for i in prefix_cases]
        resource_specs += [(f'PREFIX_INITIAL_{i}',f'prefix-initial-{i}.bin',128) for i in prefix_cases]
    pickup_cases=[i for i,c in enumerate(data['cases']) if c['name'].startswith('pickup-')]
    resource_specs += [('PICKUP_BINDINGS','pickup-bindings.bin',128),('PICKUP_CONFIG','pickup-config.bin',128)]
    resource_specs += [(f'PICKUP_{i}',f'pickup-{i}.bin',128) for i in pickup_cases]
    resource_specs += [(f'PICKUP_INITIAL_{i}',f'pickup-initial-{i}.bin',128) for i in pickup_cases]
    complete_cases=[i for i,c in enumerate(data['cases']) if c['name'].startswith('complete-')]
    startup_cases=[i for i,c in enumerate(data['cases']) if c['name']=='complete-startup']
    resource_specs.append(('STARTUP_MODELS','startup-models.bin',128))
    resource_specs += [(f'COMPLETE_INITIAL_{i}',f'complete-initial-{i}.bin',128) for i in complete_cases]
    resource_specs += [(f'COMPLETE_{i}',f'complete-{i}.bin',128) for i in complete_cases]
    initial_symbols = {}
    initial_images = {(out / 'scenery.bin').read_bytes(): 'DRIVING_SCENERY'}
    for i in scenery_cases:
        name = data['cases'][i]['scenery_initial']
        image = (out / name).read_bytes()
        if image not in initial_images:
            symbol = f'SCENERY_INIT_{i}'
            resource_specs.append((symbol, name, 128))
            initial_images[image] = 'DRIVING_' + symbol
        initial_symbols[i] = initial_images[image]
    for symbol, name, align in resource_specs:
        path = out / name
        declarations.append(f'#[asset(align={align})] pub static DRIVING_{symbol}:AssetBytes='
                            f'AssetBytes::new(include_bytes!({json.dumps(path.as_posix())}));\n')
        resources.append(dict(file=name, placement='source', symbol='DRIVING_' + symbol,
                              bytes=path.stat().st_size, sha256=hashlib.sha256(path.read_bytes()).hexdigest()))
    declarations.append('pub const DRIVING_COMPLETE_WORDS:u16=10468;\n')
    declarations.append('pub fn driving_has_startup(case:u16)->bool {\n'+(' || '.join(f'case=={i}' for i in startup_cases) or 'false')+'\n}\n')
    declarations.append('pub fn driving_has_complete(case:u16)->bool {\n'+(' || '.join(f'case=={i}' for i in complete_cases) or 'false')+'\n}\n')
    for fn,symbol in [('driving_complete_initial','COMPLETE_INITIAL'),('driving_complete_frames','COMPLETE')]:
        declarations.append(f'pub fn {fn}(case:u16)->AssetBytes {{\n')
        for i in complete_cases:
            declarations.append(f'if case=={i} {{return DRIVING_{symbol}_{i};}}\n')
        declarations.append('DRIVING_CONFIG\n}\n')
    declarations.append('pub fn driving_has_pickup(case:u16)->bool {\n'+(' || '.join(f'case=={i}' for i in pickup_cases) or 'false')+'\n}\n')
    for fn,symbol in [('driving_pickup_frames','PICKUP'),('driving_pickup_initial','PICKUP_INITIAL')]:
        declarations.append(f'pub fn {fn}(case:u16)->AssetBytes {{\n')
        for i in pickup_cases:
            declarations.append(f'if case=={i} {{return DRIVING_{symbol}_{i};}}\n')
        declarations.append('DRIVING_CONFIG\n}\n')
    declarations.append('pub fn driving_has_prefix(case:u16)->bool {\n' + (' || '.join(f'case=={i}' for i in prefix_cases) or 'false') + '\n}\n')
    declarations.append('pub fn driving_starts_spin(case:u16)->bool {\n' + (' || '.join(f'case=={i}' for i in spin_cases) or 'false') + '\n}\n')
    for fn, symbol in [('driving_prefix_frames','PREFIX'),('driving_prefix_initial','PREFIX_INITIAL')]:
        declarations.append(f'pub fn {fn}(case:u16)->AssetBytes {{\n')
        for i in prefix_cases:
            declarations.append(f'if case=={i} {{return DRIVING_{symbol}_{i};}}\n')
        declarations.append('DRIVING_CONFIG\n}\n')
    declarations.append('pub fn driving_frames(case:u16)->AssetBytes {\n')
    for i in range(len(data['cases']) - 1):
        declarations.append(f'if case=={i} {{return DRIVING_FRAME_{i};}}\n')
    declarations.append(f'DRIVING_FRAME_{len(data["cases"])-1}\n}}\n')
    declarations.append('pub fn driving_surfaces(case:u16)->AssetBytes {\n')
    for i in surface_cases[:-1]:
        declarations.append(f'if case=={i} {{return DRIVING_SURFACE_{i};}}\n')
    declarations.append((f'DRIVING_SURFACE_{surface_cases[-1]}' if surface_cases else 'DRIVING_CONFIG')+'\n}\n')
    declarations.append('pub fn driving_parent_frames(case:u16)->AssetBytes {\n')
    for i in parent_cases[:-1]:
        declarations.append(f'if case=={i} {{return DRIVING_PARENT_FRAMES_{i};}}\n')
    declarations.append((f'DRIVING_PARENT_FRAMES_{parent_cases[-1]}' if parent_cases else 'DRIVING_CONFIG')+'\n}\n')
    declarations.append('pub fn driving_has_parents(case:u16)->bool {\n')
    declarations.append((' || '.join(f'case=={i}' for i in parent_cases) or 'false') + '\n}\n')
    declarations.append('pub fn driving_has_debris(case:u16)->bool {\n' + (' || '.join(f'case=={i}' for i in debris_cases) or 'false') + '\n}\n')
    declarations.append('pub fn driving_debris_frames(case:u16)->AssetBytes {\n')
    for i in debris_cases[:-1]:
        declarations.append(f'if case=={i} {{return DRIVING_DEBRIS_FRAMES_{i};}}\n')
    declarations.append((f'DRIVING_DEBRIS_FRAMES_{debris_cases[-1]}' if debris_cases else 'DRIVING_CONFIG') + '\n}\n')
    declarations.append('pub fn driving_mesh_versions(case:u16)->AssetBytes {\n')
    for i in surface_cases[:-1]:
        declarations.append(f'if case=={i} {{return DRIVING_MESH_VERSIONS_{i};}}\n')
    declarations.append((f'DRIVING_MESH_VERSIONS_{surface_cases[-1]}' if surface_cases else 'DRIVING_CONFIG')+'\n}\n')
    declarations.append('pub fn driving_scenery(case:u16)->AssetBytes {\n')
    for i in scenery_cases[:-1]:
        declarations.append(f'if case=={i} {{return DRIVING_SCENERY_{i};}}\n')
    declarations.append((f'DRIVING_SCENERY_{scenery_cases[-1]}' if scenery_cases else 'DRIVING_CONFIG')+'\n}\n')
    declarations.append('pub fn driving_initial_scenery(case:u16)->AssetBytes {\n')
    for i, symbol in initial_symbols.items():
        if symbol != 'DRIVING_SCENERY':
            declarations.append(f'if case=={i} {{return {symbol};}}\n')
    declarations.append('DRIVING_SCENERY\n}\n')
    live_words = (out / 'scenery.bin').stat().st_size // 2
    assert live_words < 8192
    assert all(len(frame['scenery']) < 65536 for c in data['cases'] if c.get('scenery') for frame in c['frames'])
    declarations.append(f'pub const DRIVING_CASES:u16={len(data["cases"])};\n')
    declarations.append(f'pub const DRIVING_CATALOG_WORDS:u16={catalog_words};\n')
    declarations.append(f'pub const DRIVING_PARENT_WORDS:u16={len(parent_words)};\n')
    for name, value in [('CONFIG_WORDS', len(config)), ('STATE_VALUES', state_values),
                        ('INITIAL_WORDS', initial_words), ('FRAME_WORDS', frame_words),
                        ('GUN_WORDS', data['gun_words']), ('MESH_WORDS', len(data['body_data'])),
                        ('BODY_VERTICES', data['body_vertices']), ('BODY_FACES', data['body_faces']),
                        ('DEFORM_WORDS', (out / 'deform-bound.bin').stat().st_size // 2), ('SURFACE_VALUES', 749),
                        ('SCENERY_WORDS', live_words), ('IDENTITY_WORDS', (out / 'identity.bin').stat().st_size // 2),
                        ('SYMBOL_WORDS', (out / 'symbols.bin').stat().st_size // 2), ('LIFETIME_WORDS', (out / 'lifetime.bin').stat().st_size // 2)]:
        declarations.append(f'pub const DRIVING_{name}:u16={value};\n')
    declarations.append(f'pub static DRIVING_LIVE:Buf<u16,{live_words}>=Buf::new([0;{live_words}]);\n')
    assert initial_words <= frame_words
    assert (out / 'lifetime.bin').stat().st_size // 2 <= frame_words
    storage = [('gun', 'i16', data['gun_words'] * 2),
               ('body', 'u16', len(data['body_data']) * 2),
               ('deform', 'u16', (out / 'deform-bound.bin').stat().st_size // 2),
               ('identity', 'u16', (out / 'identity.bin').stat().st_size // 2),
               ('lifetime_lists', 'u16', 2080), ('catalog', 'u16', catalog_words),
               ('debris_directory', 'u16', 60)]
    declarations.append('pub struct DrivingStorage {\n')
    for name, kind, count in storage:
        declarations.append(f'pub {name}:Buf<{kind},{count}>,\n')
    declarations.append('}\npub fn driving_storage()->DrivingStorage { DrivingStorage {\n')
    for name, _, count in storage:
        declarations.append(f'{name}:Buf::new([0;{count}]),\n')
    declarations.append('}}\n')
    (out / 'driving_assets.rs').write_text(''.join(declarations))
    sources = [owner / n for n in ['driving_oracle.rs', 'driving_ground.rs', 'driving_capture.rs', 'sampling_snapshot.rs', 'reference_contact.rs', 'paired_driving_oracle.rs', 'reference_world.rs', 'reference_bridge.py', 'reference_tanks.rs', 'reference_car.rs', 'impact_snapshot.rs', 'vehicle_oracle.rs', 'drive_oracle.rs', 'terrain_assets.rs', 'driving_assets.py']]
    sources += [owner / n for n in ['scene_export.rs', 'deform_assets.rs', 'driving_step.rs', 'scenery_assets.rs',
        'scenery_frame.rs', 'identity_assets.rs', 'identity_capture.rs', 'reference_scene.rs',
        'parent_scene_assets.rs', 'prefix_scene_assets.rs', 'pickup_scene_assets.rs', 'pickup_snapshot.rs', 'driving_devices.rs', 'debris_scene_assets.rs', 'reference_symbols.rs', 'complete_scene_assets.rs', 'complete_driving_step.rs', 'vehicle_startup_assets.rs']]
    sources.append(inverse)
    sources += [Path(row['file']) for row in json.loads((catalog_reference / 'mesh-store-sources.json').read_text())['sources']]
    sources += [root / 'crates/game/src' / n for n in ['vehicle_runtime/car_objects.rs', 'scene_collision.rs', 'deformable_hit.rs', 'deformation.rs', 'obstacle_collision.rs']]
    sources += [root / 'crates/game/src' / n for n in ['contact_pipeline.rs', 'sampling.rs', 'suspension.rs', 'pose.rs', 'body.rs', 'motion.rs', 'vehicle_model.rs', 'random.rs', 'ground_collision.rs', 'impact_particles.rs', 'effect_pool.rs', 'damage.rs']]
    (out / 'assets.json').write_text(json.dumps(dict(resources=resources, sector_relocations=[], steps=steps,
        cases=[dict(name=c['name'], cars=c['cars'], updates=len(c['frames']), cells=c['cells'], fallback_contacts=c['fallback_contacts'], tank_contacts=c['tank_contacts'], deform_contacts=c.get('deform_contacts', 0),
                    deformed_vertices=c.get('deformed_vertices', 0), surface_sample_points=c.get('surface_sample_points', 0), deform_spark_events=c.get('deform_spark_events', 0), ordinary_contacts=c.get('ordinary_contacts', 0), scenery_ticks=c.get('scenery_ticks', 0), retirement=c.get('retirement', False), transient=c.get('transient', False)) for c in data['cases']], updates=updates, scope=data['scope'], checked_i32_per_frame=state_values+1,
        checked_surface_i32=sum(len(c['frames']) * 749 for c in data['cases'] if c.get('scene')),
        resource_version_words=sum(len(c['frames']) * 28 for c in data['cases'] if c.get('scene')),
        checked_pickup_words=sum(len(f.get('pickup',[])) for c in data['cases'] for f in c['frames']),
        checked_prefix_words=sum(len(f.get('prefix',[])) for c in data['cases'] for f in c['frames']),
        checked_debris_words=sum(len(f.get('debris',[])) for c in data['cases'] for f in c['frames']),
        checked_complete_words=sum(len(f.get('complete',[])) for c in data['cases'] for f in c['frames']), qualification_scope=qualification_scope, checked_parent_words=sum(len(f.get('parents', [])) for c in data['cases'] for f in c['frames']),
        catalog_initial_slots=catalog['initial_slots'],catalog_payload_bytes=catalog['initial_payload_bytes'],
        scenery_frames=sum(len(c['frames']) for c in data['cases'] if c.get('scenery')),
        checked_scenery_words=checked_scenery_words,
        sources=[dict(file=str(p), sha256=hashlib.sha256(p.read_bytes()).hexdigest()) for p in sources]), indent=2) + '\n')


if __name__ == '__main__':
    main()
