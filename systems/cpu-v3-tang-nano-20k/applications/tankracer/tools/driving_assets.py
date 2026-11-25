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
    steps = int(os.environ.get('DRIVING_STEPS', '48'))
    if not 1 <= steps <= 96:
        raise ValueError('driving steps must be in 1..96')
    project = out / 'reference-tool'
    project.mkdir(exist_ok=True)
    game = prepare(root, out, driving=True)
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
    for case in data['cases']:
        assert len(case['initial']) == 1779
        supplement = [0] * 4 + case['initial'][:295] + [0] * 18 + case['initial'][1596:1779]
        assert len(supplement) == 500
        case['initial'] += supplement + [0, -1, 0, 0, -1, 0, 0]
        case['cars'], case['entities'], case['tank_contacts'] = 1, [1, 2], 0
        for frame in case['frames']:
            seed = frame['expected'].pop()
            frame['expected'] += supplement + [0, -1, 0, 0, -1, 0, 0, seed]
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
    data['cases'] += paired['cases']
    data['scope'] += '; ordered persistent physical peers through actual private car/ground entries'
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
        metadata.extend([0, 1 if case['cars'] == 2 else 65535] + [65535] * 7)
        metadata.extend(words([case['initial'][2285]]))
        metadata.extend([int(case.get('scene', False))] * 2)
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
        frames.extend(group)
    config = words(data['spring']) + words(data['drive']) + data['attitude'] + words(data['pose'])
    config += data['bumps'] + words(data['load']) + words(data['cases'][0]['environment'])
    config += words([v for p in data['auxiliary'] for v in p])
    config += words(data['body']) + [v & 65535 for v in data['gun_binding']]
    assert len(config) == 695
    assert len(initial) == len(data['cases']) * initial_words
    assert len(metadata) == len(data['cases']) * 20
    updates = sum(len(case['frames']) for case in data['cases'])
    assert len(frames) == updates * frame_words
    binary(out / 'initial.bin', initial)
    binary(out / 'config.bin', config)
    binary(out / 'body.bin', [v & 65535 for v in data['body_data']])
    binary(out / 'cases.bin', metadata)
    binary(out / 'binding.bin', words(data['collision_binding']))
    subprocess.run([sys.executable, str(owner / 'export_trig_assets.py'), '--reference', str(root), '--output', str(out)], check=True)
    (out / 'trig_lookup.rs').write_bytes((owner.parent / 'rcc/trig_asset_lookup.rs').read_bytes())
    declarations = ['use crate::dsl_rt::*; mod static_assets;use static_assets::AssetBytes;\n']
    resources = []
    resource_specs = [('CELLS', 'cells.bin', 32768), ('DIRECTORY', 'directory.bin', 128), ('INITIAL', 'initial.bin', 128),
                                ('CONFIG', 'config.bin', 128),
                                ('SINE', 'sine.i16.bin', 128), ('ANGLE', 'angle.i16.bin', 128),
                                ('MESH', 'body.bin', 128), ('CASE_META', 'cases.bin', 128),
                                ('BINDING', 'binding.bin', 128), ('SCENE', 'scene.bin', 128), ('DEFORM', 'deform.bin', 128)]
    resource_specs += [(f'FRAME_{i}', f'frames-{i}.bin', 128) for i in range(len(data['cases']))]
    surface_cases = [i for i,c in enumerate(data['cases']) if c.get('scene')]
    resource_specs += [(f'SURFACE_{i}', f'surfaces-{i}.bin', 128) for i in surface_cases]
    for symbol, name, align in resource_specs:
        path = out / name
        declarations.append(f'#[asset(align={align})] pub static DRIVING_{symbol}:AssetBytes='
                            f'AssetBytes::new(include_bytes!({json.dumps(path.as_posix())}));\n')
        resources.append(dict(file=name, placement='source', symbol='DRIVING_' + symbol,
                              bytes=path.stat().st_size, sha256=hashlib.sha256(path.read_bytes()).hexdigest()))
    declarations.append('pub fn driving_frames(case:u16)->AssetBytes {\n')
    for i in range(len(data['cases']) - 1):
        declarations.append(f'if case=={i} {{return DRIVING_FRAME_{i};}}\n')
    declarations.append(f'DRIVING_FRAME_{len(data["cases"])-1}\n}}\n')
    declarations.append('pub fn driving_surfaces(case:u16)->AssetBytes {\n')
    for i in surface_cases[:-1]:
        declarations.append(f'if case=={i} {{return DRIVING_SURFACE_{i};}}\n')
    declarations.append(f'DRIVING_SURFACE_{surface_cases[-1]}\n}}\n')
    declarations.append(f'pub const DRIVING_CASES:u16={len(data["cases"])};\n')
    for name, value in [('CONFIG_WORDS', len(config)), ('STATE_VALUES', state_values),
                        ('INITIAL_WORDS', initial_words), ('FRAME_WORDS', frame_words),
                        ('GUN_WORDS', data['gun_words']), ('MESH_WORDS', len(data['body_data'])),
                        ('BODY_VERTICES', data['body_vertices']), ('BODY_FACES', data['body_faces']),
                        ('DEFORM_WORDS', (out / 'deform.bin').stat().st_size // 2), ('SURFACE_VALUES', 749)]:
        declarations.append(f'pub const DRIVING_{name}:u16={value};\n')
    storage = [('config', 'u16', len(config)), ('initial', 'u16', initial_words),
               ('record', 'u16', frame_words), ('gun', 'i16', data['gun_words'] * 2),
               ('body', 'u16', len(data['body_data']) * 2),
               ('deform', 'u16', (out / 'deform.bin').stat().st_size // 2), ('surface_check', 'u16', 1498)]
    declarations.append('pub struct DrivingStorage {\n')
    for name, kind, count in storage:
        declarations.append(f'pub {name}:Buf<{kind},{count}>,\n')
    declarations.append('}\npub fn driving_storage()->DrivingStorage { DrivingStorage {\n')
    for name, _, count in storage:
        declarations.append(f'{name}:Buf::new([0;{count}]),\n')
    declarations.append('}}\n')
    (out / 'driving_assets.rs').write_text(''.join(declarations))
    sources = [owner / n for n in ['driving_oracle.rs', 'driving_ground.rs', 'driving_capture.rs', 'paired_driving_oracle.rs', 'reference_world.rs', 'reference_bridge.py', 'reference_tanks.rs', 'reference_car.rs', 'impact_snapshot.rs', 'vehicle_oracle.rs', 'drive_oracle.rs', 'terrain_assets.rs', 'driving_assets.py']]
    sources += [owner / n for n in ['scene_export.rs', 'deform_assets.rs']]
    sources += [root / 'crates/game/src' / n for n in ['vehicle_runtime/car_objects.rs', 'scene_collision.rs', 'deformable_hit.rs', 'deformation.rs', 'obstacle_collision.rs']]
    sources += [root / 'crates/game/src' / n for n in ['contact_pipeline.rs', 'sampling.rs', 'suspension.rs', 'pose.rs', 'body.rs', 'motion.rs', 'vehicle_model.rs', 'random.rs', 'ground_collision.rs', 'impact_particles.rs', 'effect_pool.rs', 'damage.rs']]
    (out / 'assets.json').write_text(json.dumps(dict(resources=resources, sector_relocations=[], steps=steps,
        cases=[dict(name=c['name'], cars=c['cars'], updates=len(c['frames']), cells=c['cells'], fallback_contacts=c['fallback_contacts'], tank_contacts=c['tank_contacts'], deform_contacts=c.get('deform_contacts', 0),
                    deformed_vertices=c.get('deformed_vertices', 0), surface_sample_points=c.get('surface_sample_points', 0), deform_spark_events=c.get('deform_spark_events', 0)) for c in data['cases']], updates=updates, scope=data['scope'], checked_i32_per_frame=state_values+1,
        checked_surface_i32=sum(len(c['frames']) * 749 for c in data['cases'] if c.get('scene')),
        sources=[dict(file=str(p), sha256=hashlib.sha256(p.read_bytes()).hexdigest()) for p in sources]), indent=2) + '\n')


if __name__ == '__main__':
    main()
