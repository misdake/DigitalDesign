"""Build declarations and external programming receipts for frozen replay inputs."""
import hashlib
import json
from pathlib import Path

from snapshot import SnapshotError
from layout import COMMAND_ABI, declared_layout, header_layout, capture_description, COLUMN_MAP


def build_layout(value):
    try:
        if value['version'] not in (2, 3) or value.get('command_abi') != COMMAND_ABI:
            raise ValueError('Explicit production column/RTT build ABI required')
        layout = declared_layout(value['layout'])
        expected = layout.recorded_description() if value['version'] == 3 else layout.description()
        if value['layout'] != expected:
            raise ValueError('Build version and address policy disagree')
        return layout
    except (ValueError, KeyError) as error:
        raise SnapshotError('Unsupported build layout: ' + str(error)) from error


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
    if (value.get("schema") != "digitaldesign.gpu-capture-build" or value.get("version") not in (2, 3) or
            value.get("manifest_id") != declared_id(value) or
            value.get("clock_hz") != 54_000_000 or value.get("mc_clock_hz") != 108_000_000 or
            value.get("mc_profile") != "stream-cl3-register-return" or
            not value.get("files") or not isinstance(value.get('resources'), list) or
            (value.get('version') == 2 and not value['resources'])):
        raise SnapshotError("Unknown or inconsistent capture build declaration")
    layout = build_layout(value)
    if value['version'] in (2, 3):
        lo, hi = (0, 0x800000) if value['version'] == 3 else COLUMN_MAP['assets']
        for resource in value['resources']:
            if (not lo <= resource['address'] < resource['address'] + resource['bytes'] <= hi or
                    resource['address'] % 128 or resource['bytes'] % 128):
                raise SnapshotError('Resource exceeds the declared asset allocation/alignment')
            if resource.get('kind') not in ('mesh', 'texture', 'atlas', 'other'):
                raise SnapshotError('Build v2 catalog must type every resource')
            if resource['kind'] == 'texture' and resource.get('layout') != layout.texture_layout:
                raise SnapshotError('Ordinary texture bytes do not declare the column mip packing')
            if resource['kind']=='texture' and layout.abi==2 and (resource['bytes']<512 or resource['address']%256):
                raise SnapshotError('Column texture violates the minimum mip/cache-line footprint')
            if resource['kind']=='atlas' and (resource['address']%65536 or
                    (value['version']==2 and resource['bytes']!=65536) or
                    (value['version']==3 and resource['bytes']<256)):
                raise SnapshotError('UI atlas requires aligned glyph storage; v2 retains 64KiB')
        spans=sorted((r['address'],r['address']+r['bytes']) for r in value['resources'])
        if any(a[1]>b[0] for a,b in zip(spans,spans[1:])):
            raise SnapshotError('Build v2 resource allocations overlap')
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
    layout = declared_layout(spec['layout'])
    recorded = spec['layout'] == layout.recorded_description()
    value = dict(schema="digitaldesign.gpu-capture-build", version=3 if recorded else 2,
                 clock_hz=54_000_000, mc_clock_hz=108_000_000,
                 mc_profile="stream-cl3-register-return", files=files,
                 resources=spec["resources"], options=spec["options"],
                 toolchain=spec["toolchain"], source_commit=spec["source_commit"])
    value['command_abi'] = spec.get('command_abi', COMMAND_ABI)
    value['layout'] = spec['layout']
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


def simulation_provenance(capture, simulation_source):
    source = json.loads(Path(simulation_source).read_text())
    if (source.get('schema') != 1 or source.get('source') != 'system-simulation' or
            source.get('boundary') != 'before-profiler-gate' or source.get('gpu_consumed') is not False or
            source.get('input_sha256') != capture.manifest()['input_sha256']):
        raise SnapshotError('System-simulation provenance does not identify these frozen inputs')
    for key in ('compiled_program_sha256', 'recipe_sha256'):
        value = source.get(key, '')
        if not isinstance(value, str) or len(value) != 64 or any(c not in '0123456789abcdef' for c in value):
            raise SnapshotError('Missing compiled-program identity: ' + key)
    provenance = dict(source_kind='system-simulation', simulation_source_sha256=digest(simulation_source),
                      compiled_program_sha256=source['compiled_program_sha256'])
    return provenance


