"""Attribute a bounded instruction or scalar execute-occupancy histogram."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import struct


def profile(snapshot, histogram):
    debug = (snapshot/'application.dbg').read_text(encoding='utf-8')
    counts = struct.unpack('<65536Q', histogram.read_bytes())
    functions = []
    for m in re.finditer(r'^func (.*?) 0x([0-9a-f]+)\.\.0x([0-9a-f]+) (.*?) frame (\d+)$', debug, re.M):
        name, first, end, file, stack = m.groups()
        first, end = int(first, 16), int(end, 16)
        functions.append(dict(name=name, file=file, count=sum(counts[first:end]),
                              first_word=first, end_word=end, stack_words=int(stack)))
    functions.sort(key=lambda f: -f['count'])
    locations = {int(pc, 16): (file, line) for pc, file, line in
                 re.findall(r'^line 0x([0-9a-f]+) (\d+) (\d+)$', debug, re.M)}
    lines = {}
    for pc, (file, line) in locations.items():
        if counts[pc]:
            lines[file, line] = lines.get((file, line), 0)+counts[pc]
    files = dict(re.findall(r'^file (\d+) (.*)$', debug, re.M))
    total = sum(counts)
    manifest = json.loads((snapshot/'application.build.json').read_text())
    return dict(format='tankracer.cpu-profile.v1', artifact_id=manifest['artifact_id'],
                histogram_sha256=hashlib.sha256(histogram.read_bytes()).hexdigest(),
                unit='scalar execute occupancy cycles (fetch and FPU-only cycles excluded)' if histogram.suffix == '.cpu-execute64' else 'retired instructions in the instruction simulator',
                total=total, mapped=sum(f['count'] for f in functions), exclusive_functions=functions,
                source_lines=[dict(file=files[f], line=int(l), count=c) for (f, l), c in sorted(lines.items(), key=lambda x: -x[1])])


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('snapshot', type=Path)
    parser.add_argument('histogram', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.histogram.suffix not in ('.cpu-pc64', '.cpu-execute64'):
        parser.error('use the raw histogram emitted by the workload or CPU timing tool')
    result = profile(args.snapshot, args.histogram)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2)+'\n', encoding='utf-8')
    for item in result['exclusive_functions'][:20]:
        fraction = item['count']/max(1, result['total'])
        print(f"{item['count']:9} {fraction:6.1%} {item['file']}::{item['name']}")
    print(f"total={result['total']} mapped={result['mapped']} unit={result['unit']}")
