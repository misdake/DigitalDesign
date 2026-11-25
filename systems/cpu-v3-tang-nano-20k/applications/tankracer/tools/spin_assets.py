"""Export exact original weapon-spin calls."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess


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
    manifest = ('[package]\nname="tankracer-spin-assets"\nversion="0.1.0"\nedition="2024"\n'
                '[workspace]\n[[bin]]\nname="spin-oracle"\npath='
                + json.dumps((owner / 'spin_oracle.rs').as_posix())
                + '\n[dependencies]\nserde_json="=1.0.109"\ntankracer-game={path='
                + json.dumps((root / 'crates/game').as_posix()) + '}\ntankracer-assets={path='
                + json.dumps((root / 'crates/assets').as_posix()) + '}\n')
    (project / 'Cargo.toml').write_text(manifest)
    with (project / 'build.log').open('w') as log:
        subprocess.run(['cargo', 'build', '--release', '--offline', '--manifest-path',
                        str(project / 'Cargo.toml'), '--target-dir', str(project / 'target')],
                       check=True, timeout=300, stdout=log, stderr=subprocess.STDOUT)
    executable = project / 'target/release' / ('spin-oracle.exe' if os.name == 'nt' else 'spin-oracle')
    subprocess.run([str(executable), str(out)], check=True, timeout=120)
    sources = [owner / n for n in ['spin_oracle.rs', 'spin_assets.py']]
    sources += sorted((root / 'crates/game/src').rglob('*.rs'))
    sources += sorted((root / 'crates/assets/src').rglob('*.rs'))
    sources += [root / 'crates' / crate / 'Cargo.toml' for crate in ['game', 'assets']]
    outputs = ['spin.json']
    (out / 'spin-sources.json').write_text(json.dumps(dict(
        sources=[dict(file=str(p), sha256=hashlib.sha256(p.read_bytes()).hexdigest()) for p in sources],
        outputs={n: hashlib.sha256((out / n).read_bytes()).hexdigest() for n in outputs}), indent=2) + '\n')


if __name__ == '__main__':
    main()
