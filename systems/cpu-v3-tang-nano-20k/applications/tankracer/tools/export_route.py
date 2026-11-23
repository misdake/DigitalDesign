"""Export the authored ENGLAND/ENGB WayPoint graph (no camera recording input)."""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import tempfile

FORMAT = "tankracer.route.v1"
START = "wptstart"


def read_json(path):
    data = Path(path).read_bytes()
    if b"\0" in data:
        raise ValueError(f"NUL in JSON: {path}")
    return json.loads(data), hashlib.sha256(data).hexdigest()


def write_json_atomic(path, value, indent=None):
    """Replace a complete JSON file and verify the bytes read back from disk."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    data = (json.dumps(value, indent=indent, allow_nan=False) + "\n").encode("utf-8")
    if b"\0" in data:
        raise ValueError("NUL in serialized JSON")
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as stream:
        temporary = Path(stream.name)
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    try:
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)
    actual = path.read_bytes()
    digest = hashlib.sha256(data).hexdigest()
    if len(actual) != len(data) or b"\0" in actual or hashlib.sha256(actual).hexdigest() != digest:
        raise OSError(f"JSON readback mismatch: {path}")
    return {"bytes": len(data), "sha256": digest}


def next_names(parameters):
    """ENGB uses flat parenthesized parameter groups, as WayPoint::configure."""
    groups = re.findall(r"\(\s*:next\b([^()]*)\)", parameters)
    if len(groups) != 1:
        raise ValueError("WayPoint needs exactly one :next group")
    names = groups[0].split()
    if not 1 <= len(names) <= 2:
        raise ValueError("WayPoint needs one or two next nodes")
    if len(set(names)) != len(names):
        raise ValueError("duplicate edge in :next")
    return names


def main_loop(points):
    by_name = {p["name"]: p for p in points}
    if START not in by_name:
        raise ValueError("missing start node wptstart")
    loop, seen = [], set()
    name = START
    while name not in seen:
        if name not in by_name:
            raise ValueError(f"missing node: {name}")
        seen.add(name)
        loop.append(name)
        name = by_name[name]["next"][0]
    if name != START or len(loop) < 3:
        raise ValueError("primary route does not close at wptstart")
    return loop


def validate_route(route):
    if (route.get("format") != FORMAT or route.get("map") != "ENGLAND"
            or route.get("scene") != "ENGB.DAT" or route.get("start") != START
            or type(route.get("branch")) is not int or route["branch"] != 0):
        raise ValueError("expected ENGLAND ENGB route with start wptstart and branch 0")
    points = route["points"]
    if not isinstance(points, list) or not points:
        raise ValueError("route needs WayPoints")
    names, ids = set(), set()
    count = route["source"]["object_count"]
    if type(count) is not int or count < len(points):
        raise ValueError("invalid source object count")
    for point in points:
        name, identity = point["name"], point["object_index"]
        if not isinstance(name, str) or not name or name in names:
            raise ValueError("duplicate or empty WayPoint name")
        if type(identity) is not int or not 0 <= identity < count or identity in ids:
            raise ValueError("invalid or duplicate source object index")
        names.add(name)
        ids.add(identity)
        raw = point["position_q10"]
        if (not isinstance(raw, list) or len(raw) != 3
                or any(type(v) is not int or not -(1 << 31) <= v < 1 << 31 for v in raw)):
            raise ValueError("position_q10 needs three signed 32-bit integers")
        world = point["position"]
        if (not isinstance(world, list) or len(world) != 3
                or any(type(v) not in (int, float) or not math.isfinite(v) for v in world)
                or world != [v / 1024 for v in raw]):
            raise ValueError("world position differs from Q10 / 1024")
        if point["next"] != next_names(point["parameters"]):
            raise ValueError("next nodes differ from authored parameters")
    for point in points:
        for name in point["next"]:
            if name not in names:
                raise ValueError(f"missing node: {name}")
    loop = main_loop(points)
    if route["main_loop"] != loop:
        raise ValueError("main_loop differs from start/branch 0 traversal")
    return route


def export_route(map_directory):
    directory = Path(map_directory)
    manifest, manifest_hash = read_json(directory / "map.json")
    if manifest.get("format") != "tankracer.map.v1" or manifest.get("name") != "ENGLAND":
        raise ValueError("expected ENGLAND map manifest")
    resources = [r for r in manifest["resources"] if r["id"] == "ENGB.DAT"]
    if len(resources) != 1 or resources[0]["file"] != "scenes/ENGB.json":
        raise ValueError("manifest must identify scenes/ENGB.json once")
    resource = resources[0]
    scene, scene_hash = read_json(directory / resource["file"])
    if scene.get("format") != "tankracer.scene.v1":
        raise ValueError("unsupported source scene format")
    objects = scene["data"]["objects"]
    points = []
    for identity, obj in enumerate(objects):
        if obj["class"] == "WayPoint":
            raw = obj["position"]
            points.append(dict(object_index=identity, name=obj["name"], position_q10=raw,
                               position=[v / 1024 for v in raw], next=next_names(obj["parameters"]),
                               angles=obj["angles"], shape=obj["shape"], parameters=obj["parameters"]))
    route = dict(format=FORMAT, map="ENGLAND", scene="ENGB.DAT", start=START, branch=0,
                 source=dict(manifest_sha256=manifest_hash, scene_file=resource["file"],
                             scene_sha256=scene_hash, archive=manifest["source"],
                             resource_sha256=resource["source_sha256"],
                             resource_bytes=resource["source_bytes"], object_count=len(objects)),
                 points=points, main_loop=main_loop(points))
    return validate_route(route)


def pack_route(route, origin, scale):
    """Sidecar for offline ordering; it does not change draw or meshlet order."""
    validate_route(route)
    return dict(format="tankracer.route-render.v1", route=route, origin=list(origin), scale=scale,
                render_positions=[[ (v - o) * scale for v, o in zip(p["position"], origin)]
                                  for p in route["points"]])


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--map-directory", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        route = export_route(args.map_directory)
        result = write_json_atomic(args.output, route)
        print(json.dumps(dict(points=len(route["points"]), main_loop=len(route["main_loop"]), **result)))
    except (ValueError, TypeError, KeyError, OSError) as error:
        parser.exit(1, f"route export: {error}\n")


if __name__ == "__main__":
    main()
