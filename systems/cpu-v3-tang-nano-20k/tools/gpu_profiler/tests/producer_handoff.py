"""Replay committed CPU fixture bytes; no CPU source integration or board claims."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import identity
import snapshot as s
from layout import header_layout
from replay import execute, prepare
from replay import physical
from capture import run as capture_run
from layout_replay import stage_column
from draw_oracle import evaluate as draw_reference


def draw_archive(folder, receipt_path, receipt, commit):
    """Read the CPU-owned sequence archive without rewriting its layout/receipt."""
    if receipt.get('version') != 1 or receipt.get('source_commit') != commit:
        raise ValueError('DRAW producer receipt does not identify the requested commit')
    root = receipt_path.parent.resolve(); folder = folder.resolve()
    try:
        parts = folder.relative_to(root).parts
    except ValueError as error:
        raise ValueError('DRAW fixture is outside the sealed archive') from error
    full = receipt.get('schema') == 'digitaldesign.cpu-capture-full-scene-delivery'
    if ((full and (len(parts) != 3 or parts[0] != 'full-scene')) or
            (not full and len(parts) != 2) or parts[-1] not in ('frame-0','frame-1','frame-2')):
        raise ValueError('Expected a sealed profile/frame-N producer occurrence')
    profile_relative = '/'.join(parts[:-1]); frame_relative = '/'.join(parts)
    seen = set()
    for item in receipt['evidence_files']:
        name = item['path']; path = (root/name).resolve()
        if Path(name).is_absolute() or '..' in Path(name).parts or not path.is_relative_to(root) or name in seen:
            raise ValueError('Unsafe or duplicate DRAW evidence path')
        seen.add(name)
        if path.stat().st_size != item['bytes'] or identity.digest(path) != item['sha256']:
            raise ValueError('DRAW producer artifact changed: '+name)
    profile = next(v for v in receipt['profiles'] if v['profile'] == parts[-2])
    frame = next(v for v in profile['frames'] if v['frame_index'] == int(parts[-1][-1]))
    required = [f'{profile_relative}/build.json', f'{profile_relative}/producer.v3bin',
                *(f'{frame_relative}/{name}' for name in ('header.bin','ranges.bin','producer.json','commands.bin','objects.bin'))]
    if full: required.append(f'{frame_relative}/ui.bin')
    if not set(required) <= seen:
        raise ValueError('DRAW receipt does not seal the required producer inputs')
    if identity.digest(root/profile_relative/'producer.v3bin') != profile['producer_sha256' if full else 'program_sha256']:
        raise ValueError('DRAW program digest mismatch')
    for name, key in [('header.bin', 'header_sha256'), ('ranges.bin', 'ranges_sha256'),
                      ('commands.bin', 'commands_sha256'), ('objects.bin', 'objects_sha256')]:
        if (not full or key in ('commands_sha256','objects_sha256')) and key not in frame:
            raise ValueError('Missing DRAW occurrence digest: '+name)
        if key in frame and identity.digest(folder/name) != frame[key]:
            raise ValueError('DRAW occurrence digest mismatch: '+name)
    if full and identity.digest(folder/'ui.bin') != frame['ui_sha256']:
        raise ValueError('Full-scene UI occurrence digest mismatch')
    return root/profile_relative, dict(manifest_id=profile['manifest_id'], layout_command=profile['layout_command'], frame=frame,
                               sealed_paths=seen, archive_root=root)


def read_producer(folder, receipt_path, commit):
    receipt = json.loads(receipt_path.read_text())
    sequence = receipt.get('schema') in ('digitaldesign.cpu-capture-draw-delivery',
                                       'digitaldesign.cpu-capture-full-scene-delivery')
    build_root = folder
    if sequence:
        build_root, entry = draw_archive(folder, receipt_path, receipt, commit)
    else:
        if receipt['commit'] != commit or not receipt['source_tree_clean']:
            raise ValueError('Producer receipt does not identify the committed frozen source')
        entry = next(v for v in receipt['fixtures'] if v['name'] == folder.name)
        for item in entry['files']:
            name = item['file']
            if Path(name).name != name:
                raise ValueError('Unsafe producer receipt filename')
            path = folder / name
            if path.stat().st_size != item['bytes'] or identity.digest(path) != item['sha256']:
                raise ValueError('Producer artifact changed: ' + name)
    build = identity.load_build(build_root / 'build.json')
    if sequence and build['source_commit'] != commit:
        raise ValueError('DRAW build does not identify the committed producer source')
    abi_entry = next(v for v in build['files'] if v['name'] == 'rcc/gpu_capture_abi.rs')
    abi = s.Abi(build_root / abi_entry['path'])
    header = s.bytes_to_words((folder / 'header.bin').read_bytes())
    words = s.bytes_to_words((folder / 'ranges.bin').read_bytes())
    if len(words) % 8:
        raise ValueError('Truncated producer range table')
    rows = [words[i:i+8] for i in range(0, len(words), 8)]
    if sequence and any(str((folder/f'range-{i}.bin').resolve().relative_to(entry['archive_root'])).replace('\\', '/')
                        not in entry['sealed_paths'] for i in range(len(rows))):
        raise ValueError('DRAW receipt does not seal every frozen range payload')
    blobs = [(folder / f'range-{i}.bin').read_bytes() for i in range(len(rows))]
    wire = s.encode_reference(header, rows, blobs, abi)
    captures, decoded = s.decode_stream(wire, abi)
    if len(captures) != 1 or decoded['failures']:
        raise ValueError('Producer bytes do not form one complete independent snapshot')
    capture = captures[0]
    h = capture.header
    layout = header_layout(h)
    if layout != identity.build_layout(build) or h['manifest_id'] != build['manifest_id'] or h['manifest_id'] != entry['manifest_id']:
        raise ValueError('Producer header/build/layout identity mismatch')
    if sequence:
        frame = entry['frame']
        if entry['layout_command'] != [layout.abi, 1]:
            raise ValueError('DRAW receipt layout/command profile disagrees with frozen inputs')
        if any(h[name] != frame[key] for name, key in [('token','token'), ('publication','publication'),
                                                      ('slot','slot'), ('cb_base','cb_address'),
                                                      ('cb_bytes','command_bytes'), ('object_bytes','object_bytes')] if key in frame):
            raise ValueError('DRAW occurrence header disagrees with the receipt')
        if capture.read(h['cb_base'], h['cb_bytes']) != (folder/'commands.bin').read_bytes() or capture.read(h['object_base'], h['object_bytes']) != (folder/'objects.bin').read_bytes():
            raise ValueError('DRAW frozen command/object views disagree')
        if receipt.get('schema') == 'digitaldesign.cpu-capture-full-scene-delivery' and capture.read(h['ui_base'],h['ui_bytes']) != (folder/'ui.bin').read_bytes():
            raise ValueError('Full-scene frozen UI views disagree')
    for role in ('color', 'depth', 'front'):
        if h[role+'_base']:
            capture.read(h[role+'_base'], layout.plane_bytes)
    for role in ('object', 'ui'):
        if h[role+'_bytes']:
            capture.read(h[role+'_base'], h[role+'_bytes'])
    for resource in build['resources']:
        raw = capture.read(resource['address'], resource['bytes'])
        if hashlib.sha256(raw).hexdigest() != resource['sha256']:
            raise ValueError('Frozen resource differs from committed catalog')
    producer = json.loads((folder/'producer.json').read_text())
    source_name = 'rcc/gpu_layout.rs' if layout.abi == 1 else 'rcc/gpu_layout_column16.rs'
    actual_layout = next(v for v in build['files'] if v['name'] == source_name)
    dependencies = producer['origins'] if sequence else producer['dependencies']
    if not any(path.replace('\\', '/').endswith('/'+source_name) for path in dependencies):
        raise ValueError('Producer did not compile the declared actual layout dependency')
    origin = source_name
    if layout.abi == 2:
        origins_path = build_root/'profile-rcc/source-origins.json' if sequence else folder/'source-origins.json'
        origins = json.loads(origins_path.read_text())
        origin = origins['gpu_layout.rs']
        if not origin.replace('\\', '/').endswith('/rcc/gpu_layout_column16.rs'):
            raise ValueError('Column producer compiled a different actual layout origin')
        alias = next(v for v in build['files'] if v['name'] == 'producer-rcc/gpu_layout.rs')
        if alias['sha256'] != actual_layout['sha256']:
            raise ValueError('Generated layout alias differs from the selected canonical column source')
    return capture, wire, dict(producer_commit=commit, producer_receipt_sha256=identity.digest(receipt_path),
        build_sha256=identity.digest(build_root/'build.json'), manifest_id=h['manifest_id'],
        abi_sha256=abi.sha256, layout=layout.description(), command_abi=1, gpu_layout_origin=origin,
        selected_layout_source_sha256=actual_layout['sha256'],
        fixture_only=True, captured_plane_bytes=layout.plane_bytes,
        wire_source='Host encoding of exact CPU-produced header/ranges/payload; not actual UART',
        programming_receipt_verified=False,
        producer_boundary=producer['boundary'])


def ui_oracle(capture):
    """Independent command/data interpreter for this CLEAR + fixed-grid caller."""
    h = capture.header
    scratch = bytearray(4096)
    logical = {name: [0]*96000 for name in ('color', 'depth')}
    clear = {}; commands = []; glyphs = 0; writes = 0
    stream = capture.read(h['cb_base'], h['cb_bytes'])
    for pc, (word,) in enumerate(struct.iter_unpack('<I', stream)):
        opcode = word >> 28
        commands.append(dict(pc=pc*4, word=f'{word:08x}', opcode=opcode))
        if opcode in (0, 4, 7, 15):
            continue
        if opcode == 1:
            source = ((word >> 10) & 0x3ffff) * 32
            destination = ((word >> 3) & 127) * 32
            code = (word >> 1) & 3
            if code == 3:
                raise ValueError('Unsupported local DMA length')
            length = 32 << code
            scratch[destination:destination+length] = capture.read(source, length)
        elif opcode == 10:
            target, = struct.unpack_from('<I', scratch, (word & 127)*32)
            if (target & 0x3fff)*512 != h['color_base'] or ((target >> 14) & 0x3fff)*512 != h['depth_base']:
                raise ValueError('Actual target descriptor disagrees with frozen header')
        elif opcode == 8:
            role = 'depth' if word & 65536 else 'color'
            clear[role] = word & 65535
            logical[role] = [word & 65535]*96000
        elif opcode == 11:
            descriptor, = struct.unpack_from('<Q', scratch, (word & 127)*32)
            grid, atlas = (descriptor & 65535)*128, ((descriptor >> 16) & 127)*65536
            first, last = (((word >> 7) & 1023), ((word >> 17) & 1023)) if word & (1 << 27) else (0, 749)
            for cell in range(first, last+1):
                char = capture.read(grid+cell, 1)[0]
                if char == 255:
                    continue
                glyphs += 1
                glyph = capture.read(atlas+char*256, 256)
                for y in range(16):
                    for x in range(8):
                        # UI's documented offline four-bank packing, not a
                        # framebuffer macro addressing helper or RTL formula.
                        sx = (x & ~3) | ((x+2) & 3) if y & 1 else x
                        texel, = struct.unpack_from('<H', glyph, y*16+sx*2)
                        if texel & 1:
                            r, g, b = texel >> 11, (texel >> 6) & 31, (texel >> 1) & 31
                            value = (r << 11) | (((g << 1) | (g >> 4)) << 5) | b
                            px, py = (cell % 50)*8+x, (cell // 50)*16+y
                            logical['color'][py*400+px] = value
                            writes += 1
        else:
            raise ValueError('This oracle does not qualify DRAW/triangle or other caller work')
    if set(clear) != {'color', 'depth'}:
        raise ValueError('Expected both explicit plane CLEARs')
    return logical, clear, dict(commands=commands, draw_commands=0, ui_glyphs=glyphs, opaque_ui_pixels=writes)


def main():
    p = argparse.ArgumentParser(__doc__)
    p.add_argument('--fixture', type=Path, required=True)
    p.add_argument('--producer-receipt', type=Path, required=True)
    p.add_argument('--producer-commit', required=True)
    p.add_argument('--row-rtl', type=Path, required=True)
    p.add_argument('--frozen-column', type=Path)
    p.add_argument('--column-contract', type=Path)
    p.add_argument('--out', type=Path, required=True)
    p.add_argument('--oracle', choices=('ui', 'draw'), default='ui',
                   help='DRAW uses an independent rational reference; unsupported paths reject')
    p.add_argument('--depth-tolerance', type=int, choices=range(3), default=2)
    p.add_argument('--color-tolerance', type=int, choices=range(2), default=0,
                   help='Per RGB565 channel code, only for compared drawn pixels')
    p.add_argument('--include-edges', action='store_true',
                   help='Compare all logical pixels, including the mathematical one-pixel band')
    p.add_argument('--expect-fault', choices=('missing-read', 'forbidden-write'),
                   help='Poison one testbench mask only; never modifies producer input bytes')
    args = p.parse_args(); args.out.mkdir(parents=True, exist_ok=False)
    capture, wire, context = read_producer(args.fixture, args.producer_receipt, args.producer_commit)
    layout = header_layout(capture.header)
    capture.save(args.out/'snapshot'); (args.out/'host-wire.bin').write_bytes(wire)
    reference = None
    if args.oracle == 'draw':
        reference = draw_reference(capture)
        expected, clear, caller = reference.planes, reference.clear, reference.description()
        if sum(reference.stable[i] for i in reference.caller['drawn_pixels']) < 256:
            raise ValueError('DRAW fixture needs at least 256 stable covered pixels')
    else:
        expected, clear, caller = ui_oracle(capture)
    rtl = args.row_rtl
    if layout.abi == 2:
        if args.frozen_column is None or args.column_contract is None:
            raise ValueError('Column replay needs explicit frozen GPU and layout contract')
        rtl = args.out/'rtl'; stage_column(args.row_rtl, args.frozen_column, args.column_contract, rtl)
    manifest = prepare(capture, rtl, args.out/'replay', 180000, 180, context)
    if args.expect_fault:
        missing = args.expect_fault == 'missing-read'
        name = 'valid.hex' if missing else 'writable.hex'
        address = capture.header['cb_base' if missing else 'color_base']
        with (manifest.parent/name).open('a') as stream:
            stream.write(f'@{physical(address//4):x}\n'+('e' if missing else '0')+'\n')
        try:
            capture_run(manifest, manifest.parent/'capture')
        except RuntimeError:
            required = 'Uncaptured/uninitialized read' if missing else 'Write outside the captured writable extent'
            if required not in (manifest.parent/'capture/run.log').read_text(errors='replace'):
                raise
            (args.out/'result.json').write_text(json.dumps(dict(producer=context, caller=caller,
                expected_fault=args.expect_fault, rejected=True,
                scope='Simulation monitor negative on exact committed inputs; not hardware protection or board qualification'),indent=2)+'\n')
            print('PASS actual producer negative: '+args.expect_fault)
            return
        raise AssertionError('Invalid read/write mask was unexpectedly accepted')
    result = execute(capture, manifest)
    actual = {}
    for role in ('color', 'depth'):
        raw = (args.out/'replay/capture'/(role+'.bin')).read_bytes()
        actual[role] = layout.words(raw)
        if reference is None and actual[role] != expected[role]:
            raise AssertionError(role+' logical pixel oracle mismatch')
        for offset in range(0, layout.plane_bytes, 2):
            if layout.coordinates(offset) is None and struct.unpack_from('<H', raw, offset)[0] != clear[role]:
                raise AssertionError(role+' physical padding was not cleared')
    comparison = None
    if reference is not None:
        comparison = reference.compare(actual, depth_tolerance=args.depth_tolerance,
                                       color_tolerance=args.color_tolerance, include_edges=args.include_edges)
        (args.out/'oracle-comparison.json').write_text(json.dumps(comparison, indent=2)+'\n')
        if not comparison['pass']:
            raise AssertionError('Independent DRAW color/depth reference mismatch; see oracle-comparison.json')
    report = dict(scope='Committed CPU fixture bytes replayed offline; no combined CPU/GPU source build, UART, board/PVT or frame-performance qualification',
                  producer=context, caller=caller, replay=result,
                  logical_color_pixels_checked=comparison['compared_pixels'] if comparison else 96000,
                  logical_depth_pixels_checked=comparison['compared_pixels'] if comparison else 96000,
                  compared_guard_bytes=0,
                  padding_words_per_plane=(layout.plane_bytes-192000)//2,
                  oracle=args.oracle, oracle_comparison=comparison)
    (args.out/'result.json').write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps(dict(pass_=True, layout=layout.abi, commands=len(caller['commands']),
                         draw_commands=caller['draw_commands'], oracle=args.oracle,
                         ui_glyphs=caller.get('ui_glyphs', 0), opaque_ui_pixels=caller.get('opaque_ui_pixels', 0))))


if __name__ == '__main__':
    main()
