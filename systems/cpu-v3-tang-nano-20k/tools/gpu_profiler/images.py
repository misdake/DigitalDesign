"""Read-only command-completion images with historical cache ownership."""
from pathlib import Path
import base64
import hashlib
import json
import struct
import zlib
from layout import COLUMN, header_layout
from snapshot import MEMORY_BYTES, SnapshotError


class History:
    """Physical words plus versioned deltas of currently valid cache slots."""
    def __init__(self, initial, known, layout):
        self.initial, self.initial_known = initial, known
        self.physical, self.known = bytearray(initial), bytearray(known)
        self.layout, self.history, self.lines, self.resident = layout, {}, {}, {}
        self.sequence, self.epoch = 0, 0

    def version(self, cycle, owner, value):
        self.sequence += 1
        return cycle, self.sequence, owner, value

    def remember(self, address, version):
        values = self.history.setdefault(address, [])
        if version not in values:
            values.append(version)

    def state(self, slot, valid, tag, epoch, color, depth):
        old = self.lines.pop(slot, None)
        if old:
            for address, values in old['words'].items():
                if self.resident.get(address) is values:
                    del self.resident[address]
        if valid:
            if epoch != self.epoch:
                raise SnapshotError('Cache binding belongs to a stale target epoch')
            self.lines[slot] = dict(tag=tag, epoch=epoch, color=color, depth=depth, words={})

    def cache(self, cycle, slot, bank, address, value, owner):
        line = self.lines.get(slot)
        if line is None or line['epoch'] != self.epoch or owner < 1 or (address >> 5) & 7 != slot:
            raise SnapshotError('Unowned/invalid cache write in image journal')
        tag = line['tag']
        x = (tag & 31)*16 + (address & 3)*4 + (((bank >> 1)^((address >> 2)&1))&1)*2 + (bank & 1)
        y = (tag >> 6)*16 + ((tag >> 5)&1)*8 + ((address >> 3)&3)*2 + ((address >> 2)&1)
        base = line['depth'] if address & 512 else line['color']
        physical = base + self.layout.offset(x, y)
        values = line['words'].setdefault(physical, [])
        if physical in self.resident and self.resident[physical] is not values:
            raise SnapshotError('Two valid cache slots alias one image word')
        values.append(self.version(cycle, owner, value))
        self.resident[physical] = values

    def bus(self, cycle, address, value, direct_owner=None):
        if not 0 <= address < len(self.physical)-1 or address & 1:
            raise SnapshotError('Image write exceeds physical memory')
        if direct_owner is None:
            cached = self.resident.get(address)
            if cached:
                if cached[-1][3] != value:
                    raise SnapshotError('Writeback differs from the latest valid cache word')
                # Preserve overwritten older owners even if their value never reached DRAM.
                # A younger overlapping write must not erase an older command's image.
                for version in cached:
                    self.remember(address, version)
                owner = cached[-1][2]
            else:
                if not all(self.known[address:address+2]) or struct.unpack_from('<H', self.physical, address)[0] != value:
                    raise SnapshotError('Writeback has no known memory/cache provenance')
                owner = max(self.history.get(address, [(-1, 0, 0, value)]))[2]
        else:
            owner = direct_owner
        self.remember(address, self.version(cycle, owner, value))
        struct.pack_into('<H', self.physical, address, value)
        self.known[address:address+2] = b'\1\1'

    def plane(self, base, epoch, owner_limit):
        result = bytearray(self.layout.plane_bytes)
        for offset in range(0, len(result), 2):
            address = base + offset
            versions = [v for v in self.history.get(address, []) if v[2] <= owner_limit]
            if versions:
                value = max(versions)[3]
            elif all(self.initial_known[address:address+2]) and address+2 <= len(self.initial):
                value = struct.unpack_from('<H', self.initial, address)[0]
            else:
                raise SnapshotError('Image requires an uncaptured preimage word')
            struct.pack_into('<H', result, offset, value)
        overrides = 0
        for line in self.lines.values():
            if line['epoch'] != epoch:
                continue
            for address, versions in line['words'].items():
                if base <= address < base+len(result):
                    eligible = [v for v in versions if v[2] <= owner_limit]
                    if eligible:
                        value = max(eligible)[3]
                        at = address-base
                        overrides += struct.unpack_from('<H', result, at)[0] != value
                        struct.pack_into('<H', result, at, value)
        return bytes(result), overrides


def empty_draw_ids(profile):
    """Only absence of own published quads proves a DRAW has no output image."""
    meshes = {m['id']: m for m in profile['meshlets']}
    return {c['id'] for c in profile['commands'] if c['opcode'] == 3
            and meshes.get(c.get('meshlet'), {}).get('quads') == 0}


