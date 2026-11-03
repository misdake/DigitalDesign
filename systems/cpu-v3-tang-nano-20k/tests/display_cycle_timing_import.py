"""Import qualified Display fill pin traces; never write the source evidence tree."""
import argparse
import hashlib
import json
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("--source", required=True, type=Path)
parser.add_argument("--out", required=True, type=Path)
args = parser.parse_args()
source = args.source.resolve()
out = args.out.resolve()
if source == out or source in out.parents or out in source.parents:
    raise SystemExit("source and replay output must be separate trees")
manifest = json.loads((source / "manifest-normal.json").read_text())
cases = manifest["results"]
if len(cases) != 8 or not all(case.get("pass") for case in cases):
    raise SystemExit("complete qualified eight-case fill evidence required")
seal_path = source / "qualification-source.json"
seal = json.loads(seal_path.read_text()) if seal_path.exists() else None
parent_binding_path = source / "binding.json"
parent_binding = json.loads(parent_binding_path.read_text()) if parent_binding_path.exists() else None
if parent_binding and parent_binding["cases"] != cases:
    raise SystemExit("sealed replay cases differ from original binding")
names = ["production.v", "line.v", "tb.v", "manifest-normal.json"]
for case in cases:
    names += [case["case"] + ".trace", case["case"] + ".stdout"]
payloads = {}
hashes = {}
for name in names:
    payload = (source / name).read_bytes()
    digest = hashlib.sha256(payload).hexdigest()
    if parent_binding and parent_binding["files"].get(name, {}).get("sha256") != digest:
        raise SystemExit("sealed replay binding mismatch: " + name)
    if seal and seal["artifacts_sha256"].get(name) != digest:
        raise SystemExit("qualified artifact hash mismatch: " + name)
    if name.endswith(".stdout") and b"PASS case=" not in payload:
        raise SystemExit("missing RTL completion: " + name)
    payloads[name] = payload
    hashes[name] = {"sha256": digest, "git_blob": hashlib.sha1(
        b"blob " + str(len(payload)).encode() + b"\0" + payload).hexdigest()}
out.mkdir(parents=True, exist_ok=True)
for name, payload in payloads.items():
    target = out / name
    if target.exists() and target.read_bytes() != payload:
        raise SystemExit("preserve differing replay input: " + str(target))
    target.write_bytes(payload)
binding = {"format": "fill-rle-101-124", "origin": str(source), "files": hashes,
           "cases": cases, "origin_qualification_sha256": (
               hashlib.sha256(seal_path.read_bytes()).hexdigest() if seal else (
                   parent_binding.get("origin_qualification_sha256") if parent_binding else None)),
           "parent_binding_sha256": (hashlib.sha256(parent_binding_path.read_bytes()).hexdigest()
                                     if parent_binding else None)}
(out / "binding.json").write_text(json.dumps(binding, indent=2))
print(json.dumps({"copied": len(names), "cases": len(cases)}))
