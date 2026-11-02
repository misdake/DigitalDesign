"""Qualify real offline CLI outputs against a frozen report; no browser/RTL."""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import re
import sys
import subprocess

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from model import load_profile
from reporting import digest, _verify_input
from snapshot import Abi, DEFAULT_ABI, load_snapshot


def check(report, repeated, reference, expected):
    first, again, old = [load_profile(path/'profile.json') for path in (report, repeated, reference)]
    for key in old:
        if key not in ('images', 'provenance'):
            assert first[key] == old[key], 'Changed numeric/ownership profile field: '+key
    assert first == again, 'Default repeat rebuild changed profile data'
    records = [first['images']['final'], *first['images']['commands'].values()]
    assert len(records) == expected
    assert len(list((report/'images').glob('*.png'))) == expected
    original = {image['file']:image for image in [old['images']['final'], *old['images']['commands'].values()]}
    hashes = {}
    for image in records:
        name = image['file']
        png = (report/'images'/name).read_bytes()
        assert png == (repeated/'images'/name).read_bytes()
        assert png == base64.b64decode(image['src'].split(',', 1)[1])
        assert hashlib.sha256(png).hexdigest() == original[name]['png_sha256']
        assert image['plane_sha256'] == original[name]['plane_sha256']
        assert image['preview']['encoding'] == 'srgb'
        assert image['preview']['capture_format_known'] is False
        hashes[name] = image['png_sha256']
    for directory in (report, repeated):
        html = (directory/'profiler.html').read_text(encoding='utf-8')
        embedded = json.loads(re.search(r'^const D=(.*);$', html, re.MULTILINE)[1])
        assert embedded == first
        assert '#frame-wrap[hidden]{display:none}' in html
        assert ".removeAttribute('src')" in html
    binding = first['provenance']['report_inputs']
    capture = load_snapshot(binding['snapshot']['directory'], Abi(DEFAULT_ABI))
    assert _verify_input(capture, first, Path(binding['capture_directory']), report.parent) is None
    for name, sha in binding['capture_files'].items():
        assert digest(Path(binding['capture_directory'])/name) == sha
    for group in ('snapshot', 'raw'):
        for name, sha in binding[group]['files'].items():
            assert digest(Path(binding[group]['directory'])/name) == sha
    assert first['images']['final']['plane_sha256'] == binding['raw']['files']['color.bin']
    assert first['images']['final']['depth_plane_sha256'] == binding['raw']['files']['depth.bin']
    return dict(complete=True, images=expected, input_sha256=binding['input_sha256'],
                profile_unchanged_except_images_and_provenance=True,
                raw_and_capture_evidence_unchanged=True, default_repeat_byte_identical=True,
                bound_snapshot_trace_revalidated=True,
                browser_visual_verification=False, png_sha256=hashes,
                reports={str(path):dict(html_sha256=digest(path/'profiler.html'),
                                       profile_sha256=digest(path/'profile.json')) for path in (report, repeated)},
                reference=str(reference), dependencies=binding)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    for name in ('report', 'repeated', 'reference', 'out'):
        parser.add_argument('--'+name, type=Path, required=True)
    parser.add_argument('--expect-images', type=int, default=102)
    parser.add_argument('--first-command', required=True, help='Exact initial public CLI invocation, without a prior binding')
    args = parser.parse_args()
    result = check(args.report, args.repeated, args.reference, args.expect_images)
    assert '--snapshot' in args.first_command and '--raw' in args.first_command and '--binding' not in args.first_command
    result['first_public_cli_command'] = args.first_command
    record = result['dependencies']
    result['reproduction_command'] = subprocess.list2cmdline([sys.executable, '-B',
        str(Path(__file__).resolve().parents[1]/'profiler.py'), 'build', record['capture_directory'],
        '--snapshot', record['snapshot']['directory'], '--raw', record['raw']['directory'],
        '--out', '<NEW_EMPTY_REPORT_DIRECTORY>', '--require-images'])
    result['dependency_scope'] = 'Explicit external input files are required for rebuilding; HTML/PNG are self-contained for viewing'
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(result, indent=2)+'\n')
    print(json.dumps(dict(complete=True, images=result['images'], evidence=str(args.out))))
