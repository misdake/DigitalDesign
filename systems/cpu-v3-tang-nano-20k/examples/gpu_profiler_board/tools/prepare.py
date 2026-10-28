"""Bind sources, exports and canonical assets before the sole firmware build."""
import argparse
import json
from pathlib import Path
import subprocess
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[5]
SYSTEM = ROOT / "systems/cpu-v3-tang-nano-20k"
sys.path.insert(0, str(SYSTEM / "tools/gpu_profiler"))
from identity import create_build, digest
from assets import pack
from layout import ROW
from replay import export_layout, validate_rtl_layout


def prepare(output, hardware, system_export, replay_export, automatic, primitive, full_scene=False,
            column_catalog=None):
    output, hardware = output.resolve(), hardware.resolve()
    output.mkdir(parents=True, exist_ok=False)
    compared = []
    for qualified in sorted((hardware / "src").rglob("*")):
        if not qualified.is_file():
            continue
        relative = qualified.relative_to(hardware / "src")
        current = system_export / "src" / relative
        if not current.is_file() or digest(current) != digest(qualified):
            raise ValueError("Current system export differs from qualified hardware: " + str(relative))
        compared.append(dict(path=relative.as_posix(), sha256=digest(current)))
    if len(compared) != 32:
        raise ValueError("Expected the complete 32-file board source/constraints set")
    rtl_sources=json.loads((replay_export/'rtl.json').read_text())
    layout=export_layout(replay_export, '\n'.join((replay_export/name).read_text() for name in rtl_sources))
    validate_rtl_layout(layout, '\n'.join(p.read_text() for p in (system_export/'src').rglob('*.v')))
    pack(output / "assets", automatic_test=automatic, full_scene=full_scene,
         layout=layout, column_catalog=column_catalog)
    asset_manifest = json.loads((output / "assets/assets.json").read_text())
    names = subprocess.check_output(["git", "ls-files", "compiler/rcc", "ip/cpu-v3", "ip/gpu-v2",
        "hardware/core", "hardware/common", "hardware/vendor/gowin", "circuit",
        "systems/cpu-v3-tang-nano-20k", "Cargo.toml", "Cargo.lock"], cwd=ROOT, text=True).splitlines()
    paths = {ROOT / name for name in names if Path(name).suffix in
             {".rs", ".v", ".vh", ".py", ".ps1", ".toml", ".conf", ".lock", ".txt"}}
    # New implementation files may still be untracked during qualification.
    for folder in [SYSTEM / "tools/gpu_profiler", SYSTEM / "examples/gpu_profiler_board"]:
        paths.update(p for p in folder.rglob("*") if p.is_file() and p.suffix in {".py", ".rs", ".v", ".vh"})
    files = [dict(name=p.relative_to(ROOT).as_posix(), path=str(p)) for p in sorted(paths)]
    files.append(dict(name="vendor-primitive/prim_sim.v", path=str(primitive.resolve())))
    for prefix, folder in [("board-export", system_export / "src"), ("replay-export", replay_export),
                           ("assets", output / "assets")]:
        files.extend(dict(name=prefix + "/" + p.relative_to(folder).as_posix(), path=str(p.resolve()))
                     for p in sorted(folder.rglob("*")) if p.is_file())
    resources = [dict(name=r["file"], address=r["address"], bytes=r["bytes"],
                      sha256=r["sha256"], immutable=True) for r in asset_manifest["resources"]]
    if layout != ROW:
        for entry, resource in zip(resources, asset_manifest['resources']):
            entry['kind']=resource['kind']
            if resource['kind']=='texture': entry['layout']=resource['layout']
    spec = dict(files=files, resources=resources, options=dict(automatic_test=automatic, full_scene=full_scene,
                scene=("canonical box+RGB+948-face rabbit" if full_scene else "canonical minimal renderer"),
                input_precision="unchanged"),
                toolchain=dict(rustc=subprocess.check_output(["rustc", "--version"], text=True).strip(),
                               python=sys.version.split()[0], firmware_route="OUT_DIR + cpu-v3-boot-assets"),
                source_commit=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip())
    if layout != ROW: spec['layout']=layout.description()
    spec_path = output / "build-spec.json"
    spec_path.write_text(json.dumps(spec, indent=2) + "\n", encoding="utf-8")
    declaration = create_build(spec_path, output / "build.json")
    comparison = dict(complete=True, files=compared, hardware=str(hardware),
                      bitstream_sha256=digest(hardware / "impl/pnr/cpu_v3_system.fs"),
                      manifest_id=declaration["manifest_id"])
    comparison['layout']=layout.description()
    (output / "hardware-identity.json").write_text(json.dumps(comparison, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(dict(manifest_id=declaration["manifest_id"], matching_hardware_files=len(compared),
                         source_files=len(files), manifest=str(output / "build.json"))))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--hardware", type=Path, required=True)
    parser.add_argument("--system-export", type=Path, required=True)
    parser.add_argument("--replay-export", type=Path, required=True)
    parser.add_argument("--automatic-test", action="store_true")
    parser.add_argument("--primitive", type=Path, required=True)
    parser.add_argument("--full-scene", action="store_true")
    parser.add_argument('--column-catalog', type=Path,
                        help='CPU-produced, explicitly versioned column assets; never repack row assets here')
    args = parser.parse_args()
    prepare(args.out, args.hardware, args.system_export.resolve(), args.replay_export.resolve(),
            args.automatic_test, args.primitive, args.full_scene, args.column_catalog)
