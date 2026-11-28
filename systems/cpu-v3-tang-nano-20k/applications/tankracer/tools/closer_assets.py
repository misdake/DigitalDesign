"""Export exact inverse-basis tables and original closing-barrier fixtures."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
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
    game = prepare(root, out, closer=True)
    project = out / 'reference-tool'
    project.mkdir(exist_ok=True)
    manifest = ('[package]\nname="tankracer-closer-assets"\nversion="0.1.0"\nedition="2024"\n'
                '[workspace]\n[[bin]]\nname="closer-oracle"\npath='
                + json.dumps((owner / 'closer_oracle.rs').as_posix())
                + '\n[dependencies]\nserde_json="=1.0.109"\ntankracer-game={path='
                + json.dumps(game.as_posix()) + '}\n')
    (project / 'Cargo.toml').write_text(manifest)
    with (project / 'build.log').open('w') as log:
        subprocess.run(['cargo', 'build', '--release', '--offline', '--manifest-path', str(project / 'Cargo.toml'),
                        '--target-dir', str(project / 'target')], check=True, timeout=300,
                       stdout=log, stderr=subprocess.STDOUT)
    exe = project / 'target/release' / ('closer-oracle.exe' if os.name == 'nt' else 'closer-oracle')
    subprocess.run([str(exe), str(out)], check=True, timeout=60)
    subprocess.run([sys.executable, str(owner / 'export_trig_assets.py'), '--reference', str(root),
                    '--output', str(out)], check=True, timeout=60, stdout=subprocess.DEVNULL)
    table = root / 'crates/game/data/cosine-angle.bin'
    data = table.read_bytes()
    if len(data) != 8192:
        raise ValueError('inverse cosine table must contain 4096 original i16 values')
    (out / 'cosine-angle.bin').write_bytes(data)
    sources = [table, owner / 'closer_oracle.rs', owner / 'reference_closer.rs', owner / 'reference_bridge.py',
               Path(__file__).resolve(), root / 'crates/game/src/closer.rs',
               root / 'crates/game/src/scene_objects/closing.rs', root / 'crates/game/src/math.rs']
    outputs = ['closer.json', 'cosine-angle.bin', 'sine.i16.bin', 'angle.i16.bin']
    (out / 'closer-sources.json').write_text(json.dumps(dict(
        sources=[dict(file=str(p), sha256=hashlib.sha256(p.read_bytes()).hexdigest()) for p in sources],
        outputs={name: hashlib.sha256((out / name).read_bytes()).hexdigest() for name in outputs}), indent=2) + '\n')


if __name__ == '__main__':
    main()
