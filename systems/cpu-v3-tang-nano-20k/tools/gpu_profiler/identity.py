"""Build declarations and external programming receipts for frozen replay inputs."""
import hashlib
import json
from pathlib import Path

from snapshot import SnapshotError


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def declared_id(value):
    body = {k: v for k, v in value.items() if k != "manifest_id"}
    # File locations are relocatable locators, not part of the program identity.
    body["files"] = [{k: v for k, v in entry.items() if k != "path"} for entry in body["files"]]
    return hashlib.sha256(canonical(body)).hexdigest()


def load_build(path, verify_files=True):
    path = Path(path).resolve()
    value = json.loads(path.read_text(encoding="utf-8"))
    if (value.get("schema") != "digitaldesign.gpu-capture-build" or value.get("version") != 1 or
            value.get("manifest_id") != declared_id(value) or
            value.get("clock_hz") != 54_000_000 or value.get("mc_clock_hz") != 108_000_000 or
            value.get("mc_profile") != "stream-cl3-register-return" or
            not value.get("files") or not value.get("resources")):
        raise SnapshotError("Unknown or inconsistent capture build declaration")
    seen = set()
    for entry in value["files"]:
        name = entry["name"]
        if name in seen or Path(name).is_absolute() or ".." in Path(name).parts:
            raise SnapshotError("Duplicate/unsafe source identity")
        seen.add(name)
        source = (path.parent / entry["path"]).resolve()
        if verify_files and (source.stat().st_size != entry["bytes"] or digest(source) != entry["sha256"]):
            raise SnapshotError("Build file identity mismatch: " + name)
    return value


def create_build(spec_path, out):
    """Explicit source/export/options list; no implicit worktree or boot-byte copy."""
    spec_path, out = Path(spec_path).resolve(), Path(out).resolve()
    spec = json.loads(spec_path.read_text(encoding="utf-8"))
    if out.exists():
        raise SnapshotError("Build declaration output already exists")
    files = []
    for entry in spec["files"]:
        source = (spec_path.parent / entry["path"]).resolve()
        files.append(dict(name=entry["name"], path=str(source),
                          bytes=source.stat().st_size, sha256=digest(source)))
    value = dict(schema="digitaldesign.gpu-capture-build", version=1,
                 clock_hz=54_000_000, mc_clock_hz=108_000_000,
                 mc_profile="stream-cl3-register-return", files=files,
                 resources=spec["resources"], options=spec["options"],
                 toolchain=spec["toolchain"], source_commit=spec["source_commit"])
    value["manifest_id"] = declared_id(value)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")
    load_build(out)
    return value


def verify_receipt(path, build):
    path = Path(path).resolve()
    value = json.loads(path.read_text(encoding="utf-8"))
    if (value.get("schema") != "digitaldesign.gpu-programming-receipt" or value.get("version") != 1 or
            value.get("manifest_id") != build["manifest_id"] or
            not value.get("board_id") or not value.get("programmed_at") or
            value.get("verified") is not True):
        raise SnapshotError("Missing or incompatible verified programming receipt")
    for kind in ["firmware", "bitstream"]:
        item = value[kind]
        source = (path.parent / item["path"]).resolve()
        if source.stat().st_size != item["bytes"] or digest(source) != item["sha256"]:
            raise SnapshotError("Programmed artifact identity mismatch: " + kind)
    return value


def preflight(capture, build_path, receipt_path):
    """Verify input extents up front; RTL additionally checks every physical read."""
    build = load_build(build_path)
    if capture.header["manifest_id"] != build["manifest_id"]:
        raise SnapshotError("Frozen header does not identify this build/catalog")
    receipt = verify_receipt(receipt_path, build)
    header = capture.header
    reads = []

    def require(name, base, length, expected=None):
        data = capture.read(base, length)
        if expected is not None and hashlib.sha256(data).hexdigest() != expected:
            raise SnapshotError("Immutable catalog mismatch: " + name)
        reads.append(dict(role=name, base_bytes=base, length_bytes=length,
                          sha256=hashlib.sha256(data).hexdigest()))

    require("commands+padded-fetch", header["cb_base"], header["cb_padded_bytes"])
    for prefix in ["object", "ui"]:
        if header[prefix + "_bytes"]:
            require(prefix, header[prefix + "_base"], header[prefix + "_bytes"])
    # Version1 deliberately captures even full-CLEAR targets. This avoids
    # presuming that every later framebuffer read has already been initialized.
    require("color-preimage", header["color_base"], header["plane_bytes"])
    require("depth-preimage", header["depth_base"], header["plane_bytes"])
    if header["front_base"]:
        require("old-front", header["front_base"], header["plane_bytes"])
    for resource in build["resources"]:
        require(resource["name"], resource["address"], resource["bytes"],
                resource.get("sha256") if resource.get("immutable", True) else None)
    return dict(schema="digitaldesign.gpu-replay-preflight", version=1,
                manifest_id=build["manifest_id"], input_sha256=capture.manifest()["input_sha256"],
                board_id=receipt["board_id"], programming_receipt_sha256=digest(receipt_path),
                build_manifest_sha256=digest(build_path), extents=reads,
                policy="cold GPU+production MC; no CPU/Display contention; initialized-read guard required",
                dependency_closure="static extents verified; dynamic RTL reads must still complete")
