"""Append the original Player pre-Tank body as an audited reference entry."""
import hashlib
import json
from pathlib import Path


def append_player_probe(root: Path, output: Path, game: Path):
    source = root / 'crates/game/src/vehicle_runtime/execution.rs'
    raw = source.read_bytes()
    text = raw.decode()
    function = text.index('    pub(super) fn update_car(')
    start = text.index('        let player = self.vehicles.entries[id.0].as_mut().ok_or(-1)?;', function)
    end = text.index('        if !player.visible {', start)
    body = text[start:end]
    after = text.index('        let result = self.physics(id, &mut call.context, env);', end)
    tail_start = text.index('        let player = self.vehicles.entries[id.0].as_mut().ok_or(-1)?;', after)
    tail_end = text.index('        Ok(())', tail_start) + len('        Ok(())')
    tail = text[tail_start:tail_end]
    for anchor in ['super::car::gun_z(', 'gun.changed();', '        result?;', 'self.horn(', '*call.result = call.entity.0;']:
        if tail.count(anchor) != 1:
            raise ValueError(f'Player tail reference anchor changed: {anchor}')
    for anchor in ['player.input.step(', 'player.route.cross(', 'self.fire_vehicle(',
                   'player.brake.step(', 'player.route.progress(', 'self.shared.weather.apply(']:
        if body.count(anchor) != 1:
            raise ValueError(f'Player reference stage anchor changed: {anchor}')
    owner = Path(__file__).resolve().parent
    bridge = owner / 'reference_player.rs'
    if not bridge.is_file():
        raise FileNotFoundError(bridge)
    suffix = (
        '\nimpl<D: Devices> Execution<\'_, D> {\n'
        '    fn rcc_before_tank(&mut self, id: TankId, call: &mut factory::Invocation<\'_, Handle>, '
        'env: factory::Environment<\'_>) -> Result<(), i32> {\n'
        + body + '        Ok(())\n    }\n'
        '    fn rcc_after_physics(&mut self, id: TankId, call: &mut factory::Invocation<\'_, Handle>, '
        'gun_before: Option<[Option<i16>; 4]>, result: Result<(), i32>) -> Result<(), i32> {\n'
        + tail + '\n    }\n}\n'
        + '#[path=' + json.dumps(bridge.as_posix()) + '] pub mod rcc_player_probe;\n'
    )
    manifest_path = output / 'reference-overlay.json'
    manifest = json.loads(manifest_path.read_text())
    additions = {'vehicle_runtime/execution.rs': suffix,
                 'vehicle_runtime.rs': '\npub use execution::rcc_player_probe;\n'}
    additions['vehicle_model.rs'] = '\nimpl Mesh { pub fn rcc_set_revision(&mut self, value: u64) { self.revision = value; } }\n'
    scenery_bridge = owner / 'reference_scenery_player.rs'
    if not scenery_bridge.is_file():
        raise FileNotFoundError(scenery_bridge)
    additions['scenery.rs'] = '\n#[path=' + json.dumps(scenery_bridge.as_posix()) + '] pub mod rcc_player_resources;\n'
    for relative, extra in additions.items():
        original = root / 'crates/game/src' / relative
        record = next(r for r in manifest['sources'] if Path(r['file']) == original)
        target = game / 'src' / relative
        expected = original.read_bytes() + record['appended'].encode()
        if target.read_bytes() != expected:
            raise ValueError(f'Player overlay input changed: {relative}')
        record['appended'] += extra
        target.write_bytes(expected + extra.encode())
        if target.read_bytes() != original.read_bytes() + record['appended'].encode():
            raise ValueError(f'Player overlay readback failed: {relative}')
    manifest['player_bridge'] = dict(file=str(bridge), sha256=hashlib.sha256(bridge.read_bytes()).hexdigest())
    manifest['player_scene_bridge'] = dict(file=str(scenery_bridge), sha256=hashlib.sha256(scenery_bridge.read_bytes()).hexdigest())
    manifest['player_stage'] = dict(source=str(source), source_sha256=hashlib.sha256(raw).hexdigest(),
                                    body_sha256=hashlib.sha256(body.encode()).hexdigest(),
                                    scope='Original Player body after finish takeover and before Tank visibility/prepare.')
    manifest['player_tail'] = dict(source=str(source), body_sha256=hashlib.sha256(tail.encode()).hexdigest(),
                                   scope='Original post-physics gun revision, error propagation, horn and audio publication.')
    manifest_path.write_text(json.dumps(manifest, indent=2) + '\n')
