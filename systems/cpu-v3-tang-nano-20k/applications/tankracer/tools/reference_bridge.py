"""Expose private reference entry points in an audited temporary source copy."""
import hashlib
import json
from pathlib import Path


def prepare(root: Path, output: Path, driving=False, lifecycle=False, closer=False, meshes=False, symbols=False) -> Path:
    source = root / 'crates/game/src'
    owner = Path(__file__).resolve().parent
    bridge = owner / 'reference_tanks.rs'
    if not source.is_dir() or not bridge.is_file():
        raise FileNotFoundError('reference game source or tank bridge')
    destination = output / 'reference-game'
    suffixes = {
        'vehicle_runtime.rs': b'\npub use car_objects::rcc_probe;\n',
        'vehicle_runtime/car_objects.rs': (
            '\n#[path=' + json.dumps(bridge.as_posix()) + '] pub mod rcc_probe;\n').encode(),
    }
    car_bridge = owner / 'reference_car.rs'
    scene_bridge = owner / 'reference_scene.rs'
    if lifecycle:
        if not scene_bridge.is_file():
            raise FileNotFoundError(scene_bridge)
        suffixes['scene_objects.rs'] = (
            '\n#[path=' + json.dumps(scene_bridge.as_posix()) + '] pub mod rcc_lifecycle_probe;\n').encode()
        suffixes['scene_objects/projectiles.rs'] = b'''
impl View<'_> {
    pub(crate) fn rcc_reflect_projectile(&mut self, id: ObstacleId, vehicle: crate::object_query::TankId) -> Result<bool, i32> {
        self.reflect_projectile(id, vehicle)
    }
}
'''
    closer_bridge = owner / 'reference_closer.rs'
    meshes_bridge = owner / 'reference_meshes.rs'
    if meshes:
        if not meshes_bridge.is_file():
            raise FileNotFoundError(meshes_bridge)
        suffixes['scene_meshes.rs'] = (
            '\n#[path=' + json.dumps(meshes_bridge.as_posix()) + '] pub mod rcc_probe;\n').encode()
    if closer:
        if not closer_bridge.is_file():
            raise FileNotFoundError(closer_bridge)
        suffixes['scene_objects.rs'] = suffixes.get('scene_objects.rs', b'') + b'\npub use closing::rcc_probe as rcc_closer_probe;\n'
        suffixes['scene_objects/closing.rs'] = (
            '\n#[path=' + json.dumps(closer_bridge.as_posix()) + '] pub mod rcc_probe;\n').encode()
    if symbols:
        symbol_bridge = owner / 'reference_symbols.rs'
        if not symbol_bridge.is_file():
            raise FileNotFoundError(symbol_bridge)
        suffixes['symbols.rs'] = (
            '\n#[path=' + json.dumps(symbol_bridge.as_posix()) + '] pub mod rcc_probe;\n').encode()
    if driving:
        if not car_bridge.is_file():
            raise FileNotFoundError(car_bridge)
        suffixes['vehicle_runtime.rs'] += b'pub use car::rcc_car_probe;\n'
        suffixes['vehicle_runtime/car.rs'] = (
            '\n#[path=' + json.dumps(car_bridge.as_posix()) + '] pub mod rcc_car_probe;\n').encode()
    records = []
    paths = sorted(source.rglob('*')) + sorted((source.parent / 'data').rglob('*'))
    for path in paths:
        if not path.is_file():
            continue
        relative = path.relative_to(source.parent)
        suffix = suffixes.get(relative.as_posix().removeprefix('src/'), b'') if relative.parts[0] == 'src' else b''
        original = path.read_bytes()
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(original + suffix)
        if target.read_bytes() != original + suffix:
            raise ValueError(f'reference overlay readback failed: {relative}')
        records.append(dict(file=str(path), sha256=hashlib.sha256(original).hexdigest(),
                            appended=suffix.decode()))
    manifest = ('[package]\nname="tankracer-game"\nversion="0.1.0"\nedition="2024"\n'
                '[workspace]\n[features]\nextra-maps=[]\ntest-probes=[]\n'
                '[dependencies]\ntankracer-assets={path='
                + json.dumps((root / 'crates/assets').as_posix()) + '}\n')
    (destination / 'Cargo.toml').write_text(manifest)
    (output / 'reference-overlay.json').write_text(json.dumps(dict(
        scope='Original implementation bytes preserved; appended private-call bridge only.',
        bridge=dict(file=str(bridge), sha256=hashlib.sha256(bridge.read_bytes()).hexdigest()),
        car_bridge=dict(file=str(car_bridge), sha256=hashlib.sha256(car_bridge.read_bytes()).hexdigest()) if driving else None,
        scene_bridge=dict(file=str(scene_bridge), sha256=hashlib.sha256(scene_bridge.read_bytes()).hexdigest()) if lifecycle else None,
        closer_bridge=dict(file=str(closer_bridge), sha256=hashlib.sha256(closer_bridge.read_bytes()).hexdigest()) if closer else None,
        meshes_bridge=dict(file=str(meshes_bridge), sha256=hashlib.sha256(meshes_bridge.read_bytes()).hexdigest()) if meshes else None,
        symbol_bridge=dict(file=str(symbol_bridge), sha256=hashlib.sha256(symbol_bridge.read_bytes()).hexdigest()) if symbols else None,
        sources=records), indent=2) + '\n')
    return destination
