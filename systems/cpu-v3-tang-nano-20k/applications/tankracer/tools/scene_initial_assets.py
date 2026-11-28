"""Export original scene initialization helpers through an append-only probe."""
import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import subprocess
import struct
import sys
from reference_bridge import prepare


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reference', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    root = Path(args.reference.resolve().parent.as_posix().removeprefix('//?/'))
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    owner = Path(__file__).resolve().parent
    game = prepare(root, out)
    extra = '''
pub mod rcc_initial_probe {
    use super::*;
    pub fn visibility(parameters: &str, seed: &mut u32) -> [i32; 4] {
        let p = Parameters::new(parameters).unwrap();
        let v = super::visibility(p, seed).unwrap();
        [i32::from(v.distance), i32::from(v.add_lap), i32::from(v.remove_lap), i32::from(v.entered)]
    }
    pub fn glows(parameters: &str, pose: Pose, seed: &mut u32) -> Vec<[i32; 11]> {
        let p = Parameters::new(parameters).unwrap();
        let (records, _) = super::glows(p, pose, seed).unwrap();
        records.iter().map(|r| [r.position[0], r.position[1], r.position[2],
            r.world_position[0], r.world_position[1], r.world_position[2],
            r.timer, i32::from(r.state), i32::from(r.color[0]), i32::from(r.color[1]), i32::from(r.color[2])]).collect()
    }
    pub fn vertex_flags(count: usize, seed: &mut u32) -> Vec<u8> {
        super::vertex_flags(count, seed)
    }
}
'''
    manifest_path = out / 'reference-overlay.json'
    overlay = json.loads(manifest_path.read_text())
    actor_probe = """
pub mod rcc_actor_initial_probe {
    use super::*;
    fn source<'a>(level: &'a [u8], shared: &'a [u8]) -> Mounted<'a> {
        Mounted { shapes: Bundles { vehicle: vec![],
            shared: vec![shapes(shared).unwrap()], level: vec![shapes(level).unwrap()] },
            level: Archive::parse(level).unwrap(), shared: Archive::parse(shared).unwrap() }
    }
    pub fn flock(level: &[u8], shared: &[u8], terrain: &tankracer_assets::terrain::TerrainData,
        parameters: &str, pose: &mut crate::pose::Pose, seed: &mut u32) -> crate::actor_startup::flock::Flock {
        let source = source(level, shared);
        let mut models = ModelBank::default();
        let mut resources = crate::actor_startup::Resources { source: &source, models: &mut models,
            terrain, random: seed, camera: Default::default() };
        crate::actor_startup::flock::Flock::construct(
            tankracer_assets::parameters::Parameters::new(parameters).unwrap(), pose, &mut resources).unwrap()
    }
    pub fn emitter(level: &[u8], shared: &[u8], terrain: &tankracer_assets::terrain::TerrainData,
        seed: &mut u32) -> i32 {
        let source = source(level, shared);
        let mut models = ModelBank::default();
        let mut resources = crate::actor_startup::Resources { source: &source, models: &mut models,
            terrain, random: seed, camera: Default::default() };
        crate::actor_startup::emitter::Emitter::construct(
            tankracer_assets::parameters::Parameters::new(":bomber_type GUN ").unwrap(),
            crate::pose::Pose::default(), &mut resources).unwrap().timer
    }
    pub fn train(level: &[u8], shared: &[u8], terrain: &tankracer_assets::terrain::TerrainData,
        parameters: &str, pose: &mut crate::pose::Pose, seed: &mut u32) -> crate::actor_startup::train::Train {
        let source = source(level, shared); let mut models = ModelBank::default();
        let mut resources = crate::actor_startup::Resources { source: &source, models: &mut models,
            terrain, random: seed, camera: Default::default() };
        crate::actor_startup::train::Train::construct(
            tankracer_assets::parameters::Parameters::new(parameters).unwrap(), pose, &mut resources).unwrap()
    }
    pub fn pickup(level: &[u8], shared: &[u8], terrain: &tankracer_assets::terrain::TerrainData,
        record: tankracer_assets::level::Object<'_>, pose: &mut crate::pose::Pose, seed: &mut u32,
        setup: &mut crate::actor_startup::pickup::Setup) -> crate::actor_startup::pickup::Pickup {
        let source = source(level, shared); let mut models = ModelBank::default();
        let mut resources = crate::actor_startup::Resources { source: &source, models: &mut models,
            terrain, random: seed, camera: Default::default() };
        crate::actor_startup::pickup::Pickup::construct(record,
            tankracer_assets::parameters::Parameters::new(record.parameters).unwrap(), pose, &mut resources, setup).unwrap()
    }
}
"""
    camera_probe = """
pub fn rcc_starting_camera(pose: crate::pose::Pose, near: [i32; 3], seed: &mut u32) -> Option<Vec<i32>> {
    let config = tankracer_assets::object_catalog::Camera { near, ..Default::default() };
    let camera = StartingCamera::construct(crate::object_query::EntityId(1), pose, config, seed).ok()?;
    let mut out = Vec::new();
    out.extend(camera.pose.matrix.0); out.extend(camera.pose.position);
    out.extend(camera.angles.map(|v| v as i32)); out.extend(camera.displacement);
    out.extend(camera.target_angles); out.push(camera.variant as i32);
    for row in camera.resize { out.extend([i32::from(row.rate), i32::from(row.multiplier), i32::from(row.shift)]); }
    Some(out)
}
"""
    for relative, addition in [('scene_startup.rs', extra), ('scenery.rs', actor_probe), ('world_startup.rs', camera_probe)]:
        original = root / 'crates/game/src' / relative
        record = next(r for r in overlay['sources'] if Path(r['file']) == original)
        target = game / 'src' / relative
        expected = original.read_bytes() + record['appended'].encode()
        if target.read_bytes() != expected:
            raise ValueError('scene constructor overlay input changed: ' + relative)
        record['appended'] += addition
        target.write_bytes(expected + addition.encode())
        if target.read_bytes() != original.read_bytes() + record['appended'].encode():
            raise ValueError('scene constructor overlay readback failed: ' + relative)
    manifest_path.write_text(json.dumps(overlay, indent=2) + '\n')
    project = out / 'reference-tool'
    project.mkdir(exist_ok=True)
    manifest = ('[package]\nname="tankracer-scene-initial"\nversion="0.1.0"\nedition="2024"\n'
                '[workspace]\n[[bin]]\nname="scene-initial-oracle"\npath='
                + json.dumps((owner / 'scene_initial_oracle.rs').as_posix())
                + '\n[dependencies]\nserde_json="=1.0.109"\ntankracer-game={path='
                + json.dumps(game.as_posix()) + '}\ntankracer-assets={path='
                + json.dumps((root/'crates/assets').as_posix()) + '}\n')
    (project / 'Cargo.toml').write_text(manifest)
    with (project / 'build.log').open('w') as log:
        subprocess.run(['cargo', 'build', '--release', '--offline', '--manifest-path',
                        str(project / 'Cargo.toml'), '--target-dir', str(project / 'target')],
                       check=True, timeout=300, stdout=log, stderr=subprocess.STDOUT)
    executable = project / 'target/release' / ('scene-initial-oracle.exe' if os.name == 'nt' else 'scene-initial-oracle')
    subprocess.run([str(executable), str(root), str(out)], check=True, timeout=30)
    subprocess.run([sys.executable, str(owner/'export_trig_assets.py'), '--reference',
                    str(root), '--output', str(out)], check=True, timeout=30, stdout=subprocess.DEVNULL)
    inverse = root / 'crates/game/data/cosine-angle.bin'
    (out / 'cosine-angle.bin').write_bytes(inverse.read_bytes())
    pickup_source = (root/'crates/game/src/actor_startup/pickup.rs').read_text()
    table = re.search(r'let words = \[([\s\S]*?)\];', pickup_source)
    raw_words = [int(value,16) for value in re.findall(r'0x([0-9a-fA-F]+)', table[1])]
    if len(raw_words) != 8:
        raise ValueError('original pickup tilt table must contain eight words')
    raw_bytes = struct.pack('<8I',*raw_words)
    (out/'pickup-tilt.u16.bin').write_bytes(struct.pack('<32H',*raw_bytes))
    sources = [Path(row['file']) for row in overlay['sources']]
    sources += [Path(__file__).resolve(), owner / 'scene_initial_oracle.rs', owner/'scene_actor_initial_oracle.rs', owner / 'reference_bridge.py', owner/'export_trig_assets.py']
    sources += [root/'assets/game'/name/'FASTFILE.DAT' for name in ['DATA','INGAME','LEVELS/ENGLAND']]
    sources.append(inverse)
    sources += [Path(row['path']) for row in json.loads((out/'trig-assets.json').read_text())['source_files']]
    sources += [root/'crates/game/data'/name for name in ['sine.bin','angle.bin']]
    (out / 'scene-initial-sources.json').write_text(json.dumps(dict(
        sources=[dict(file=str(p), sha256=hashlib.sha256(p.read_bytes()).hexdigest()) for p in sources]), indent=2) + '\n')


if __name__ == '__main__':
    main()
