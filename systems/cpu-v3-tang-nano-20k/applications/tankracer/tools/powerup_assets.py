"""Export exact original pickup phase and inventory calls and immutable lookup tables."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reference', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    root = Path(args.reference.resolve().parent.as_posix().removeprefix('//?/'))
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    owner = Path(__file__).resolve().parent
    project = out / 'reference-tool'
    project.mkdir(exist_ok=True)
    manifest = ('[package]\nname="tankracer-powerup-assets"\nversion="0.1.0"\nedition="2024"\n'
                '[workspace]\n[[bin]]\nname="powerup-oracle"\npath='
                + json.dumps((owner / 'powerup_oracle.rs').as_posix())
                + '\n[dependencies]\nserde_json="=1.0.109"\ntankracer-game={path='
                + json.dumps((root / 'crates/game').as_posix()) + '}\ntankracer-assets={path='
                + json.dumps((root / 'crates/assets').as_posix()) + '}\n')
    (project / 'Cargo.toml').write_text(manifest)
    with (project / 'build.log').open('w') as log:
        subprocess.run(['cargo', 'build', '--release', '--offline', '--manifest-path',
                        str(project / 'Cargo.toml'), '--target-dir', str(project / 'target')],
                       check=True, timeout=300, stdout=log, stderr=subprocess.STDOUT)
    executable = project / 'target/release' / ('powerup-oracle.exe' if os.name == 'nt' else 'powerup-oracle')
    subprocess.run([str(executable), str(root), str(out)], check=True, timeout=120)
    subprocess.run([sys.executable,str(owner/'export_trig_assets.py'),'--reference',str(root),
        '--output',str(out)],check=True,timeout=60,stdout=subprocess.DEVNULL)
    sources = [owner / n for n in ['powerup_oracle.rs', 'reference_world.rs', 'terrain_assets.rs', 'powerup_assets.py', 'export_trig_assets.py']]
    sources += sorted((root / 'crates/game/src').rglob('*.rs'))
    sources += sorted((root / 'crates/assets/src').rglob('*.rs'))
    sources += [root / 'crates' / crate / 'Cargo.toml' for crate in ['game', 'assets']]
    sources += [root / 'crates/game/data' / n for n in ['sine.bin', 'angle.bin']]
    sources += [root / 'assets/game' / n / 'FASTFILE.DAT' for n in ['DATA', 'INGAME',
                'TANKS/BRONZE', 'TANKS/SILVER', 'TANKS/GOLD', 'LEVELS/ENGLAND']]
    outputs = ['powerup.json', 'powerup-cells.bin', 'sine.i16.bin', 'angle.i16.bin']
    (out / 'powerup-sources.json').write_text(json.dumps(dict(
        sources=[dict(file=str(p), sha256=hashlib.sha256(p.read_bytes()).hexdigest()) for p in sources],
        outputs={n: hashlib.sha256((out / n).read_bytes()).hexdigest() for n in outputs}), indent=2) + '\n')


if __name__ == '__main__':
    main()
