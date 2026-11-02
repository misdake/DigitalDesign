"""Bounded cold replay of verified frozen bytes on production GPU+streaming MC."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[4] / "hardware/vendor/gowin/tests"))
from stream_contract import load_contract
from capture import run as capture_run
from identity import digest, input_preflight, load_build, preflight
from model import build_capture
from reporting import build_report, image_options
from images import preview  # Existing direct raw-plane export API.
from snapshot import Abi, DEFAULT_ABI, SnapshotError, load_snapshot
from layout import COLUMN, COMMAND_ABI, get_layout, header_layout, capture_description


def physical(logical_word):
    return (((logical_word >> 5) & 3) << 19) | (logical_word & 31) | ((logical_word >> 7) << 5)


def seed_files(capture, output):
    """Merge aliases by physical bytes, preserving validity of partial dwords."""
    words, masks, writable = {}, {}, {}
    for region in capture.ranges:
        blob = capture.data[region.index]
        for offset in range(0, len(blob), 2):
            address = region.address + offset
            index, shift = physical(address // 4), (address & 3) * 8
            value = blob[offset] | blob[offset + 1] << 8
            mask = 65535 << shift
            words[index] = (words.get(index, 0) & ~mask) | (value << shift)
            masks[index] = masks.get(index, 0) | (3 << (address & 3))
            if region.flags & capture.abi['WRITE']:
                writable[index] = writable.get(index, 0) | (3 << (address & 3))
    for name, values, width in [("memory.hex", words, 8), ("valid.hex", masks, 1),
                                 ("marked.hex", {i: 1 for i in masks}, 1), ('writable.hex', writable, 1)]:
        with (output / name).open("w", encoding="ascii", newline="\n") as stream:
            previous = -2
            for address in sorted(values):
                if address != previous + 1:
                    stream.write(f"@{address:x}\n")
                stream.write(f"{values[address]:0{width}x}\n")
                previous = address



def export_layout(rtl, combined):
    path = Path(rtl) / 'layout.json'
    if path.is_file():
        value = json.loads(path.read_text())
        layout = get_layout(value.get('layout_abi'))
        if value != dict(schema=1, layout_abi=layout.abi, gpu_profile=layout.profile):
            raise SnapshotError('Unknown RTL layout declaration')
    else:
        raise SnapshotError('Explicit RTL column layout declaration is required')
    validate_rtl_layout(layout, combined)
    return layout


def validate_rtl_layout(layout, combined):
    column = 'function[8:0]column_ui_tile;' in combined and 'clear_sector==1599' in combined
    if column != (layout.abi == 2):
        raise SnapshotError('RTL structure disagrees with its explicit framebuffer layout')
    if layout.abi == 2 and 'wire[11:0]macro_column=' not in combined:
        raise SnapshotError('Column framebuffer cannot use row-packed ordinary textures')


def validate_command_abi(capture):
    """A newer RTL export must not extend the frozen stream's declared ABI."""
    abi = capture.header.get('command_abi')
    if abi not in (1, 2, 3, 4):
        raise SnapshotError(f'Unsupported replay command ABI{abi}')
    last = 12 if abi in (3, 4) else 11
    words = capture.read(capture.header['cb_base'], capture.header['cb_bytes'])
    for pc, (word,) in enumerate(struct.iter_unpack('<I', words)):
        opcode = word >> 28
        if opcode > last and opcode != 15:
            raise SnapshotError(f'Command ABI{abi} does not support opcode{opcode} at byte PC0x{pc*4:x}')


