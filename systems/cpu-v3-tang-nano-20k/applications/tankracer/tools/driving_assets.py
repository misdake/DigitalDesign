"""Build raw-map driving qualification assets and unmodified reference answers."""
import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import struct
import subprocess
import sys
from terrain_map_pack import pack_map
from reference_bridge import prepare
from reference_player import append_player_probe
from scene_boot_assets import build as build_scene_boot


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
    append_player_probe(root, out, game)
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
    player_catalog_reference = out / 'player-catalog-reference'
    subprocess.run([sys.executable, str(owner / 'mesh_store_assets.py'), '--reference',
                    str(root / 'Cargo.toml'), '--output', str(player_catalog_reference), '--player'], check=True, timeout=400)
    (out / 'player-mesh-catalog.bin').write_bytes((player_catalog_reference / 'mesh-catalog.bin').read_bytes())
    player_catalog = json.loads((player_catalog_reference / 'mesh-catalog.json').read_text())
    catalog_words = max(catalog_words, 2560 + player_catalog['source_metadata_bytes'] // 2 + player_catalog['compact_index_bytes'] // 2)
    for field in ['motion_layout', 'auxiliary_spans']:
        assert catalog[field] == player_catalog[field], field
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
    player_pickup_words = list(pickup_words)
    for i, row in enumerate(pickups):
        name = row['name'].lower()
        player_pickup_words[i*26+2] = player_catalog['auxiliary_sources'].get(name, 65535)
        player_pickup_words[i*26+3] = player_catalog['names'].index(name) if name else 65535
    binary(out/'player-pickup-bindings.bin', player_pickup_words)
    pickup_config=json.loads((out/'pickup-config.json').read_text())
    assert len(pickup_config)==46
    binary(out/'pickup-config.bin',words(pickup_config[:5])+pickup_config[5:]+words([5,6])+words([0,0,0,4096,0,0,0,4096,0,0,0,4096]))
    binary(out / 'parent-bindings.bin', parent_words)
    player_parents = list(parent_words)
    for i, row in enumerate(parents):
        name = row['name'].lower()
        player_parents[8 + i * 26 + 2] = player_catalog['auxiliary_sources'].get(name, 65535)
        player_parents[8 + i * 26 + 3] = player_catalog['names'].index(name) if name else 65535
    binary(out / 'player-parent-bindings.bin', player_parents)
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
        # The second diagnostic car is frozen at the duplicated initial row.
        # Its identity and visibility do not participate in the trajectory.
        frozen = list(case['car_bindings'])
        frozen[:3] = [2, 0, 1]
        case['car_bindings'] += frozen
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
    whole_map = (out / 'map-cells.bin').read_bytes()
    for cell, block in cells.items():
        if whole_map[cell * 32768:(cell + 1) * 32768] != block:
            raise ValueError(f'whole-map collision cell differs: {cell}')
    terrain_map = pack_map(whole_map, out)
    environment=data['cases'][0]['environment']
    data['cases'] += paired['cases']
    qualification_scope=os.environ.get('TANKRACER_DRIVING_SCOPE','full')
    finish_steps = 48 if qualification_scope=='finish-ufo' else 4
    finish_checks = json.loads((out/'finish-run-checks.json').read_text()) if qualification_scope in ('finish-run','finish-world','finish-ufo') else []
    if qualification_scope=='full':
        # Keep the baseline and impact suites independently within asset banks.
        # The long slot scan is qualified separately through --scope complete.
        data['cases']=[c for c in data['cases'] if c['name']!='complete-slot-gap' and not c['name'].startswith(('complete-player-ground','complete-player-hit','complete-player-mines','complete-player-ufo','complete-player-emitter','complete-player-train','complete-player-ambient','complete-player-flock','complete-player-lap-crush','complete-player-world'))]
    elif qualification_scope in ('finish-transfer','finish-tick','finish-run','finish-world','finish-ufo'):
        data['cases']=[c for c in data['cases'] if c['name']=='complete-player-world-cold']
    elif qualification_scope=='world':
        data['cases']=[c for c in data['cases'] if c.get('world_pass')]
    elif qualification_scope=='startup':
        data['cases']=[c for c in data['cases'] if c['name']=='complete-startup']
    elif qualification_scope=='debris':
        data['cases']=[c for c in data['cases'] if c['name'] in ['debris-169','debris-peer-blast','debris-retire']]
    elif qualification_scope=='fast-controls':
        data['cases']=[c for c in data['cases'] if c['name']=='complete-fast-controls']
    elif qualification_scope=='dynamic':
        data['cases']=[c for c in data['cases'] if c['name']=='complete-dynamic']
    elif qualification_scope=='player':
        data['cases']=[c for c in data['cases'] if c.get('player_initial')]
    elif qualification_scope=='clock':
        data['cases']=[c for c in data['cases'] if c.get('clock_initial')]
    elif qualification_scope=='tail':
        data['cases']=[c for c in data['cases'] if c['name'] in ['complete-player-horn','complete-player-hidden']]
    elif qualification_scope=='contact':
        data['cases']=[c for c in data['cases'] if c['name'].startswith('complete-player-hit')]
    elif qualification_scope=='ufos':
        data['cases']=[c for c in data['cases'] if (c['name'].startswith('complete-player-ufo') and not c['name'].startswith('complete-player-ufo-object')) or c['name']=='complete-player-hit-ufo']
    elif qualification_scope=='object-ufos':
        data['cases']=[c for c in data['cases'] if c['name'].startswith('complete-player-ufo-object')]
    elif qualification_scope=='lap-surfaces':
        data['cases']=[c for c in data['cases'] if c['name'].startswith('complete-player-lap-crush')]
    elif qualification_scope=='flocks':
        data['cases']=[c for c in data['cases'] if c['name'].startswith('complete-player-flock')]
    elif qualification_scope=='ambient':
        data['cases']=[c for c in data['cases'] if c['name'].startswith('complete-player-ambient')]
    elif qualification_scope=='trains':
        data['cases']=[c for c in data['cases'] if c['name'].startswith('complete-player-train')]
    elif qualification_scope=='emitters':
        data['cases']=[c for c in data['cases'] if c['name'].startswith('complete-player-emitter')]
    elif qualification_scope=='mines':
        data['cases']=[c for c in data['cases'] if c['name'].startswith('complete-player-mines')]
    elif qualification_scope=='impact':
        data['cases']=[c for c in data['cases'] if c['name'].startswith('complete-player-ground')]
    elif qualification_scope=='flight':
        data['cases']=[c for c in data['cases'] if c['name'] in ['complete-player','complete-player-homing','complete-player-warp']]
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
    selected_case = os.environ.get('TANKRACER_DRIVING_CASE')
    if selected_case is not None:
        index = int(selected_case)
        if index < 0 or index >= len(data['cases']):
            raise ValueError('case index outside the selected driving scope')
        data['cases'] = [data['cases'][index]]
    data['scope'] = ('persistent real terrain contact, suspension/power, ordered ordinary/tank/deformable response, '
                     'ground reflection/impact/damage, motion, articulation, lean, live gun/body vertices, '
                     'contact hull, shared impact animation/pool and RNG; original ordered physical '
                     'car/scenery ticks with complete live-world comparisons, constructed mesh owner, '
                     'shared Crush hit/sampling vertices and resource revisions, authored Fall/Closer '
                     'fragment hit/tick/resource retirement on the same owner, complete vehicle prepare, '
                     'pickup flight/collection, inventory and shared quad/light state; '
                     'unrelated actors frozen in qualifier initial state')
    if qualification_scope in ('finish-transfer','finish-tick','finish-run','finish-world','finish-ufo'):
        data['scope'] = ('Cold ENGLAND world followed by actual finish ownership publication, deep vehicle clone, '
                         'pickup/debris retargeting and brake-light release; original first-Tick dispatch failure. '
                         'Successful first Tick remains a separate integration boundary.')
    if qualification_scope in ('finish-tick','finish-run','finish-world','finish-ufo'):
        data['scope'] = ('Cold ENGLAND world followed by actual finish takeover and its immediate successful AITank Tick, '
                         'then old Player stop controls; complete third-car, shared effects and world snapshots.')
    if qualification_scope in ('finish-run','finish-world','finish-ufo'):
        data['scope'] = ('Cold ENGLAND world, successful finish takeover and four complete original world frames: '
                         'activation/Stop, far driving, middle rejoin, near wake after frame 499 and continued physical driving.')
    if qualification_scope == 'finish-ufo':
        data['scope'] = ('Ordered ENGLAND finish takeover followed by an original UFO targeting the new owner, '
                         'with retired-car separation and 48 complete world frames at dt=112; '
                         'snapshots at phase transitions, every eighth frame and the final frame.')
    if qualification_scope == 'world':
        data['scope'] = ('Loaded ENGLAND world with the complete authored Tick order, both non-AI vehicle '
                         'updates, raw race clock, shared RNG, live pickup flight and all scene owners; '
                         'independent input/time sequences. Cold-scene cases construct actor state and '
                         'memberships from authored inputs on the startup RNG; other cases use '
                         'controlled initial vehicle poses, velocity and RNG.')
    deform_data = (out/'deform-bound.bin').read_bytes()
    deform_count = struct.unpack_from('<H',deform_data,8)[0]//40
    assert deform_count == 10
    cold_cases = [i for i,c in enumerate(data['cases']) if c.get('cold_scene')]
    if cold_cases:
        build_scene_boot(root, out)
    surface_values = sum(5+struct.unpack_from('<H',deform_data,(i*40+1)*2)[0]*3 for i in range(deform_count))
    state_values = len(data['cases'][0]['initial'])
    initial_words = state_values * 2 + 18
    frame_words = 60 + (state_values + 1) * 2
    camera_cases = [i for i,c in enumerate(data['cases']) if c['name'].startswith(('complete-player-emitter','complete-player-ambient','complete-player-flock','complete-player-lap-crush'))]
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
        metadata.extend([len(case['frames']), (1 if case.get('ordered',True) else 2) if case.get('scenery',False) else 0])
        metadata.extend([case.get('retire_object', 0), case.get('retire_frame', 65535)])
        group = []
        for frame in case['frames']:
            control = list(frame['control'])
            assert len(control) == 30 and len(frame['expected']) == state_values + 1, case['name']
            if case.get('scene'):
                assert len(frame['surface']) == surface_values, case['name']
            # Only independent input reaches the target. Sampling and boost publish these slots.
            control[5:19] = [0] * 14
            group.extend(words(control))
            group.extend(words(frame['expected']))
        binary(out / f'frames-{case_index}.bin', group)
        if case['name'] == 'complete-dynamic' or case['name'].startswith(('complete-player-mines','complete-player-ufo')):
            binary(out / f'world-events-{case_index}.bin', [v for frame in case['frames'] for v in frame['world_event']])
        if case_index in camera_cases:
            binary(out / f'camera-{case_index}.bin', words([v for frame in case['frames'] for v in frame['camera']]))
        if case.get('player_initial'):
            binary(out / f'player-{case_index}.bin', case['player_initial'] + [v for frame in case['frames'] for v in frame['player']])
        if case.get('clock_initial'):
            binary(out / f'clock-{case_index}.bin', case['clock_initial'] + [v for frame in case['frames'] for v in frame['clock']])
        if case.get('world_pass'):
            assert all(len(frame['pickup_flights']) == 810 for frame in case['frames'])
            binary(out / f'world-flights-{case_index}.bin', words([v for frame in case['frames'] for v in frame['pickup_flights']]))
            assert all(len(f['camera']) == 3 and len(f['camera_basis']) == 9 for f in case['frames'])
            binary(out / f'world-view-{case_index}.bin', words([v for f in case['frames'] for v in f['camera']+f['camera_basis']]))
        if case.get('scene'):
            binary(out / f'surfaces-{case_index}.bin', words([v for frame in case['frames'] for v in frame['surface']]))
        if case.get('scenery'):
            binary(out / f'scenery-{case_index}.bin', [v for frame in case['frames'] for v in frame['scenery']])
        if case.get('scene'):
            assert all(len(frame['resource_versions']) == deform_count*4 for frame in case['frames'])
            binary(out / f'mesh-versions-{case_index}.bin', [v for frame in case['frames'] for v in frame['resource_versions']])
        if case.get('player_initial') or case['name'].startswith(('auxiliary-', 'debris-', 'prefix-', 'pickup-')) or case['name'] in ['complete-wall','complete-prepare','complete-dynamic']:
            binary(out / f'parents-{case_index}.bin', [v for frame in case['frames'] for v in frame['parents']])
        if case['name'].startswith('debris-'):
            binary(out / f'debris-{case_index}.bin', [v for frame in case['frames'] for v in frame['debris']])
        if case.get('prefix_initial'):
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
    # The diagnostic snapshot also contains derived state that load_initial
    # recomputes. Store only its independent inputs, sharing identical rows.
    car_words = 542 + data['gun_words'] * 2
    start_words = car_words * 2 + 42
    start_images = {}
    start_data = []
    start_offsets = []
    for case in range(len(data['cases'])):
        row = initial[case * initial_words:(case + 1) * initial_words]
        packed = []
        for begin, count in [(0, car_words), (3566, car_words),
                             (state_values * 2, 18), (602, 2), (4156, 2),
                             (4558, 6), (4564, 6), (4572, 4), (4576, 4)]:
            packed += row[begin:begin + count]
        assert len(packed) == start_words
        key = tuple(packed)
        if key not in start_images:
            start_images[key] = len(start_data)
            start_data += packed
        start_offsets.append(start_images[key])
    assert len(start_data) <= 65536
    binary(out / 'initial.bin', start_data)
    binary(out / 'seeds.bin', [0x5253,1,len(data['cases']),0] + words([c.get('seed_before_noise',0x1234abcd) for c in data['cases']]))
    binary(out / 'initial-offsets.bin', start_offsets)
    binary(out / 'config.bin', config)
    binary(out / 'body.bin', [v & 65535 for v in data['body_data']])
    binary(out / 'cases.bin', metadata)
    binary(out / 'binding.bin', words(data['collision_binding']))
    subprocess.run([sys.executable, str(owner / 'export_trig_assets.py'), '--reference', str(root), '--output', str(out)], check=True)
    (out / 'trig_lookup.rs').write_bytes((owner.parent / 'rcc/trig_asset_lookup.rs').read_bytes())
    for module in ['scene_parent', 'scene_fragments', 'scene_meshes', 'mesh_storage',
                   'mesh_catalog', 'mesh_break', 'scene_auxiliaries', 'closer', 'inverse_basis', 'shatter', 'debris_catalog', 'scene_debris','scene_ufo_transfer','scene_vehicle_transfer','scene_obstacle_transfer','vehicle_transfer','emitter','scene_emitter','train','scene_train','smoker','scene_ambient','scene_flock','flock','scene_initial','scene_actor_initial']:
        source = (owner.parent / 'rcc' / f'{module}.rs').read_text()
        (out / f'{module}.rs').write_text('#![link_cseg = "parents"]\n' + source)
    for module in ['prefix','spin','vehicle_inactive','player_control']:
        (out / f'{module}.rs').write_text('#![link_cseg = "prefix"]\n' + (owner.parent / 'rcc' / f'{module}.rs').read_text())
    for module in ['scene_world', 'scene_lifecycle', 'entities', 'lifecycle']:
        (out / f'{module}.rs').write_text('#![link_cseg = "world"]\n' + (owner.parent / 'rcc' / f'{module}.rs').read_text())
    for module in ['scene_creation','scene_mines','scene_explosions','scene_fire','scene_laps','scene_player','player_route','player_tail','scene_projectiles','scene_projectile_impact','scene_ufos','tank_hit','projectile','projectile_sweep','projectile_flight','projectile_trail','projectile_ricochet','projectile_marks','explosion','explosion_blasts','mine','particle_sprite']:
        (out / f'{module}.rs').write_text('#![link_cseg = "world"]\n' + (owner.parent / 'rcc' / f'{module}.rs').read_text())
    for module in ['powerup','inventory','quad_pool','pickup_light','scene_pickups']:
        (out/f'{module}.rs').write_text('#![link_cseg = "pickups"]\n'+(owner.parent/'rcc'/f'{module}.rs').read_text())
    (out/'vehicle_startup.rs').write_text('#![link_cseg = "prefix"]\n'+(owner.parent/'rcc/vehicle_startup.rs').read_text())
    for module in ['car_motion','car_effects','vehicle_pose','vehicle_smoke','vehicle_debris','vehicle_shield','track_effects','temporary']:
        (out/f'{module}.rs').write_text('#![link_cseg = "pickups"]\n'+(owner.parent/'rcc'/f'{module}.rs').read_text())
    declarations = ['use crate::dsl_rt::*; mod static_assets;use static_assets::AssetBytes;\n']
    declarations.append('pub fn driving_orthogonal(case:u16)->bool {\n')
    for i, case in enumerate(data['cases']):
        if case.get('attitude_orthogonal', False):
            declarations.append(f'if case=={i} {{return true;}}\n')
    declarations.append('false\n}\n')
    resources = []
    resource_specs = [('CELLS', 'cells.bin', 32768), ('DIRECTORY', 'directory.bin', 128), ('INITIAL', 'initial.bin', 128),
                                ('INITIAL_OFFSETS', 'initial-offsets.bin', 128), ('SEEDS','seeds.bin',128),
                                ('CONFIG', 'config.bin', 128),
                                ('SINE', 'sine.i16.bin', 128), ('ANGLE', 'angle.i16.bin', 128),
                                ('MESH', 'body.bin', 128), ('CASE_META', 'cases.bin', 128),
                                ('BINDING', 'binding.bin', 128), ('SCENE', 'scene.bin', 128), ('DEFORM', 'deform-bound.bin', 128),
                                ('MESH_CATALOG', 'mesh-catalog.bin', 128), ('DEFORM_IDS', 'deform-mesh-ids.bin', 128),
                                ('AUXILIARY_SPANS','auxiliary-spans.bin',128), ('DEBRIS', 'debris.bin', 128), ('SYMBOLS', 'symbols.bin', 128), ('CAR_BINDINGS','car-bindings.bin',128), ('PARENTS', 'parent-bindings.bin', 128), ('MOTION_LAYOUT', 'motion-layout.bin', 128),
                                ('INVERSE_COSINE', 'cosine-angle.bin', 128),
                                ('SCENERY', 'scenery.bin', 128), ('IDENTITY', 'identity.bin', 128), ('LIFETIME', 'lifetime.bin', 128)]
    resource_specs.append(('FINISH_SCENE', 'finish-scene.bin', 128))
    resource_specs += [(f'FRAME_{i}', f'frames-{i}.bin', 128) for i in range(len(data['cases']))]
    surface_cases = [i for i,c in enumerate(data['cases']) if c.get('scene')]
    resource_specs += [(f'SURFACE_{i}', f'surfaces-{i}.bin', 128) for i in surface_cases]
    resource_specs += [(f'MESH_VERSIONS_{i}', f'mesh-versions-{i}.bin', 128) for i in surface_cases]
    parent_cases = [i for i,c in enumerate(data['cases']) if c.get('player_initial') or c['name'].startswith(('auxiliary-', 'debris-', 'prefix-', 'pickup-')) or c['name'] in ['complete-wall','complete-prepare','complete-dynamic']]
    resource_specs += [(f'PARENT_FRAMES_{i}', f'parents-{i}.bin', 128) for i in parent_cases]
    debris_cases = [i for i,c in enumerate(data['cases']) if c['name'].startswith('debris-')]
    resource_specs += [(f'DEBRIS_FRAMES_{i}', f'debris-{i}.bin', 128) for i in debris_cases]
    scenery_cases = [i for i,c in enumerate(data['cases']) if c.get('scenery')]
    resource_specs += [(f'SCENERY_{i}', f'scenery-{i}.bin', 128) for i in scenery_cases]
    dynamic_cases = [i for i,c in enumerate(data['cases']) if c['name'] == 'complete-dynamic' or c['name'].startswith(('complete-player-mines','complete-player-ufo'))]
    resource_specs += [(f'WORLD_EVENTS_{i}', f'world-events-{i}.bin', 128) for i in dynamic_cases]
    player_cases = [i for i,c in enumerate(data['cases']) if c.get('player_initial')]
    clock_cases = [i for i,c in enumerate(data['cases']) if c.get('clock_initial')]
    resource_specs += [(f'CLOCK_{i}',f'clock-{i}.bin',128) for i in clock_cases]
    resource_specs += [(f'CAMERA_{i}', f'camera-{i}.bin', 128) for i in camera_cases]
    resource_specs += [('FLOCK','flock-initial.bin',128),('PLAYER_DEFORM_IDS','player-deform-mesh-ids.bin',128),('OBJECT_MESHES','object-meshes.bin',128),('PLAYER_PICKUP_FLIGHTS','player-pickup-flights.bin',128)]
    world_cases = [i for i,c in enumerate(data['cases']) if c.get('world_pass')]
    resource_specs += [(f'WORLD_FLIGHTS_{i}',f'world-flights-{i}.bin',128) for i in world_cases]
    resource_specs += [(f'WORLD_VIEW_{i}',f'world-view-{i}.bin',128) for i in world_cases]
    if player_cases:
        (out/'player-context.bin').write_bytes((out/data['cases'][player_cases[0]]['player_context']).read_bytes())
        (out/'player-scene.bin').write_bytes((out/data['cases'][player_cases[0]]['player_scene']).read_bytes())
    resource_specs += [('PLAYER_CONTEXT','player-context.bin',128)]
    resource_specs += [(f'PLAYER_CONTEXT_{i}',data['cases'][i]['player_context'],128) for i in player_cases]
    resource_specs += [('PLAYER_SCENE','player-scene.bin',128)]
    resource_specs += [(f'PLAYER_SCENE_{i}',data['cases'][i]['player_scene'],128) for i in player_cases]
    resource_specs += [('PLAYER_MESH_CATALOG','player-mesh-catalog.bin',128),
                       ('PLAYER_PARENTS','player-parent-bindings.bin',128)]
    resource_specs += [(f'PLAYER_{i}',f'player-{i}.bin',128) for i in player_cases]
    prefix_cases=[i for i,c in enumerate(data['cases']) if c.get('prefix_initial')]
    spin_cases=[i for i,c in enumerate(data['cases']) if c['name'].startswith('prefix-') or c['name']=='complete-prepare']
    resource_specs.append(('PREFIX_CONFIG','prefix-config.bin',128))
    if prefix_cases:
        resource_specs += [(f'PREFIX_{i}',f'prefix-{i}.bin',128) for i in prefix_cases]
        resource_specs += [(f'PREFIX_INITIAL_{i}',f'prefix-initial-{i}.bin',128) for i in prefix_cases]
    pickup_cases=[i for i,c in enumerate(data['cases']) if c['name'].startswith('pickup-')]
    resource_specs += [('PICKUP_BINDINGS','pickup-bindings.bin',128),('PLAYER_PICKUP_BINDINGS','player-pickup-bindings.bin',128),('PICKUP_CONFIG','pickup-config.bin',128)]
    resource_specs += [(f'PICKUP_{i}',f'pickup-{i}.bin',128) for i in pickup_cases]
    resource_specs += [(f'PICKUP_INITIAL_{i}',f'pickup-initial-{i}.bin',128) for i in pickup_cases]
    complete_cases=[i for i,c in enumerate(data['cases']) if c['name'].startswith('complete-')]
    startup_cases=[i for i,c in enumerate(data['cases']) if c.get('constructed')]
    resource_specs.append(('STARTUP_MODELS','startup-models.bin',128))
    for i in startup_cases:
        filename=f'startup-models-{i}.bin'
        (out/filename).write_bytes((out/f"startup-models-{data['cases'][i]['name']}.bin").read_bytes())
        resource_specs.append((f'STARTUP_MODELS_{i}',filename,128))
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
    if cold_cases:
        resource_specs += [('SCENE_BOOT','scene-boot.bin',128),('SCENE_BOOT_TILTS','scene-boot-tilts.bin',128),('SCENE_BOOT_EXPECTED','scene-boot-expected.bin',128),('SCENE_BOOT_CAMERA','scene-boot-camera.bin',128),('SCENE_BOOT_DEBRIS','scene-boot-debris.bin',128)]
    if qualification_scope in ('finish-transfer','finish-tick','finish-run','finish-world','finish-ufo'):
        resource_specs += [('FINISH_TRANSFER','finish-transfer.bin',128),
                           ('FINISH_TRANSFER_WORLD','finish-transfer-world.bin',128)]
    if qualification_scope in ('finish-run','finish-world','finish-ufo'):
        resource_specs += [('FINISH_RUN_INPUT','finish-run-input.bin',128)]
        for index in finish_checks:
            resource_specs += [(f'FINISH_RUN_{index}',f'finish-run-{index}.bin',128),
                               (f'FINISH_RUN_WORLD_{index}',f'finish-run-world-{index}.bin',128)]
    declarations.append('pub fn driving_finish_compare(index:u16)->bool {\n'+('index==0'+''.join(f' || index=={i}' for i in finish_checks) if finish_checks else 'true')+'\n}\n')
    declarations.append('pub fn driving_finish_ufo()->bool {\n'+('true' if qualification_scope=='finish-ufo' else 'false')+'\n}\n')
    declarations.append('pub fn driving_finish_world()->bool {\n'+('true' if qualification_scope in ('finish-world','finish-ufo') else 'false')+'\n}\n')
    declarations.append('pub fn driving_finish_input()->AssetBytes {\n'+('DRIVING_FINISH_RUN_INPUT' if qualification_scope in ('finish-run','finish-world','finish-ufo') else 'DRIVING_CONFIG')+'\n}\n')
    declarations.append('pub fn driving_finish_frames()->u16 {\n'+(str(finish_steps+1) if qualification_scope in ('finish-run','finish-world','finish-ufo') else '1' if qualification_scope=='finish-tick' else '0')+'\n}\n')
    declarations.append('pub fn driving_finish_tick(case:u16)->bool {\n'+('case==0' if qualification_scope in ('finish-tick','finish-run','finish-world','finish-ufo') else 'false')+'\n}\n')
    declarations.append('pub fn driving_finish_transfer(case:u16)->bool {\n'
                        + ('case==0' if qualification_scope=='finish-transfer' else 'false')+'\n}\n')
    declarations.append('pub fn driving_finish_transfer_data(index:u16)->AssetBytes {\n'
                        + (''.join(f'if index=={i} {{ return DRIVING_FINISH_RUN_{i}; }}\n' for i in finish_checks) if qualification_scope in ('finish-run','finish-world','finish-ufo') else '') + ('DRIVING_FINISH_TRANSFER' if qualification_scope in ('finish-transfer','finish-tick','finish-run','finish-world','finish-ufo') else 'DRIVING_CONFIG')+'\n}\n')
    declarations.append('pub fn driving_finish_transfer_world(index:u16)->AssetBytes {\n'
                        + (''.join(f'if index=={i} {{ return DRIVING_FINISH_RUN_WORLD_{i}; }}\n' for i in finish_checks) if qualification_scope in ('finish-run','finish-world','finish-ufo') else '') + ('DRIVING_FINISH_TRANSFER_WORLD' if qualification_scope in ('finish-transfer','finish-tick','finish-run','finish-world','finish-ufo') else 'DRIVING_CONFIG')+'\n}\n')
    resource_images = {}
    resource_aliases = {}
    for symbol, name, align in resource_specs:
        path = out / name
        if symbol in ('PLAYER_CONTEXT','PLAYER_SCENE','DEFORM_IDS','PLAYER_DEFORM_IDS') or re.search(r'_\d+$', symbol):
            key = (align, path.read_bytes())
            if key in resource_images:
                resource_aliases['DRIVING_' + symbol] = resource_images[key]
                continue
            resource_images[key] = 'DRIVING_' + symbol
        declarations.append(f'#[asset(align={align})] pub static DRIVING_{symbol}:AssetBytes='
                            f'AssetBytes::new(include_bytes!({json.dumps(path.as_posix())}));\n')
        resources.append(dict(file=name, placement='source', symbol='DRIVING_' + symbol,
                              bytes=path.stat().st_size, sha256=hashlib.sha256(path.read_bytes()).hexdigest()))
    declarations.append('pub const DRIVING_COMPLETE_WORDS:u16=10468;\n')
    declarations.append('pub fn driving_has_cold_scene(case:u16)->bool {\n'+(' || '.join(f'case=={i}' for i in cold_cases) or 'false')+'\n}\n')
    declarations.append('pub fn driving_scene_boot()->AssetBytes {\n'+('DRIVING_SCENE_BOOT' if cold_cases else 'DRIVING_CONFIG')+'\n}\n')
    declarations.append('pub fn driving_scene_boot_tilts()->AssetBytes {\n'+('DRIVING_SCENE_BOOT_TILTS' if cold_cases else 'DRIVING_CONFIG')+'\n}\n')
    declarations.append('pub fn driving_scene_boot_expected()->AssetBytes {\n'+('DRIVING_SCENE_BOOT_EXPECTED' if cold_cases else 'DRIVING_CONFIG')+'\n}\n')
    declarations.append('pub fn driving_scene_boot_camera()->AssetBytes {\n'+('DRIVING_SCENE_BOOT_CAMERA' if cold_cases else 'DRIVING_CONFIG')+'\n}\n')
    declarations.append('pub fn driving_scene_boot_debris()->AssetBytes {\n'+('DRIVING_SCENE_BOOT_DEBRIS' if cold_cases else 'DRIVING_CONFIG')+'\n}\n')
    declarations.append('pub fn driving_has_startup(case:u16)->bool {\n'+(' || '.join(f'case=={i}' for i in startup_cases) or 'false')+'\n}\n')
    declarations.append('pub fn driving_startup_models(case:u16)->AssetBytes {\n')
    for i in startup_cases:
        declarations.append(f'if case=={i} {{return DRIVING_STARTUP_MODELS_{i};}}\n')
    declarations.append('DRIVING_STARTUP_MODELS\n}\n')
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
    declarations.append('pub fn driving_has_camera_input(case:u16)->bool {\n' + (' || '.join(f'case=={i}' for i in camera_cases) or 'false') + '\n}\n')
    declarations.append('pub fn driving_camera_input(case:u16)->AssetBytes {\n')
    for i in camera_cases:
        declarations.append(f'if case=={i} {{return DRIVING_CAMERA_{i};}}\n')
    declarations.append('DRIVING_CONFIG\n}\n')
    declarations.append('pub fn driving_has_world_events(case:u16)->bool {\n' + (' || '.join(f'case=={i}' for i in dynamic_cases) or 'false') + '\n}\n')
    declarations.append('pub fn driving_world_pass(case:u16)->bool {\n' + (' || '.join(f'case=={i}' for i,c in enumerate(data['cases']) if c.get('world_pass')) or 'false') + '\n}\n')
    declarations.append('pub fn driving_world_view(case:u16)->AssetBytes {\n')
    for i in world_cases:
        declarations.append(f'if case=={i} {{return DRIVING_WORLD_VIEW_{i};}}\n')
    declarations.append('DRIVING_CONFIG\n}\n')
    declarations.append('pub fn driving_world_flights(case:u16)->AssetBytes {\n')
    for i in world_cases:
        declarations.append(f'if case=={i} {{return DRIVING_WORLD_FLIGHTS_{i};}}\n')
    declarations.append('DRIVING_CONFIG\n}\n')
    declarations.append('pub fn driving_world_events(case:u16)->AssetBytes {\n')
    for i in dynamic_cases:
        declarations.append(f'if case=={i} {{return DRIVING_WORLD_EVENTS_{i};}}\n')
    declarations.append('DRIVING_CONFIG\n}\n')
    declarations.append('pub fn driving_scene(case:u16)->AssetBytes {\n')
    for i in player_cases:
        declarations.append(f'if case=={i} {{return DRIVING_PLAYER_SCENE_{i};}}\n')
    declarations.append('DRIVING_SCENE\n}\n')
    declarations.append('pub fn driving_deform_ids(case:u16)->AssetBytes {\nif driving_has_player(case) {return DRIVING_PLAYER_DEFORM_IDS;}\nDRIVING_DEFORM_IDS\n}\n')
    declarations.append('pub fn driving_catalog(case:u16)->AssetBytes {\nif driving_has_player(case) {return DRIVING_PLAYER_MESH_CATALOG;}\nDRIVING_MESH_CATALOG\n}\n')
    declarations.append('pub fn driving_parents(case:u16)->AssetBytes {\nif driving_has_player(case) {return DRIVING_PLAYER_PARENTS;}\nDRIVING_PARENTS\n}\n')
    declarations.append('pub fn driving_player_context(case:u16)->AssetBytes {\n')
    for i in player_cases:
        declarations.append(f'if case=={i} {{return DRIVING_PLAYER_CONTEXT_{i};}}\n')
    declarations.append('DRIVING_CONFIG\n}\n')
    declarations.append('pub fn driving_has_player(case:u16)->bool {\n' + (' || '.join(f'case=={i}' for i in player_cases) or 'false') + '\n}\n')
    declarations.append('pub fn driving_has_clock(case:u16)->bool {\n' + (' || '.join(f'case=={i}' for i in clock_cases) or 'false') + '\n}\n')
    declarations.append('pub fn driving_clock(case:u16)->AssetBytes {\n')
    for i in clock_cases:
        declarations.append(f'if case=={i} {{return DRIVING_CLOCK_{i};}}\n')
    declarations.append('DRIVING_CONFIG\n}\n')
    declarations.append('pub fn driving_player(case:u16)->AssetBytes {\n')
    for i in player_cases:
        declarations.append(f'if case=={i} {{return DRIVING_PLAYER_{i};}}\n')
    declarations.append('DRIVING_CONFIG\n}\n')
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
    scenery_words = (out / 'scenery.bin').stat().st_size // 2
    scenery_header = struct.unpack('<48H', (out / 'scenery.bin').read_bytes()[:96])
    assert scenery_header[13] == scenery_header[15]
    entity_capacity = 512
    object_capacity = scenery_header[2] + 215
    # Components have one resident owner; diagnostic identity columns are not
    # copied. Flags use one word per ID and ordered queues reserve future IDs.
    live_words = scenery_words - scenery_header[13] - scenery_header[15] - scenery_header[5]
    live_words += entity_capacity - scenery_header[11] * 2
    live_words += 3 * (entity_capacity - scenery_header[13]) * 2
    live_words += 2 * (object_capacity - scenery_header[2]) * 2
    lifetime = struct.unpack('<' + 'H' * ((out / 'lifetime.bin').stat().st_size // 2),
                             (out / 'lifetime.bin').read_bytes())
    source_cursor = 2
    list_cursor = 0
    queue_cursor = 15008
    for index, name in enumerate(['FREE', 'LAP', 'SIGNS', 'ENTER', 'LEAVE']):
        count = lifetime[source_cursor]
        # Lap and sign membership is fixed at map construction; updates only
        # remove entries. Deferred visibility queues retain entity capacity.
        capacity = scenery_header[13]
        if index == 0:
            capacity = object_capacity
        elif index == 1 or index == 2:
            capacity = count
        assert count <= capacity
        offset = queue_cursor if index >= 3 else list_cursor
        declarations.append(f'pub const DRIVING_LIFE_{name}_OFFSET:u16={offset};\n')
        declarations.append(f'pub const DRIVING_LIFE_{name}_CAPACITY:u16={capacity};\n')
        source_cursor += 1 + count * 2
        if index >= 3:
            queue_cursor += capacity * 2
        else:
            list_cursor += capacity * 2
    assert queue_cursor < 16383
    declarations.append(f'pub const DRIVING_LIFE_QUEUE_END:u16={queue_cursor};\n')
    assert source_cursor == len(lifetime)
    assert live_words < 8192
    assert all(len(frame['scenery']) < 65536 for c in data['cases'] if c.get('scenery') for frame in c['frames'])
    declarations.append(f'pub const DRIVING_CASES:u16={len(data["cases"])};\n')
    declarations.append(f'pub const DRIVING_START_WORDS:u16={start_words};\n')
    declarations.append(f'pub const DRIVING_CATALOG_WORDS:u16={catalog_words};\n')
    declarations.append(f'pub const DRIVING_PARENT_WORDS:u16={len(parent_words)};\n')
    declarations.append(f'pub const DRIVING_ENTITY_CAPACITY:u16={entity_capacity};\n')
    declarations.append(f'pub const DRIVING_OBJECT_CAPACITY:u16={object_capacity};\n')
    for name, value in [('CONFIG_WORDS', len(config)), ('STATE_VALUES', state_values),
                        ('INITIAL_WORDS', initial_words), ('FRAME_WORDS', frame_words),
                        ('GUN_WORDS', data['gun_words']), ('MESH_WORDS', len(data['body_data'])),
                        ('BODY_VERTICES', data['body_vertices']), ('BODY_FACES', data['body_faces']),
                        ('DEFORM_WORDS', (out / 'deform-bound.bin').stat().st_size // 2), ('SURFACE_VALUES', surface_values), ('DEFORM_COUNT',deform_count),
                        ('SCENERY_WORDS', scenery_words), ('LIVE_WORDS', live_words), ('IDENTITY_WORDS', (out / 'identity.bin').stat().st_size // 2),
                        ('SYMBOL_WORDS', (out / 'symbols.bin').stat().st_size // 2), ('LIFETIME_WORDS', (out / 'lifetime.bin').stat().st_size // 2)]:
        declarations.append(f'pub const DRIVING_{name}:u16={value};\n')
    declarations.append(f'pub static DRIVING_LIVE:Buf<u16,{live_words}>=Buf::new([0;{live_words}]);\n')
    assert initial_words <= frame_words
    assert (out / 'lifetime.bin').stat().st_size // 2 <= frame_words
    storage = [('gun', 'i16', data['gun_words'] * 3),
               ('surface_views', 'u16', deform_count*40),
               ('identity', 'u16', 12 + entity_capacity * 4),
               ('records', 'u16', object_capacity * 8),
               ('lifetime_lists', 'u16', list_cursor), ('catalog', 'u16', catalog_words),
               ('debris_directory', 'u16', 60)]
    declarations.append('pub struct DrivingStorage {\n')
    for name, kind, count in storage:
        declarations.append(f'pub {name}:Buf<{kind},{count}>,\n')
    declarations.append('}\npub fn driving_storage()->DrivingStorage { DrivingStorage {\n')
    for name, _, count in storage:
        declarations.append(f'{name}:Buf::new([0;{count}]),\n')
    declarations.append('}}\n')
    source = ''.join(declarations)
    if resource_aliases:
        source = re.sub(r'\b(' + '|'.join(resource_aliases) + r')\b',
                        lambda match: resource_aliases[match[0]], source)
    (out / 'driving_assets.rs').write_text(source)
    sources = [owner / n for n in ['driving_oracle.rs', 'driving_ground.rs', 'driving_capture.rs', 'sampling_snapshot.rs', 'reference_contact.rs', 'paired_driving_oracle.rs', 'finish_scene_assets.rs', 'finish_transfer_assets.rs', 'reference_world.rs', 'reference_bridge.py', 'reference_tanks.rs', 'reference_car.rs', 'impact_snapshot.rs', 'vehicle_oracle.rs', 'drive_oracle.rs', 'terrain_assets.rs', 'terrain_map_pack.py', 'driving_assets.py','player_scene_assets.rs','projectile_snapshot.rs','reference_player.py','reference_player.rs','reference_scenery_player.rs','scene_boot_assets.py','scene_initial_assets.py','scene_initial_oracle.rs','scene_actor_initial_oracle.rs']]
    sources += [owner / n for n in ['scene_export.rs', 'deform_assets.rs', 'driving_step.rs', 'scenery_assets.rs',
        'scenery_frame.rs', 'identity_assets.rs', 'identity_capture.rs', 'reference_scene.rs',
        'parent_scene_assets.rs', 'prefix_scene_assets.rs', 'pickup_scene_assets.rs', 'pickup_snapshot.rs', 'driving_devices.rs', 'debris_scene_assets.rs', 'reference_symbols.rs', 'complete_scene_assets.rs', 'complete_driving_step.rs', 'vehicle_startup_assets.rs']]
    sources.append(inverse)
    sources += [Path(row['file']) for row in json.loads((catalog_reference / 'mesh-store-sources.json').read_text())['sources']]
    sources += [Path(row['file']) for row in json.loads((player_catalog_reference / 'mesh-store-sources.json').read_text())['sources']]
    sources += [root / 'crates/game/src' / n for n in ['vehicle_runtime/car_objects.rs', 'scene_collision.rs', 'deformable_hit.rs', 'deformation.rs', 'obstacle_collision.rs']]
    sources += [root / 'crates/game/src' / n for n in ['contact_pipeline.rs', 'sampling.rs', 'suspension.rs', 'pose.rs', 'body.rs', 'motion.rs', 'vehicle_model.rs', 'random.rs', 'ground_collision.rs', 'impact_particles.rs', 'effect_pool.rs', 'damage.rs']]
    (out / 'assets.json').write_text(json.dumps(dict(resources=resources, resource_aliases=resource_aliases, sector_relocations=[], steps=steps,
        terrain_map=terrain_map,
        initial_packing=dict(source_words=len(initial), stored_words=len(start_data),
                             row_words=start_words, unique_rows=len(start_images),
                             offsets='initial-offsets.bin'),
        cases=[dict(name=c['name'], cars=c['cars'], constructed=c.get('constructed',False), cold_scene=c.get('cold_scene',False), world_pass=c.get('world_pass', False), scheduled_tanks=2 if c.get('world_pass') else 1, updates=len(c['frames']), cells=c['cells'], fallback_contacts=c['fallback_contacts'], tank_contacts=c['tank_contacts'], deform_contacts=c.get('deform_contacts', 0),
                    deformed_vertices=c.get('deformed_vertices', 0), surface_sample_points=c.get('surface_sample_points', 0), deform_spark_events=c.get('deform_spark_events', 0), ordinary_contacts=c.get('ordinary_contacts', 0), scenery_ticks=c.get('scenery_ticks', 0), retirement=c.get('retirement', False), transient=c.get('transient', False), emitter_coverage=c.get('emitter_coverage'), train_coverage=c.get('train_coverage'), ambient_coverage=c.get('ambient_coverage'), flock_coverage=c.get('flock_coverage'), initial_rng_seed=c.get('seed_before_noise',0x1234abcd)) for c in data['cases']], updates=updates, scope=data['scope'], checked_i32_per_frame=state_values+1,
        checked_surface_i32=sum(len(c['frames']) * surface_values for c in data['cases'] if c.get('scene')),
        resource_version_words=sum(len(c['frames']) * deform_count*4 for c in data['cases'] if c.get('scene')),
        checked_pickup_words=sum(len(f.get('pickup',[])) for c in data['cases'] for f in c['frames']),
        checked_prefix_words=sum(len(f.get('prefix',[])) for c in data['cases'] for f in c['frames']),
        checked_debris_words=sum(len(f.get('debris',[])) for c in data['cases'] for f in c['frames']),
        checked_complete_words=sum(len(f.get('complete',[])) for c in data['cases'] for f in c['frames']), qualification_scope=qualification_scope, world_frames=sum(len(c['frames']) for c in data['cases'] if c.get('world_pass'))+(finish_steps+1 if qualification_scope in ('finish-run','finish-world','finish-ufo') else 0), finish_check_frames=finish_checks, checked_parent_words=sum(len(f.get('parents', [])) for c in data['cases'] for f in c['frames']),
        checked_player_words=sum(len(f.get('player',[])) for c in data['cases'] for f in c['frames']),
        checked_world_flight_words=sum(len(f.get('pickup_flights', []))*2 for c in data['cases'] for f in c['frames']),
        finish_transfers=int(qualification_scope in ('finish-transfer','finish-tick','finish-run','finish-world','finish-ufo')),
        checked_finish_transfer_words=(sum((out/f).stat().st_size//2 for f in ['finish-transfer.bin','finish-transfer-world.bin'] + ([f'finish-run{kind}-{i}.bin' for i in finish_checks for kind in ['', '-world']] if qualification_scope in ('finish-run','finish-world','finish-ufo') else [])) if qualification_scope in ('finish-transfer','finish-tick','finish-run','finish-world','finish-ufo') else 0),
        vehicle_updates=None if qualification_scope=='finish-ufo' else sum(len(c['frames'])*(2 if c.get('world_pass') else 1) for c in data['cases'])+(json.loads((out/'finish-run-updates.json').read_text()) if qualification_scope in ('finish-run','finish-world','finish-ufo') else int(qualification_scope=='finish-tick')),
        catalog_initial_slots=catalog['initial_slots'],catalog_payload_bytes=catalog['initial_payload_bytes'],
        scenery_frames=sum(len(c['frames']) for c in data['cases'] if c.get('scenery')),
        checked_scenery_words=checked_scenery_words,
        sources=[dict(file=str(p), sha256=hashlib.sha256(p.read_bytes()).hexdigest()) for p in sources]), indent=2) + '\n')


if __name__ == '__main__':
    main()