def input_extents(capture, required_span, resources, *, asset_files=()):
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
    if header.get('has_target', True):
        require("color-preimage", header["color_base"], required_span)
        require("depth-preimage", header["depth_base"], required_span)
    if header.get('has_front', bool(header["front_base"])):
        require("old-front", header["front_base"], required_span)
    if header.get('capture_version') == 2:
        target_ranges = [r for r in capture.ranges if r.tag in (capture.abi['TAG_COLOR'], capture.abi['TAG_DEPTH'])
                         and r.flags & capture.abi['WRITE']]
        if bool(target_ranges) != header['has_target']:
            raise SnapshotError('Target presence disagrees with recorded target ranges')
        if header['has_target']:
            for role in ('COLOR','DEPTH'):
                if not any(r.tag == capture.abi['TAG_'+role] and r.address == header[role.lower()+'_base']
                           and r.length == required_span for r in target_ranges):
                    raise SnapshotError('Final target is missing its physical writable range')
        for region in capture.ranges:
            require(f'recorded-range-{region.index}', region.address, region.length)
            if region.tag in (capture.abi['TAG_COLOR'], capture.abi['TAG_DEPTH']):
                if region.flags & capture.abi['WRITE']:
                    if region.length != required_span or region.address % 65536:
                        raise SnapshotError('Recorded target plane has an invalid physical extent')
                elif region.flags != (capture.abi['READ'] | capture.abi['PREIMAGE']):
                    raise SnapshotError('Recorded allocation tail is not read-only preimage')
    for resource in resources:
        name,base,length=resource["name"],resource["address"],resource["bytes"]
        immutable=resource.get("immutable",True)
        expected=resource.get("sha256") if immutable else None
        if header.get('capture_version') != 2:
            require(name,base,length,expected)
            continue
        # Recorded captures carry only reachable packet extents. An unused
        # catalog allocation is not a missing GPU input. All recorded extents
        # above remain mandatory and the RTL initialized-read guard is unchanged.
        selected=[]
        for region in capture.ranges:
            lo,hi=max(base,region.address),min(base+length,region.address+region.length)
            if lo>=hi:
                continue
            if immutable and region.flags & capture.abi['WRITE']:
                raise SnapshotError('Write aliases immutable catalog allocation: '+name)
            if region.flags & capture.abi['READ']:
                selected.append((lo,hi))
        if not selected:
            continue
        reference=None
        if expected is not None:
            entry=next((f for f in asset_files if f['bytes']==length and f['sha256']==expected),None)
            if entry is None:
                # A full capture can still authenticate the allocation by hash.
                # Partial hashes cannot prove omitted bytes without bound input.
                require(name,base,length,expected)
                continue
            reference=Path(entry['path']).read_bytes()
            if len(reference)!=length or hashlib.sha256(reference).hexdigest()!=expected:
                raise SnapshotError('Catalog asset file identity mismatch: '+name)
        for lo,hi in sorted(set(selected)):
            role=name+'+0x'+format(lo-base,'x')
            require(role,lo,hi-lo)
            if reference is not None and capture.read(lo,hi-lo)!=reference[lo-base:hi-base]:
                raise SnapshotError('Immutable catalog mismatch: '+role)
    return reads


def preflight(capture, build_path, receipt_path=None, *, simulation_source=None):
    """Verify input extents up front; RTL additionally checks every physical read."""
    build = load_build(build_path)
    if capture.header["manifest_id"] != build["manifest_id"]:
        raise SnapshotError("Frozen header does not identify this build/catalog")
    if (receipt_path is None) == (simulation_source is None):
        raise SnapshotError("Choose exactly one board receipt or system-simulation source")
    if simulation_source is not None:
        provenance = simulation_provenance(capture, simulation_source)
    else:
        receipt = verify_receipt(receipt_path, build)
        provenance = dict(source_kind='board-uart', board_id=receipt['board_id'],
                          programming_receipt_sha256=digest(receipt_path))
    header = capture.header
    if header.get('command_abi') != build.get('command_abi'):
        raise SnapshotError('Frozen descriptor and build use different command ABIs')
    layout = header_layout(header)
    if layout != build_layout(build):
        raise SnapshotError('Frozen descriptor and source/firmware build use different layouts')
    if build['layout'] != capture_description(header):
        raise SnapshotError('Frozen descriptor and build use different address policies')
    guards = build.get('options', {}).get('capture_allocation_guards', False)
    if type(guards) is not bool:
        raise SnapshotError('capture_allocation_guards must be an explicit boolean')
    if header.get('capture_version') == 2 and guards:
        raise SnapshotError('Recorded allocations use explicit ranges, not a fixed allocation span')
    required_span = layout.allocation_bytes if guards else layout.plane_bytes
    asset_files=[{**entry,'path':str((Path(build_path).resolve().parent/entry['path']).resolve())}
                 for entry in build['files']]
    reads = input_extents(capture, required_span, build["resources"],asset_files=asset_files)
    return dict(schema="digitaldesign.gpu-replay-preflight", version=1,
                layout=capture_description(header),
                captured_plane_bytes=required_span if header.get('has_target', True) else 0, allocation_guards_required=guards,
                manifest_id=build["manifest_id"], input_sha256=capture.manifest()["input_sha256"],
                build_identity_verified=True, verified_board_identity=receipt_path is not None,
                **provenance,
                build_manifest_sha256=digest(build_path), extents=reads,
                policy="cold GPU+production MC; no CPU/Display contention; initialized-read guard required",
                dependency_closure="static extents verified; dynamic RTL reads must still complete")


def input_preflight(capture, *, simulation_source=None):
    """Replay complete frozen bytes without claiming firmware/bitstream qualification."""
    layout = header_layout(capture.header)
    provenance = (simulation_provenance(capture, simulation_source) if simulation_source is not None
                  else dict(source_kind="frozen-input"))
    return dict(schema="digitaldesign.gpu-replay-preflight", version=1,
                manifest_id=capture.header["manifest_id"], input_sha256=capture.manifest()["input_sha256"],
                verified_board_identity=False, build_identity_verified=False,
                layout=capture_description(capture.header),
                captured_plane_bytes=layout.plane_bytes if capture.header.get('has_target', True) else 0,
                allocation_guards_required=False, extents=input_extents(capture, layout.plane_bytes, []),
                **provenance,
                policy="frozen bytes only; firmware/bitstream unverified; cold GPU+production MC; no CPU/Display contention",
                dependency_closure="explicit preimages verified; dynamic RTL reads must still complete")