def prepare(capture, rtl, output, max_cycles, timeout, preflight_result=None):
    """Prepare one isolated run; a test fixture may explicitly omit board identity."""
    validate_command_abi(capture)
    rtl, output = Path(rtl).resolve(), Path(output).resolve()
    if not 0 < max_cycles <= 20_000_000 or not 1 <= timeout <= 3600:
        raise SnapshotError("Replay needs explicit cycle and wall-clock bounds")
    output.mkdir(parents=True, exist_ok=False)
    sources = [(rtl / name).resolve() for name in json.loads((rtl / "rtl.json").read_text())]
    combined = "\n".join(p.read_text(encoding="utf-8") for p in sources)
    layout = header_layout(capture.header)
    if layout != export_layout(rtl, combined):
        raise SnapshotError('Frozen input and replay RTL have different storage layouts')
    has_target = capture.header.get('has_target', True)
    span=(preflight_result or {}).get('captured_plane_bytes', layout.plane_bytes if has_target else 0)
    if span not in ((layout.plane_bytes, layout.allocation_bytes) if has_target else (0,)):
        raise SnapshotError('Unexpected physical output/guard comparison extent')
    contract = load_contract(rtl / "peer")
    expected_peer = {(rtl / "peer" / name).resolve() for name in contract["files"] if name.endswith(".v")}
    if not expected_peer <= set(sources):
        raise SnapshotError("Stream manifest sources are missing from selected replay RTL")
    for required in ["module GowinSdramCombination",
                     "module gpu_v2_vliw_command", "reg read_event=0,read_visible=0,read_seen=0",
                     "Ordered two-descriptor endpoint"]:
        if required not in combined:
            raise SnapshotError("Export is not the selected production GPU/stream CL3 MC: " + required)
    primitive = Path(os.environ["GOWIN_HOME"]) / "IDE/simlib/gw2a/prim_sim.v"
    if not primitive.is_file():
        raise SnapshotError("Required vendor primitive model is missing")
    seed_files(capture, output)
    capture.save(output / 'snapshot')
    template = Path(__file__).with_name("replay_tb.v").read_text(encoding="utf-8")
    header = capture.header
    for name, value in [("COUNT", header["count_minus_one"]), ("BASE_LINE", header["cb_base"] // 128),
                        ("COLOR_BASE", header["color_base"]), ("DEPTH_BASE", header["depth_base"]),
                        ('PLANE_BYTES', layout.plane_bytes if has_target else 0), ('ALLOCATION_BYTES', span)]:
        template = template.replace("@" + name + "@", str(value))
    buffers = [dict(index=r.index, address=r.address, bytes=r.length, file=f'buffer-{r.index}.bin')
               for r in capture.ranges if r.tag == capture.abi.constants.get('GPU_CAPTURE_TAG_BUFFER', -1) and r.flags & capture.abi['WRITE']]
    writes = [dict(index=r.index, address=r.address, bytes=r.length, file=f'range-{r.index}.bin')
              for r in capture.ranges if capture.header.get('capture_version') == 2 and r.flags & capture.abi['WRITE']]
    template = template.replace('@BUFFER_DUMPS@', '\n'.join(
        f' dump_plane({r["address"]},{r["bytes"]},"{r["file"]}");' for r in buffers + writes))
    bench = output / "replay-input-tb.v"
    bench.write_text(template, encoding="utf-8")
    manifest = dict(version=1, title="Frozen pre-submit cold GPU+MC replay", clock_hz=54_000_000,
                    max_cycles=max_cycles, timeout_seconds=timeout, submissions=1, top="tb",
                    finish_on_complete=False, testbench=str(bench),
                    sources=[dict(path=str(p), sha256=digest(p)) for p in sources + [primitive]],
                    inputs={name: str(output / name) for name in ["memory.hex", "valid.hex", "marked.hex", 'writable.hex']},
                    input_sha256=capture.manifest()["input_sha256"], preflight=preflight_result,
                    report_snapshot='../snapshot',
                    layout=capture_description(header),
                    command_abi=COMMAND_ABI,
                    raw_buffers=buffers,
                    write_ranges=writes, has_target=has_target,
                    captured_plane_bytes=span,
                    functional_pin_model=dict(cas=3, tac_ns=6, toh_ns=2, fixed_return_delay_ns=4,
                                              sdram_phase_degrees=157.5,
                                              note="Functional replay assumptions; not measured PVT bounds"),
                    replay_scope="cold GPU+stream CL3 MC; no CPU/Display contention; functional pins",
                    submission_labels={"1": dict(frame_id=header["frame"], name="Frozen prepared CB")})
    path = output / "replay-manifest.json"
    path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    return path


def execute(capture, manifest_path, *, preview='display1', capture_format='unknown', require_images=False):
    """A stopped/failed backend must never leave a successful replay receipt."""
    validate_command_abi(capture)
    manifest_path = Path(manifest_path)
    declared = json.loads(manifest_path.read_text())
    if declared.get('command_abi') != capture.header['command_abi']:
        raise SnapshotError('Prepared replay/header command ABI mismatch')
    try:
        return _execute(capture, manifest_path, declared, preview=preview,
                        capture_format=capture_format, require_images=require_images)
    except Exception as error:
        (Path(manifest_path).parent / 'replay.json').write_text(json.dumps(dict(
            complete=False, input_sha256=capture.manifest()['input_sha256'], error=str(error)
        ), indent=2) + '\n')
        raise


def _execute(capture, manifest_path, declared, **image_policy):
    output = manifest_path.parent
    capture_run(manifest_path, output / "capture")
    return _collect(capture, manifest_path, declared, **image_policy)


def _collect(capture, manifest_path, declared, *, preview='display1', capture_format='unknown', require_images=False):
    """Check a completed native run; never synthesize a missing completion."""
    output = manifest_path.parent
    profile = build_capture(output / "capture")
    log = (output / "capture/run.log").read_text(encoding="utf-8", errors="replace")
    match = re.search(r"REPLAY_COMPLETE cycles=(\d+) initialized_reads=checked", log)
    if not match:
        raise SnapshotError("Replay did not reach its complete output/initialized-read boundary")
    color, depth = [output / "capture" / (name + ".bin") for name in ["color", "depth"]]
    layout = header_layout(capture.header)
    span=declared['captured_plane_bytes']
    if any(p.stat().st_size != capture.header["plane_bytes"] for p in [color, depth]):
        raise SnapshotError("Replay has incomplete output planes")
    has_target=capture.header.get('has_target',True)
    result = dict(complete=True, input_sha256=capture.manifest()["input_sha256"],
                  cold_submission_cycles=int(match.group(1)), color_sha256=digest(color) if has_target else None,
                  depth_sha256=digest(depth) if has_target else None, has_target=has_target,
                  profile=str(output / "profile/profiler.html"),
                  layout=capture_description(capture.header),
                  command_abi=COMMAND_ABI,
                  captured_plane_bytes=span,
                  scope="No CPU/Display contention or warm cache; not board timing/PVT qualification")
    result['raw_buffers'] = []
    for region in declared.get('raw_buffers', []):
        path = output/'capture'/region['file']
        if path.stat().st_size != region['bytes']:
            raise SnapshotError('Replay has an incomplete raw-copy destination')
        result['raw_buffers'].append(dict(**region, sha256=digest(path)))
    result['write_ranges'] = []
    for region in declared.get('write_ranges', []):
        path=output/'capture'/region['file']
        if path.stat().st_size != region['bytes']:
            raise SnapshotError('Incomplete recorded writable-range dump')
        result['write_ranges'].append(dict(**region,sha256=digest(path)))
    for role in (('color', 'depth') if has_target else ()):
        path = output / 'capture' / (role + '-allocation.bin')
        allocation = path.read_bytes()
        if len(allocation) != span:
            raise SnapshotError('Incomplete output allocation/guard dump')
        before = capture.read(capture.header[role + '_base'], span)
        if allocation[layout.plane_bytes:] != before[layout.plane_bytes:]:
            raise SnapshotError('Replay modified a plane allocation guard')
        if allocation[:layout.plane_bytes] != (output / 'capture' / (role + '.bin')).read_bytes():
            raise SnapshotError('Plane and allocation dumps disagree')
        result[role + '_allocation_sha256'] = digest(path)
    profile = build_report(output/'capture', output/'profile', snapshot=output/'snapshot', abi=capture.abi,
                           preview=preview, capture_format=capture_format, require_images=require_images,
                           image_output=output/'images', _profile=profile)
    if profile['images']['available']:
        (output/'color.png').write_bytes((output/'images/final.png').read_bytes())
    result['command_images'] = dict(available=profile['images']['available'],
                                  index=str(output / 'images/index.json') if profile['images']['available'] else None)
    (output / "replay.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    return result


def run(snapshot, rtl, out, max_cycles, timeout, *, abi=None, build=None, receipt=None, simulation_source=None,
        backend='rtl', simulator=None, maximum_work=64_000_000,
        preview='display1', capture_format='unknown', require_images=False):
    if backend not in ('rtl', 'sim'):
        raise SnapshotError('Unknown replay backend')
    if preview not in ('display1', 'raw', 'capture') or capture_format not in ('known', 'unknown'):
        raise SnapshotError('Unknown replay preview policy or capture-format declaration')
    out = Path(out)
    abi = abi or Abi(DEFAULT_ABI)
    capture = load_snapshot(snapshot, abi)
    if build is None:
        if receipt is not None:
            raise SnapshotError("A board receipt requires its build declaration")
        identity = input_preflight(capture, simulation_source=simulation_source)
    else:
        identity = preflight(capture, build, receipt, simulation_source=simulation_source)
    if backend == 'sim':
        if require_images:
            raise SnapshotError('Sim backend has final results only; --require-images requires RTL command history')
        if rtl is not None or max_cycles is not None:
            raise SnapshotError('Sim backend does not use RTL exports or cycle bounds')
        from simulator import run as simulate
        identity = dict(identity, policy='Native result simulation; captured reads/writes checked; no profiling',
                        dependency_closure='Static input checks and dynamic native byte access validation')
        return simulate(capture, out, identity, executable=simulator, maximum_work=maximum_work,
                        timeout=60 if timeout is None else timeout, preview_mode=preview,
                        capture_format_known=capture_format == 'known')
    if rtl is None or max_cycles is None or timeout is None or max_cycles < 1 or timeout <= 0:
        raise SnapshotError('RTL backend requires --rtl, positive --max-cycles and --timeout')
    rtl = Path(rtl)
    if build is not None:
        build = load_build(build)
        recorded = {entry["sha256"] for entry in build["files"]}
        for name in [*json.loads((rtl / "rtl.json").read_text()), "rtl.json", "peer/stream-contract.json"]:
            if digest(rtl / name) not in recorded:
                raise SnapshotError("Unbound RTL export; generate a matching build declaration before firmware")
        if (rtl / 'layout.json').is_file() and digest(rtl / 'layout.json') not in recorded:
            raise SnapshotError('Replay layout descriptor is not bound to the firmware build')
    path = prepare(capture, rtl, out, max_cycles, timeout, identity)
    return execute(capture, path, preview=preview, capture_format=capture_format, require_images=require_images)


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("snapshot", type=Path)
    parser.add_argument("--abi", type=Path, default=DEFAULT_ABI)
    parser.add_argument("--backend", choices=['rtl', 'sim'], default='rtl')
    parser.add_argument("--rtl", type=Path)
    parser.add_argument("--simulator", type=Path)
    parser.add_argument("--max-work", type=int, default=64_000_000)
    parser.add_argument("--build", type=Path)
    origin = parser.add_mutually_exclusive_group()
    origin.add_argument("--receipt", type=Path)
    origin.add_argument("--simulation-source", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--max-cycles", type=int)
    parser.add_argument("--timeout", type=int)
    image_options(parser, snapshot=False)
    args = parser.parse_args()
    result = run(args.snapshot, args.rtl, args.out, args.max_cycles, args.timeout,
                         abi=Abi(args.abi), build=args.build, receipt=args.receipt,
                         simulation_source=args.simulation_source, backend=args.backend,
                         simulator=args.simulator, maximum_work=args.max_work, preview=args.preview,
                         capture_format=args.capture_format, require_images=args.require_images)
    if args.backend == 'sim':
        print(json.dumps(dict(complete=result['complete'], backend='sim', input_sha256=result['input_sha256'],
                              result=str((args.out/'result.json').resolve()), viewer=str((args.out/'profiler.html').resolve())), indent=2))
    else:
        print(json.dumps(result, indent=2))


if __name__ == "__main__":
    try:
        main()
    except (SnapshotError, OSError, KeyError, ValueError, RuntimeError, subprocess.TimeoutExpired) as error:
        print("Replay refused: " + str(error), file=sys.stderr)
        raise SystemExit(1)
