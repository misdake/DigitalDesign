"""Expose private reference entry points in an audited temporary source copy."""
import hashlib
import json
from pathlib import Path


def prepare(root: Path, output: Path, driving=False) -> Path:
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
        sources=records), indent=2) + '\n')
    return destination
