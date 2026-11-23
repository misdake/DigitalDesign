"""Rebuild route assets solely from verified snapshot inputs, including no-route mode."""
import argparse
import hashlib
import json
import os
import re
from pathlib import Path, PurePosixPath
import subprocess
import sys
import tempfile

from export_route import write_json_atomic


def identity(path):
    data = path.read_bytes()
    return dict(bytes=len(data), sha256=hashlib.sha256(data).hexdigest())


def check(snapshot, output):
    sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "tools"))
    from application_artifact import Artifact
    artifact = Artifact(snapshot)
    snapshot = artifact.root
    output.mkdir(parents=True, exist_ok=False)
    staged = output / "frozen-inputs"
    recovered = {}
    for entry in artifact.manifest["declared_inputs"]:
        origin = entry["origin"]
        if origin.get("kind") != "repository":
            continue
        relative = PurePosixPath(origin["path"])
        if relative.is_absolute() or ".." in relative.parts or ":" in str(relative):
            raise ValueError("invalid frozen input path")
        destination = staged.joinpath(*relative.parts)
        destination.parent.mkdir(parents=True, exist_ok=True)
        data = artifact.files[entry["file"]]
        with tempfile.NamedTemporaryFile(dir=destination.parent, delete=False) as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
            temporary = Path(stream.name)
        os.replace(temporary, destination)
        if destination.read_bytes() != data:
            raise OSError(f"frozen input readback: {destination}")
        recovered[hashlib.sha256(data).hexdigest()] = destination
    asset_folder = snapshot / "tankracer-static-assets"
    before = json.loads((asset_folder / "assets.json").read_bytes())
    route_input = recovered[before["route"]["input_sha256"]]
    environment = {k: v for k, v in os.environ.items() if not k.startswith("TANKRACER_")}
    common = staged / "systems/cpu-v3-tang-nano-20k/tools"
    producer = staged / "systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/static_assets.py"
    constants = artifact.files["tankracer-static-assets/tankracer_static.rs"].decode("utf-8")
    policy = int(re.search(r"pub const TANKRACER_POLICY\s*:\s*u16\s*=\s*(\d+)", constants)[1])
    reuse = int(re.search(r"pub const TANKRACER_REUSE\s*:\s*u16\s*=\s*(\d+)", constants)[1])
    environment.update(PYTHONPATH=str(common), PYTHONDONTWRITEBYTECODE="1",
                       TANKRACER_MAP_INPUT=str(recovered[before["map_sha256"]]),
                       TANKRACER_CAMERA_INPUT=str(recovered[before["camera_sha256"]]),
                       TANKRACER_POLICY={0: "full", 1: "cull", 2: "lod"}[policy],
                       TANKRACER_RECORDING="reuse" if reuse else "always",
                       TANKRACER_CANDIDATES=before.get("candidate_source", "clusters"))
    def produce(name, with_route):
        destination = output / name
        command = [sys.executable, "-B", str(producer), "--output", str(destination)]
        if with_route:
            command += ["--route", str(route_input)]
        subprocess.run(command, env=environment, cwd=staged, check=True)
        manifest = json.loads((destination / "assets.json").read_bytes())
        resources = {r["file"]: dict(bytes=r["bytes"], sha256=r["sha256"])
                     for r in manifest["resources"]}
        for name, expected in resources.items():
            if identity(destination / name) != expected:
                raise ValueError(f"rebuilt resource identity: {name}")
        return destination, manifest, resources
    rebuilt, after, resources = produce("rebuilt", True)
    candidate_flag = re.search(r"pub const TANKRACER_ROUTE_CANDIDATES\s*:\s*u16\s*=\s*(\d+)", constants)
    if candidate_flag:
        rebuilt_constants = (rebuilt / "tankracer_static.rs").read_text()
        rebuilt_flag = re.search(r"pub const TANKRACER_ROUTE_CANDIDATES\s*:\s*u16\s*=\s*(\d+)", rebuilt_constants)
        if rebuilt_flag is None or candidate_flag[1] != rebuilt_flag[1]:
            raise ValueError("snapshot candidate-mode constant rebuild differs")
    expected = {r["file"]: dict(bytes=r["bytes"], sha256=r["sha256"])
                for r in before["resources"]}
    if resources != expected or before["sector_relocations"] != after["sector_relocations"]:
        raise ValueError("snapshot resource rebuild differs")
    sidecars = {}
    for key in ("route", "route_ring"):
        name = before[key]["file"]
        locator = f"tankracer-static-assets/{name}"
        if locator not in artifact.files or (rebuilt / name).read_bytes() != artifact.files[locator]:
            raise ValueError(f"snapshot sidecar rebuild differs: {name}")
        sidecars[name] = identity(rebuilt / name)
    _, bare, bare_resources = produce("without-route", False)
    expected_bare = {k: v for k, v in expected.items() if k != "map_ring_group_data.bin"}
    if "route" in bare or "route_ring" in bare or bare_resources != expected_bare:
        raise ValueError("standalone no-route pack changed original resources")
    metadata = artifact.files["tankracer-static-assets/map_ring_group_data.bin"]
    resident_group_bytes = len(metadata) if any(metadata in artifact.files[r["file"]]
        for r in artifact.manifest["resources"] if r["kind"] == "copy") else 0
    result = dict(format="tankracer.route-snapshot-check.v1",
                  artifact_id=artifact.manifest["artifact_id"], frozen_inputs=len(recovered),
                  route_input=identity(route_input), resources=len(resources),
                  resource_bytes=sum(r["bytes"] for r in resources.values()),
                  all_resource_hashes_equal=True, sector_relocations_equal=True,
                  candidate_source=before.get("candidate_source", "clusters"),
                  candidate_mode_constant_equal=True,
                  sidecars=sidecars, no_route_resources=len(bare_resources), no_route_compatible=True,
                  producer_uses_only_frozen_inputs=True,
                  runtime_resident_ring_group_bytes=resident_group_bytes,
                  runtime_ring_table_globals=[name for name in artifact.globals if "MAP_RING_" in name])
    write_json_atomic(output / "result.json", result, indent=2)
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("snapshot", type=Path)
    parser.add_argument("output", type=Path)
    arguments = parser.parse_args()
    print(json.dumps(check(arguments.snapshot, arguments.output)))
