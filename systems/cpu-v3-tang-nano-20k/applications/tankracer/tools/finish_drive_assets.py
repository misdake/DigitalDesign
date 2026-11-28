"""Prepare original first-map finish-driving branch fixtures."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
from reference_bridge import prepare
from reference_finish import append_finish_controls


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reference', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    root = args.reference.resolve().parent
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    owner = Path(__file__).resolve().parent
    game = prepare(root, out)
    append_finish_controls(root, out, game)
    project = out / 'tool'
    project.mkdir(exist_ok=True)
    manifest = ('[package]\nname="tankracer-finish-drive-oracle"\nversion="0.1.0"\nedition="2024"\n'
                '[workspace]\n[[bin]]\nname="finish-drive-oracle"\npath='
                + json.dumps((owner / 'finish_drive_oracle.rs').as_posix())
                + '\n[dependencies]\nserde_json="=1.0.109"\ntankracer-game={path='
                + json.dumps(game.as_posix()) + '}\ntankracer-assets={path='
                + json.dumps((root / 'crates/assets').as_posix()) + '}\n')
    (project / 'Cargo.toml').write_text(manifest)
    with (project / 'build.log').open('w') as log:
        subprocess.run(['cargo', 'build', '--release', '--offline', '--manifest-path',
                        str(project / 'Cargo.toml'), '--target-dir', str(project / 'target')],
                       check=True, timeout=300, stdout=log, stderr=subprocess.STDOUT)
    executable = project / 'target/release' / ('finish-drive-oracle.exe' if os.name == 'nt' else 'finish-drive-oracle')
    subprocess.run([str(executable), str(root), str(out)], check=True, timeout=60)
    subprocess.run([sys.executable, str(owner / 'export_trig_assets.py'), '--reference',
                    str(root), '--output', str(out)], check=True, timeout=60)


if __name__ == '__main__':
    main()
