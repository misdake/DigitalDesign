"""Compare persistent game owners from frame-end captures of two frozen images."""
import argparse
import array
import hashlib
import json
from pathlib import Path
import re
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'tools'))
from application_artifact import Artifact


def free_payload_ranges(words, start, count):
    """Keep allocator heads/links; only free payload bytes are unspecified."""
    if count < 3 or start < 0 or start + count > len(words):
        raise ValueError('mesh arena exceeds its owning bank')
    block = words[start]
    previous_end = start + 1
    result = []
    while block:
        if block < previous_end or block + 2 > start + count:
            raise ValueError('mesh free-list order or header is invalid')
        size = words[block]
        if size < 2 or block + size > start + count:
            raise ValueError('mesh free block exceeds its arena')
        result.append((block + 2, block + size))
        previous_end = block + size
        block = words[block + 1]
    return result


class Recording:
    def __init__(self, directory, report):
        self.artifact = Artifact(directory)
        self.report_path = Path(report).resolve()
        self.report = json.loads(self.report_path.read_text())
        system_prefix = (self.report['backend'] == 'system'
                         and self.report.get('requested_frame_prefix') == len(self.report['frames']))
        if (not self.report['passed'] or self.report['backend'] not in ('state', 'system')
                or not (self.report.get('execution_complete', True) or system_prefix)):
            raise ValueError('a completed state-capture report is required')
        if self.report['artifact_id'] != self.artifact.manifest['artifact_id']:
            raise ValueError('capture report belongs to another image')
        self.assets = json.loads(self.artifact.files['tankracer-driving-assets/assets.json'])
        if self.report['reference_checks'] != self.assets.get('validation', True):
            raise ValueError('capture validation mode differs from its image')
        if len(self.assets['cases']) != 1:
            raise ValueError('select one continuous level')
        self.types = {}
        self.run = None
        m = self.artifact.manifest
        segments = [(m['target']['code_segment'], m['debug'])]
        segments += [(s['cseg'], s['debug']) for s in m.get('code_segments', [])]
        for segment, filename in segments:
            debug = self.artifact.files[filename].decode()
            for match in re.finditer(r'^type (\S+) (\d+) \d+\n((?:  .*\n)*)', debug, re.M):
                fields = {}
                for line in match[3].splitlines():
                    name, offset, kind = line.strip().split(' ', 2)
                    fields[name] = (int(offset), kind)
                self.types[match[1]] = (int(match[2]), fields)
            match = re.search(r'^func (?:level_run::)?run (0x[\da-f]+)\.\.(0x[\da-f]+) level_run.rs frame \d+\n((?:  .*\n)*)', debug, re.M)
            if match:
                self.run = (segment, int(match[1], 16), int(match[2], 16), match[3])
        if self.run is None:
            raise ValueError('level-run frame debug information is missing')
        self.buffers = json.loads(self.artifact.files['application.buffers.json'])
        self.constants = self.artifact.files['tankracer-driving-assets/driving_assets.rs'].decode()

    def constant(self, name):
        match = re.search(r'pub const ' + name + r':u16=(\d+);', self.constants)
        if match is None:
            raise ValueError('missing layout constant: ' + name)
        return int(match[1])

    def load(self, index):
        self.index = index
        self.capture = self.report['frames'][index]['capture']
        c = self.capture
        if not re.fullmatch(r'frame-\d+\.bin', c['file']):
            raise ValueError('invalid capture file locator')
        data = (self.report_path.with_suffix('.state') / c['file']).read_bytes()
        if len(data) != c['bytes'] or hashlib.sha256(data).hexdigest() != c['sha256']:
            raise ValueError('capture bytes differ from their report')
        if len(data) != 8 * 1024 * 1024:
            raise ValueError('capture is not a complete 20K memory image')
        self.memory = array.array('H')
        self.memory.frombytes(data)
        if sys.byteorder != 'little':
            self.memory.byteswap()
        self.pages = c['data_segments']
        segment, first, end, locals_text = self.run
        if c['code_segment'] != segment or not first <= c['pc'] < end:
            raise ValueError('capture is outside the level-run frame')
        local = re.search(r'^  frame\+(\d+) runtime (\S+)', locals_text, re.M)
        if local is None:
            raise ValueError('persistent Runtime is unavailable in this build')
        self.runtime = (c['registers'][13] + int(local[1]), local[2])
        self.data = {}
        self.unexpanded = set()
        self.page_cache = {}
        self.free_words = {}
        return self

    def read(self, address, count=1):
        if address < 0 or count < 0 or address + count > 65536:
            raise ValueError('logical view exceeds the CPU address space')
        result = []
        for word in range(address, address + count):
            physical = self.pages[word >> 14] * 16384 + (word & 16383)
            result.append(self.memory[physical])
        return result

    def size(self, kind):
        if kind in self.types:
            return self.types[kind][0]
        if kind == 'fix32':
            return 2
        if kind.startswith('Buf<'):
            element, count = kind[4:-1].rsplit(', ', 1)
            return self.size(element) * int(count)
        if kind in ('u16', 'i16', 'bool'):
            return 1
        raise ValueError('unsupported owned type: ' + kind)

    def field(self, value, name):
        offset, kind = self.types[value[1]][1][name]
        return value[0] + offset, kind

    def scalar(self, value, name):
        return self.read(self.field(value, name)[0])[0]

    def view(self, value, name):
        address, kind = self.field(value, name)
        if not kind.startswith('Array<'):
            raise ValueError('field is not a view: ' + name)
        return self.read(address)[0], kind[6:-1]

    def page(self, number):
        if number in self.page_cache:
            return self.page_cache[number]
        for owner in self.buffers:
            first = owner['address'] // 32768
            if first <= number < first + owner['bytes'] // 32768:
                return [owner['name'], number - first]
        for name in ('DRIVING_MESH_CATALOG', 'DRIVING_PLAYER_MESH_CATALOG'):
            global_value = self.artifact.globals.get(name)
            if global_value is None:
                continue
            first, _, low, high = self.read(global_value['logical_word'], 4)
            if first & 255 or first >> 8 != number:
                continue
            length = low + (high << 16)
            raw = self.memory[first * 64:first * 64 + length // 2].tobytes()
            resource = next(r for r in self.assets['resources'] if r['symbol'] == name)
            if hashlib.sha256(raw).hexdigest() != resource['sha256']:
                raise ValueError('immutable catalog bytes changed: ' + name)
            value = [name, resource['sha256']]
            self.page_cache[number] = value
            return value
        return number

    def walk(self, value, path):
        address, kind = value
        if kind == 'AssetBytes' or kind.startswith(('Array<', 'Ptr<', '&')):
            self.unexpanded.add(path)
        elif kind in self.types:
            for name in self.types[kind][1]:
                if (path, name) in [('players', 'expected_offset'), ('players', 'clock_frame'),
                                    ('parents.map.terrain', 'switches')]:
                    continue
                self.walk(self.field(value, name), path + '.' + name)
        elif kind.startswith('Buf<'):
            element, count = kind[4:-1].rsplit(', ', 1)
            if element in ('u16', 'i16', 'bool', 'fix32'):
                self.data[path] = self.read(address, self.size(kind))
            else:
                for i in range(int(count)):
                    self.walk((address + i * self.size(element), element), f'{path}[{i}]')
        else:
            words = self.read(address, self.size(kind))
            if path.rsplit('.', 1)[-1].endswith('page') or path == 'parents.map.terrain.saved':
                words = [self.page(words[0])]
            self.data[path] = words

    def resident(self, value, name, count, path):
        address, _ = self.view(value, name)
        # Mapped views are compared through their physical owning bank below.
        if address < 32768:
            self.data[path + '.' + name] = self.read(address, count)
            self.unexpanded.discard(path + '.' + name)

    def ordered_list(self, value, path):
        count = self.scalar(value, 'count')
        if count > self.scalar(value, 'capacity'):
            raise ValueError('ordered list exceeds its storage: ' + path)
        self.resident(value, 'items', count * 2, path)

    def snapshot(self):
        self.walk(self.runtime, 'runtime')
        roots = {}
        for name in ['parents', 'players', 'pickups', 'vehicles', 'impacts', 'effects',
                     'surface', 'prefix_shared', 'components']:
            roots[name] = self.view(self.runtime, name)
            self.walk(roots[name], name)
        parent = roots['parents']
        cars = self.view(parent, 'cars')
        count = self.scalar(parent, 'car_count')
        if not 1 <= count <= 3:
            raise ValueError('unexpected persistent vehicle count')
        for i in range(count):
            car = (cars[0] + i * self.size(cars[1]), cars[1])
            path = f'car[{i}]'
            self.walk(car, path)
            for name, size in [('attitude', 58), ('suspension', 88), ('drive', 32),
                               ('motion', 32), ('boost', 8), ('tracks', 48),
                               ('gun', self.constant('DRIVING_GUN_WORDS')),
                               ('gun_binding', 10), ('blast', 14), ('marker', 4)]:
                self.resident(car, name, size, path)
        actors = self.view(parent, 'actors')
        for i in range(self.scalar(parent, 'actor_count')):
            self.walk((actors[0] + i * self.size(actors[1]), actors[1]), f'actor[{i}]')
        for name, size in [('fields', 90), ('auxiliary', 108), ('inventory', 8), ('present', 3)]:
            self.resident(roots['players'], name, size, 'players')
        for name, size in [('camera', 6), ('events', 3), ('binding', 26), ('peer_controls', 60)]:
            self.resident(self.runtime, name, size, 'runtime')
        self.resident(parent, 'slots', 3, 'parents')
        lifetime = self.field(parent, 'lifetime')
        identities = self.field(lifetime, 'identities')
        self.resident(identities, 'header', 12, 'parents.lifetime.identities')
        self.resident(identities, 'rows', self.scalar(identities, 'storage_capacity') *
                      self.scalar(identities, 'row_words'), 'parents.lifetime.identities')
        self.resident(lifetime, 'records', self.scalar(lifetime, 'object_count') * 8, 'parents.lifetime')
        for name in ['free_objects', 'lap_objects', 'signs', 'enter', 'leave']:
            self.ordered_list(self.field(lifetime, name), 'parents.lifetime.' + name)
        runtime = self.field(parent, 'runtime')
        ticks = self.field(runtime, 'ticks')
        self.resident(ticks, 'flags', self.scalar(ticks, 'flag_count') * self.scalar(ticks, 'flag_words'),
                      'parents.runtime.ticks')
        self.ordered_list(self.field(ticks, 'order'), 'parents.runtime.ticks.order')
        for name in ['ordinary', 'special']:
            self.ordered_list(self.field(runtime, name), 'parents.runtime.' + name)
        queues = self.field(runtime, 'queues')
        for name in ['notifications', 'destruction']:
            self.ordered_list(self.field(queues, name), 'parents.runtime.queues.' + name)
        self.resource_store(self.field(parent, 'resources'))
        models = self.field(parent, 'models')
        self.resident(models, 'records', self.scalar(models, 'count') * 6, 'parents.models')
        for name, kind in [('deform', 'deform_count'), ('prefixes', None)]:
            view = self.view(self.runtime, name)
            count = self.scalar(self.runtime, kind) if kind else 3
            for i in range(count):
                item = (view[0] + i * self.size(view[1]), view[1])
                self.walk(item, f'{name}[{i}]')
                if name == 'deform':
                    mesh = self.field(item, 'mesh')
                    self.resident(mesh, 'vertices', self.scalar(mesh, 'vertex_count') * 3, f'deform[{i}].mesh')
                    self.resident(mesh, 'flags', self.scalar(mesh, 'flag_count'), f'deform[{i}].mesh')
                    surface = self.field(item, 'sampling')
                    records = self.view(surface, 'records')
                    if records[0] < 32768:
                        for j in range(self.scalar(surface, 'record_count')):
                            self.walk((records[0] + j * self.size(records[1]), records[1]),
                                      f'deform[{i}].sampling.records[{j}]')
        debris = re.search(r'^  frame\+(\d+) debris (Buf<[^\n]+?>) scope ', self.run[3], re.M)
        if debris:
            self.walk((self.capture['registers'][13] + int(debris[1]), debris[2]), 'debris')
        local = re.search(r'^  frame\+(\d+) live_storage (Buf<u16, \d+>)', self.run[3], re.M)
        if local:
            address = self.capture['registers'][13] + int(local[1])
            self.data['live'] = self.read(address, self.size(local[2]))
        else:
            old = self.artifact.globals['DRIVING_LIVE']
            self.data['live'] = self.read(old['logical_word'], self.size(old['type']))
        self.banks(self.field(parent, 'resources'))
        self.check_debug_binding()
        return self.data

    def resource_store(self, store):
        count = self.scalar(store, 'count')
        if count > self.scalar(store, 'capacity'):
            raise ValueError('resource store exceeds its capacity')
        records = self.read(self.view(store, 'records')[0], count * 13)
        for i in range(count):
            flags = records[i * 13]
            if flags >> 2:
                records[i * 13] = [flags & 3, self.page((flags >> 2) - 1)]
        self.data['resources.records'] = records
        pages = self.read(self.view(store, 'pages')[0], count)
        self.data['resources.pages'] = [self.page(page) for page in pages]
        pointers = self.read(self.view(store, 'data')[0], count)
        for pointer, page in zip(pointers, pages):
            if pointer and page == 65535:
                raise ValueError('CPU-owned mesh payload needs an explicit snapshot span')
        self.data['resources.data'] = pointers
        self.data['resources.indices'] = self.read(self.view(store, 'indices')[0], count)
        sources = self.read(self.view(store, 'sources')[0], count)
        for i, address in enumerate(sources):
            if 0 < address < 32768:
                self.data[f'resources.source[{i}]'] = self.read(address, 6)
        self.resident(store, 'free', self.scalar(store, 'free_count'), 'resources')
        self.resident(store, 'names', self.scalar(store, 'name_count') * 2, 'resources')

    def banks(self, store):
        for owner in self.buffers:
            name = owner['name']
            if not name.startswith('DRIVING_'):
                continue
            base = owner['address'] // 2
            words = list(self.memory[base:base + owner['bytes'] // 2])
            if name in ('DRIVING_MOTION', 'DRIVING_ACTORS'):
                ranges = free_payload_ranges(words, self.scalar(store, 'arena_start'),
                                             self.scalar(store, 'arena_words'))
                self.free_words[name] = sum(end - start for start, end in ranges)
                for start, end in ranges:
                    words[start:end] = [None] * (end - start)
            sentinels = []
            if name == 'DRIVING_WORLD':
                sentinels = [(14400, 0xa55a), (16383, 0x5aa5),
                             (16384 + 8132, 0x393c), (32767, 0xc3c3)]
            if name == 'DRIVING_DYNAMIC_WORLD':
                sentinels = [(16340, 0x6a39), (16383, 0x93a6)]
            for offset, canary in sentinels:
                expected = canary if self.assets.get('validation', True) else 0
                if words[offset] != expected:
                    raise ValueError(f'bank boundary changed: {name}[{offset}]')
                words[offset] = None
            self.data['bank.' + name] = words

    def check_debug_binding(self):
        if not self.report['reference_checks']:
            return
        size = self.constant('DRIVING_FRAME_WORDS')
        raw = self.artifact.files['tankracer-driving-assets/frames-0.bin']
        words = array.array('H')
        words.frombytes(raw)
        if sys.byteorder != 'little':
            words.byteswap()
        start = self.index * size + 60
        expected = list(words[start:start + 58])
        actual = self.data['car[0].attitude']
        if not self.data['car[0].visible'][0]:
            expected = expected[:48] + expected[52:]
            actual = actual[:48] + actual[52:]
        if actual != expected:
            raise ValueError('debug-derived Car view differs from the Rust attitude oracle')


def compare(left, right):
    if left.assets['cases'][0]['name'] != right.assets['cases'][0]['name']:
        raise ValueError('recordings contain different trajectories')
    if not left.report['reference_checks'] or right.report['reference_checks']:
        raise ValueError('compare an original-reference run with a recorded-I/O run')
    results = []
    count = len(right.report['frames'])
    if count > len(left.report['frames']):
        raise ValueError('the reference run does not cover the runtime trajectory')
    if count == 0:
        raise ValueError('recordings have no shared frames')
    for i in range(count):
        inputs = []
        for recording in (left, right):
            size = recording.constant('DRIVING_FRAME_WORDS')
            raw = recording.artifact.files['tankracer-driving-assets/frames-0.bin']
            inputs.append(raw[i * size * 2:(i * size + 60) * 2])
        if len(inputs[0]) != 120 or inputs[0] != inputs[1]:
            raise ValueError(f'independent input words differ at frame {i}')
        a = left.load(i).snapshot()
        b = right.load(i).snapshot()
        if a.keys() != b.keys():
            raise ValueError('snapshot ownership schemas differ')
        differences = []
        checked = 0
        for name in a:
            if len(a[name]) != len(b[name]):
                raise ValueError('owner extent differs: ' + name)
            changed = []
            for index, (expected, actual) in enumerate(zip(a[name], b[name])):
                if expected is not None or actual is not None:
                    checked += 1
                if expected != actual:
                    changed.append(dict(word=index, expected=expected, actual=actual))
            if changed:
                differences.append(dict(owner=name, count=len(changed), first=changed[:8]))
        row = dict(frame=i, checked_word_observations=checked, differences=differences)
        results.append(row)
        print(json.dumps(row))
    return dict(format='tankracer.runtime-state-comparison.v1',
                comparison_tool_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                passed=all(not row['differences'] for row in results), frames=results,
                runtime_execution_complete=right.report.get('execution_complete', True),
                reference_artifact=left.report['artifact_id'], runtime_artifact=right.report['artifact_id'],
                excluded_diagnostics=['players.expected_offset', 'players.clock_frame',
                                      'parents.map.terrain.switches'],
                free_payload_words=right.free_words,
                unexpanded_views=sorted(right.unexpanded),
                scope='Persistent typed owners and all named physical game banks. '
                      'Page fields use named owners; allocator headers and allocated mesh bytes are compared. '
                      'Only free payload bytes and six validation sentinel words are masked. '
                      'Mapped views are covered through their banks; pointer addresses are not compared.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('reference_artifact', type=Path)
    parser.add_argument('reference_report', type=Path)
    parser.add_argument('runtime_artifact', type=Path)
    parser.add_argument('runtime_report', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        raise ValueError('choose a fresh comparison report')
    result = compare(Recording(args.reference_artifact, args.reference_report),
                     Recording(args.runtime_artifact, args.runtime_report))
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    if not result['passed']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