def build(capture, profile, event_path, output):
    """Project each command at its real completion, with younger owners excluded."""
    if not capture.header.get('has_target', True):
        return dict(available=False, reason='This submission has no render target')
    if profile['partial'] or not profile.get('memory', {}).get('available'):
        return dict(available=False, reason='Complete memory/ownership trace required')
    rows = {}
    for line in Path(event_path).read_text().splitlines():
        fields = line.split()
        if fields and fields[0].startswith('IMAGE_'):
            row = [int(v) for v in fields[1:]]
            row[0] -= profile['origin']
            rows.setdefault(fields[0], []).append(row)
    if not rows:
        return dict(available=False, reason='Capture has no image journal')
    layout = header_layout(capture.header)
    initial, known = bytearray(MEMORY_BYTES), bytearray(MEMORY_BYTES)
    for region in capture.ranges:
        initial[region.address:region.address+region.length] = capture.data[region.index]
        known[region.address:region.address+region.length] = b'\1' * region.length
    history = History(initial, known, layout)
    mesh_owner = {m['id']: m['command'] for m in profile['meshlets']}
    pc_owner = {(c['submission'], c['pc']): c['id'] for c in profile['commands']}
    def command_at(cycle, pc):
        for submission in profile['submissions']:
            if submission['start'] <= cycle < (submission['end'] or profile['end']):
                return pc_owner[(submission['id'], pc)]
        raise SnapshotError('Image write has no owning submission')
    targets = sorted(rows.get('IMAGE_TARGET', []))
    def target_at(cycle):
        selected = [r for r in targets if r[0] <= cycle and r[4]]
        if selected:
            _, epoch, color, depth, _ = selected[-1]
            return dict(epoch=epoch, color=color, depth=depth, binding='observed GPU target')
        return dict(epoch=None, color=capture.header['color_base'], depth=capture.header['depth_base'],
                    binding='capture output preimage; GPU target not yet bound')
    timeline = []
    for row in targets:
        timeline.append((row[0], 2, 'target', row[1:]))
    for row in rows.get('IMAGE_CACHE_STATE', []):
        timeline.append((row[0], 3, 'state', row[1:]))
    for row in rows.get('IMAGE_CACHE_WRITE', []):
        cycle, slot, bank, address, value, mid = row
        if mid not in mesh_owner:
            raise SnapshotError('Cache image write has no DRAW owner')
        timeline.append((cycle, 0, 'cache', (slot, bank, address, value, mesh_owner[mid])))
    owners = {r[0]: r[1:] for r in rows.get('IMAGE_WRITE_OWNER', [])}
    writes = [t for t in profile['memory']['transactions'] if t['port'] == 2]
    index, beat = 0, 0
    for row in sorted(rows.get('IMAGE_WRITE_DATA', [])):
        cycle, *values = row
        if index >= len(writes) or len(values) != 4:
            raise SnapshotError('Image payload has no write transaction')
        tx = writes[index]
        if not tx['start'] <= cycle-1 < tx['end'] or tx['start'] not in owners:
            raise SnapshotError('Image payload/transaction interval mismatch')
        pc, mode = owners[tx['start']]
        if mode not in range(4):
            raise SnapshotError('Unknown image write path')
        owner = command_at(tx['start'], pc) if mode else None
        address = tx['address_halfwords']*2 + beat*8
        for i, value in enumerate(values):
            timeline.append((cycle, 1, 'bus', (address+i*2, value, owner)))
        beat += 1
        if beat == tx['expected_beats']:
            index, beat = index+1, 0
    if index != len(writes) or beat:
        raise SnapshotError('Incomplete write payload in image journal')
    commands = {c['id']: c for c in profile['commands']}
    empty = empty_draw_ids(profile)
    for c in profile['commands']:
        if c.get('complete_cycle') is None:
            raise SnapshotError('Image requires proven command completion')
        if c['id'] not in empty:
            timeline.append((c['complete_cycle'], 4, 'snapshot', c['id']))
    timeline.append((profile['end'], 5, 'final', 0))
    output = Path(output)
    output.mkdir(parents=True, exist_ok=False)
    rendered, images = {}, {}
    def render(cycle, command, target, name, limit):
        plane, overrides = history.plane(target['color'], target['epoch'], limit)
        sha = hashlib.sha256(plane).hexdigest()
        file = output / name
        if sha in rendered:
            file.write_bytes(rendered[sha])
        else:
            preview(plane, file, capture.header['target_format'] == 1, layout)
            rendered[sha] = file.read_bytes()
        png = file.read_bytes()
        return dict(command=command, cycle=cycle, target_base_bytes=target['color'],
                    depth_base_bytes=target['depth'], target_epoch=target['epoch'], binding=target['binding'],
                    format=capture.header['target_format'], cache_override_words=overrides,
                    plane_sha256=sha, png_sha256=hashlib.sha256(png).hexdigest(), file=name,
                    src='data:image/png;base64,'+base64.b64encode(png).decode('ascii'))
    final = None
    for cycle, _, kind, args in sorted(timeline, key=lambda item: (item[0], item[1])):
        if kind == 'target':
            epoch, _, _, _ = args
            history.epoch = epoch
        elif kind == 'state':
            history.state(*args)
        elif kind == 'cache':
            history.cache(cycle, *args)
        elif kind == 'bus':
            history.bus(cycle, *args)
        elif kind == 'snapshot':
            c = commands[args]
            target = target_at(cycle if c['opcode'] == 10 else c['execute'])
            images[str(args)] = render(cycle, args, target, f'command-{args:06}.png', args)
        else:
            target = target_at(cycle)
            if target['color'] != capture.header['color_base']:
                target = dict(epoch=None, color=capture.header['color_base'], depth=capture.header['depth_base'],
                              binding='capture-declared final output')
            final = render(cycle, None, target, 'final.png', max(commands))
    result = dict(available=True, complete=True, scope='Historical memory plus valid cache; younger command owners excluded',
                  final=final, commands=images)
    portable = dict(result, final={k:v for k,v in final.items() if k != 'src'},
                    commands={k:{a:b for a,b in v.items() if a != 'src'} for k,v in images.items()})
    (output / 'index.json').write_text(json.dumps(portable, indent=2)+'\n')
    return result


