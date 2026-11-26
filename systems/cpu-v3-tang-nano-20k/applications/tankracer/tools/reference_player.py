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
        + body + '        Ok(())\n    }\n}\n'
        + '#[path=' + json.dumps(bridge.as_posix()) + '] pub mod rcc_player_probe;\n'
    )
    manifest_path = output / 'reference-overlay.json'
    manifest = json.loads(manifest_path.read_text())
    additions = {'vehicle_runtime/execution.rs': suffix,
                 'vehicle_runtime.rs': '\npub use execution::rcc_player_probe;\n'}
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
    manifest['player_stage'] = dict(source=str(source), source_sha256=hashlib.sha256(raw).hexdigest(),
                                    body_sha256=hashlib.sha256(body.encode()).hexdigest(),
                                    scope='Original Player body after finish takeover and before Tank visibility/prepare.')
    manifest_path.write_text(json.dumps(manifest, indent=2) + '\n')
