"""Inspect a verified same-build application artifact without compiling sources."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import struct


def require(condition, message):
    if not condition:
        raise ValueError('Application artifact: ' + message)


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode('utf-8')


def digest(data):
    return hashlib.sha256(data).hexdigest()


class Artifact:
    def __init__(self, directory):
        self.root = Path(directory).resolve()
        self.manifest = m = json.loads((self.root/'application.build.json').read_text(encoding='utf-8'))
        require(m.get('schema') == 'digitaldesign.application-build' and m.get('version') == 1, 'schema/version')
        require(m.get('artifact_id') == digest(canonical({k: v for k, v in m.items() if k != 'artifact_id'})), 'manifest identity')
        self.files = {}
        for name, identity in m['files'].items():
            require(not name.startswith('/') and '\\' not in name and ':' not in name
                    and all(p not in ('', '.', '..') for p in name.split('/')), 'relative file locator: '+name)
            path = (self.root/name).resolve()
            require(path.is_relative_to(self.root), 'file escapes artifact root: '+name)
            data = path.read_bytes()
            require(len(data) == identity['bytes'] and digest(data) == identity['sha256'], 'file identity: '+name)
            self.files[name] = data
        self.code = self.files[m['code']]
        require(len(self.code) > 0 and len(self.code) % 2 == 0, 'code word alignment')
        self.functions, self.globals, self.constants, self.debug_files = {}, {}, {}, {}
        debug = self.files[m['debug']].decode('utf-8')
        require(debug.startswith('# rcc debug info v2\n'), 'debug version')
        base, end = m['target']['code_base_words'], m['target']['code_base_words'] + len(self.code)//2
        for line in debug.splitlines():
            if match := re.fullmatch(r'file (\d+) (.+)', line):
                self._insert(self.debug_files, int(match[1]), match[2])
            elif match := re.fullmatch(r'func (\S+) (0x[0-9a-f]+)\.\.(0x[0-9a-f]+) (.+) frame (\d+)', line):
                lo, hi = int(match[2], 16), int(match[3], 16)
                require(base <= lo < hi <= end, 'function outside linked code: '+match[1])
                self._insert(self.functions, match[1], dict(pc=lo, end_pc=hi, file=match[4], frame_words=int(match[5])))
            elif match := re.fullmatch(r'global (\S+) (.+) (0x[0-9a-f]+)', line):
                address = int(match[3], 16)
                require(0 <= address <= m['target']['static_words'], 'global outside statics: '+match[1])
                self._insert(self.globals, match[1], dict(logical_word=address, type=match[2]))
            elif match := re.fullmatch(r'const (\S+) (\S+) (\d+)', line):
                self._insert(self.constants, match[1], dict(type=match[2], value=int(match[3])))
            elif match := re.fullmatch(r'line (0x[0-9a-f]+) (\d+) (\d+)', line):
                require(base <= int(match[1], 16) < end and int(match[2]) in self.debug_files, 'debug line location')
        require(self.functions and set(self.debug_files.values()) == set(m['source_map']), 'complete debug source map')
        declared = {canonical(e['origin']): e['file'] for e in m['declared_inputs']}
        require(all(name in self.files for name in declared.values()), 'declared source snapshots')
        for name, source in m['source_map'].items():
            require(source['file'] in self.files and digest(self.files[source['file']]) == source['sha256'], 'effective source: '+name)
            if source['origin'] is not None:
                origin = source['origin']
                original = declared.get(canonical(origin['location']))
                require(original in self.files and digest(self.files[original]) == origin['input_sha256'], 'source origin: '+name)
        require(int(self.files['application.static-words']) == m['target']['static_words'], 'static extent')
        self._check_package()

    @staticmethod
    def _insert(table, name, value):
        require(name not in table, 'duplicate symbol/file: '+str(name))
        table[name] = value

    def _check_package(self):
        m, target = self.manifest, self.manifest['target']
        package = self.files[m['package']]
        require(package[:4] == b'CB06', 'package signature')
        expected = []
        data = self.files.get('application.static-data')
        require(data is None or len(data) == target['static_words']*2, 'static data extent')
        if target['static_words']:
            expected.append((0, target['static_words']*2, data))
        expected.append((((target['code_segment'] << 16) | target['code_base_words'])*2, len(self.code), self.code))
        for resource in m['resources']:
            require(resource['kind'] in ('copy', 'zero'), 'resource operation')
            data = self.files[resource['file']] if resource['kind'] == 'copy' else None
            require(data is None or len(data) == resource['bytes'], 'resource extent')
            expected.append((resource['address_bytes'], resource['bytes'], data))
        cursor = 4
        for address, length, data in expected:
            require(cursor+8 <= len(package), 'truncated package command')
            actual_address, control = struct.unpack_from('<II', package, cursor)
            require((actual_address, control & 0x7fffffff, bool(control >> 31))
                    == (address, length, data is None), 'package/description command mismatch')
            cursor += 8
            if data is not None:
                require(package[cursor:cursor+length] == data, 'package/description data mismatch')
                cursor += length
        require(package[cursor:] == bytes(8), 'package END/trailing data')

    def symbol(self, name, kind='function'):
        table = {'function': self.functions, 'global': self.globals, 'constant': self.constants}[kind]
        require(name in table, f'unknown {kind}: {name}')
        result = dict(table[name], kind=kind, name=name, artifact_id=self.manifest['artifact_id'])
        target = self.manifest['target']
        if kind == 'function':
            result['segment'] = target['code_segment']
            result['physical_word'] = (result['segment'] << 16) | result['pc']
        elif kind == 'global':
            result['dseg_index'] = result['logical_word'] >> 14
            result['dseg_page'] = target['data_segments'][result['dseg_index']]
            result['physical_word'] = (result['dseg_page'] << 14) | (result['logical_word'] & 0x3fff)
        return result

    def instructions(self, function):
        """Same-build listing, authenticated against the actual code words."""
        scope = self.symbol(function)
        if hasattr(self, '_instructions'):
            return [i for i in self._instructions if scope['pc'] <= i['pc'] < scope['end_pc']]
        result = []
        base = self.manifest['target']['code_base_words']
        for line in self.files['application.asm'].decode('utf-8').splitlines():
            match = re.fullmatch(r'\s+([0-9a-f]+): ([0-9a-f]{4}(?: [0-9a-f]{4})*)\s+(.+)', line)
            if not match:
                continue
            pc = int(match[1], 16)
            words = [int(word, 16) for word in match[2].split()]
            raw = b''.join(word.to_bytes(2, 'little') for word in words)
            require(self.code[(pc-base)*2:(pc-base+len(words))*2] == raw, 'listing/code mismatch')
            if match[3].startswith('jalrel '):
                require(len(words) in (1, 2) and words[-1] & 0xff00 == 0xb700
                        and (len(words) == 1 or words[0] & 0xff00 == 0xf000), 'direct call encoding')
                immediate = (words[-1] & 255) | ((words[0] & 255) << 8 if len(words) == 2 else 0)
                bits = len(words)*8
                signed = immediate - (1 << bits) if immediate & (1 << (bits-1)) else immediate
                require(int(match[3].split()[1]) == signed, 'direct call listing/encoding mismatch')
            result.append(dict(pc=pc, words=len(words), text=match[3]))
        self._instructions = result
        result = [i for i in result if scope['pc'] <= i['pc'] < scope['end_pc']]
        require(result, 'no linked instructions for '+function)
        return result

    def source_line(self, function, statement):
        scope = self.symbol(function)
        source = self.files[self.manifest['source_map'][scope['file']]['file']].decode('utf-8').replace('\r\n', '\n')
        start = re.search(r'^(?:pub )?fn '+re.escape(function)+r'\(', source, re.M)
        require(start, 'function source '+function)
        following = re.search(r'^(?:pub )?fn ', source[start.end():], re.M)
        end = len(source) if following is None else start.end()+following.start()
        positions = [m.start() for m in re.finditer(re.escape(statement), source[start.start():end])]
        require(len(positions) == 1, 'unique source statement '+function+': '+statement)
        return source[:start.start()+positions[0]].count('\n')+1

    def call(self, caller, callee=None, statement=None):
        """Unique direct call, final Execute word and first return instruction."""
        destination = self.symbol(callee)['pc'] if callee is not None else None
        calls = [i for i in self.instructions(caller) if i['text'].startswith('jalrel ')
                 and (destination is None or (i['pc']+i['words']+int(i['text'].split()[1])) & 65535 == destination)]
        if statement is not None:
            line = self.source_line(caller, statement)
            locations = {int(m[1], 16): (self.debug_files[int(m[2])], int(m[3])) for m in
                         re.finditer(r'^line (0x[0-9a-f]+) (\d+) (\d+)$', self.files[self.manifest['debug']].decode(), re.M)}
            calls = [i for i in calls if locations.get(i['pc']) == (self.functions[caller]['file'], line)]
        require(len(calls) == 1, f'expected one {caller} -> {callee} call, found {len(calls)}')
        call = calls[0]
        following = next((i for i in self.instructions(caller) if i['pc'] == call['pc']+call['words']), None)
        require(following is not None, 'call must have a linked return instruction')
        return dict(segment=self.manifest['target']['code_segment'],
                    execute_pc=call['pc']+call['words']-1, return_pc=call['pc']+call['words'],
                    return_execute_pc=following['pc']+following['words']-1,
                    target_pc=(call['pc']+call['words']+int(call['text'].split()[1])) & 65535)

    def caller(self, callee):
        destination = self.symbol(callee)['pc']
        callers = [name for name in self.functions if any(i['text'].startswith('jalrel ') and
                   (i['pc']+i['words']+int(i['text'].split()[1])) & 65535 == destination for i in self.instructions(name))]
        require(len(callers) == 1, 'unique caller of '+callee)
        return callers[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    check = commands.add_parser('check')
    check.add_argument('artifact', type=Path)
    symbol = commands.add_parser('symbol')
    symbol.add_argument('artifact', type=Path)
    symbol.add_argument('name')
    symbol.add_argument('--kind', choices=('function', 'global', 'constant'), default='function')
    args = parser.parse_args()
    artifact = Artifact(args.artifact)
    result = (artifact.symbol(args.name, args.kind) if args.command == 'symbol' else
              dict(artifact_id=artifact.manifest['artifact_id'], verified_files=len(artifact.files),
                   functions=len(artifact.functions), globals=len(artifact.globals), source_files=len(artifact.debug_files)))
    print(json.dumps(result, ensure_ascii=False))


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError, IndexError) as error:
        raise SystemExit(str(error)) from error