def validate(images, profile):
    if not isinstance(images, dict) or type(images.get('available')) is not bool:
        raise ValueError('Invalid image metadata')
    if not images['available']:
        return
    commands = {str(c['id']): c for c in profile['commands']}
    required = set(commands) - {str(i) for i in empty_draw_ids(profile)}
    if (images.get('complete') is not True or profile['partial']
            or not required <= set(images['commands']) <= set(commands)):
        raise ValueError('Incomplete command image set')
    def one(image, command):
        cycle = image['cycle']
        if type(cycle) is not int or not 0 <= cycle <= profile['end'] or image['command'] != command:
            raise ValueError('Invalid image completion identity')
        if command is not None and cycle != commands[str(command)].get('complete_cycle'):
            raise ValueError('Image predates real command completion')
        base = image['target_base_bytes']
        if type(base) is not int or base & 1 or not 0 <= base <= MEMORY_BYTES-COLUMN.plane_bytes or image['format'] not in (0, 1):
            raise ValueError('Invalid image target/format')
        src = image['src']
        prefix = 'data:image/png;base64,'
        if not isinstance(src, str) or not src.startswith(prefix) or len(src) > 1_000_000:
            raise ValueError('Image must be a bounded inline PNG')
        png = base64.b64decode(src[len(prefix):], validate=True)
        if (len(png) < 33 or png[:8] != b'\x89PNG\r\n\x1a\n' or
                struct.unpack_from('>II', png, 16) != (400, 240) or hashlib.sha256(png).hexdigest() != image['png_sha256']):
            raise ValueError('Image PNG identity/dimensions mismatch')
    one(images['final'], None)
    for key, image in images['commands'].items():
        one(image, int(key))


def preview(plane, output, linear, layout=COLUMN):
    """Native tiled RGB565 -> lossless PNG preview. Raw plane remains authoritative."""
    if len(plane) != layout.plane_bytes:
        raise SnapshotError("Unexpected output plane length")
    rows = []
    for y in range(240):
        row = bytearray([0])
        for x in range(400):
            at = layout.offset(x, y)
            word, = struct.unpack_from("<H", plane, at)
            channels = [(word >> 11) / 31, ((word >> 5) & 63) / 63, (word & 31) / 31]
            if linear:
                # Viewing aid only; exact output comparisons use the raw plane.
                channels = [12.92 * v if v <= 0.0031308 else 1.055 * v ** (1 / 2.4) - 0.055
                            for v in channels]
            row.extend(round(max(0, min(1, v)) * 255) for v in channels)
        rows.append(row)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xffffffff)

    Path(output).write_bytes(b"\x89PNG\r\n\x1a\n" +
        chunk(b"IHDR", struct.pack(">IIBBBBB", 400, 240, 8, 2, 0, 0, 0)) +
        chunk(b"IDAT", zlib.compress(b"".join(rows))) + chunk(b"IEND", b""))
