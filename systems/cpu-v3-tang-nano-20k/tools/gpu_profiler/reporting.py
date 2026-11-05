"""One production path from bound raw capture inputs to a complete offline report."""
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import struct
import tempfile
import uuid

from images import build as build_images, display_reference, preview_policy
from model import build_capture, load_profile, write_report
from snapshot import Abi, DEFAULT_ABI, SnapshotError, load_snapshot

SEEDS = ('memory.hex', 'valid.hex', 'marked.hex', 'writable.hex')


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def image_options(parser, *, snapshot=True, replace=False):
    if snapshot:
        parser.add_argument('--snapshot', type=Path, help='Frozen GPS1 input for historical command images')
        parser.add_argument('--raw', type=Path, help='Same-run raw output folder; defaults to the capture folder')
        parser.add_argument('--binding', type=Path, help='Verify dependencies against a previous report-inputs.json')
    parser.add_argument('--preview', choices=['display1', 'raw', 'capture'], default='display1',
                        help='Saved PNG policy (default: actual Display1 LUT before PNG encoding)')
    parser.add_argument('--capture-format', choices=['unknown', 'known'], default='unknown',
                        help='Whether the capture format field is authoritative; zero is not proof')
    parser.add_argument('--require-images', action='store_true', help='Refuse a timing-only/incomplete image report')
    if replace:
        parser.add_argument('--replace-report', action='store_true',
                            help='Replace a generated report after verification; preserve its files and hashes in an archive')


def _replace_report(output, build):
    """Stage a report, archive only recognized generated files, then publish."""
    requested = Path(output).absolute()
    output = requested.resolve()
    if requested != output or not output.is_dir():
        raise SnapshotError('Replacement requires an existing ordinary report directory')
    names = {'profile.json', 'profiler.html', 'validation.json', 'commands.csv', 'cpu.csv',
             'meshlets.csv', 'submissions.csv', 'memory-transactions.csv', 'report-inputs.json',
             'report-replacement.json', 'system-memory.csv', 'system-state.csv', 'system-transactions.csv'}
    names |= {'frame-summary.json', 'system-submissions.csv', 'scanout-selected.png'}
    previous = load_profile(output/'profile.json')
    reports = {Path('.'): previous}
    frame_index = output/'frames/index.json'
    if frame_index.is_file():
        records = json.loads(frame_index.read_text())
        for record in records:
            number = record['presentation']
            if type(number) is not int or number < 1:
                raise SnapshotError('Replacement requires generated frame identities')
            relative = Path('frames')/f'frame-{number}'
            frame = load_profile(output/relative/'profile.json')
            if relative in reports or record != dict(presentation=number, html=f'frame-{number}/profiler.html',
                                                     frame=frame['system']['frame_view']):
                raise SnapshotError('Replacement requires matching generated frame index')
            reports[relative] = frame
    paths = list(output.rglob('*'))
    for path in [output, *paths]:
        if path.is_symlink() or getattr(path.lstat(), 'st_file_attributes', 0) & 0x400:
            raise SnapshotError('Replacement refuses links/reparse points')
        relative = path.relative_to(output)
        if path == output:
            continue
        allowed = frame_index.is_file() and relative in (Path('frames'), Path('frames/index.json'))
        for root, profile in reports.items():
            if relative == root and path.is_dir():
                allowed = True
            if not relative.is_relative_to(root):
                continue
            local = relative.relative_to(root)
            allowed |= len(local.parts) == 1 and path.is_file() and (
                path.name in names or bool(re.fullmatch(r'scanout-\d{6}\.png', path.name)))
            allowed |= local == Path('images') and path.is_dir()
            if len(local.parts) == 2 and local.parts[0] == 'images' and path.is_file():
                allowed |= bool(re.fullmatch(r'(?:index\.json|final\.png|command-\d{6}\.png)', path.name))
                images = profile.get('images', {})
                if path.name in images.get('assets', {}):
                    identity = images['assets'][path.name]
                    if digest(path) != identity['sha256'] or path.stat().st_size != identity['bytes']:
                        raise SnapshotError('Replacement requires matching native raw assets')
                    allowed = True
                allowed |= path.name == 'native-index.json' and 'preimage_sha256' in images
                allowed |= any(path.name == image['file'] and digest(path) == image['png_sha256']
                               for image in images.get('commands', {}).values() if 'png_sha256' in image)
        if not allowed:
            raise SnapshotError('Replacement refuses unknown report content: '+str(relative))
    for root, profile in reports.items():
        html = (output/root/'profiler.html').read_text(encoding='utf-8')
        embedded = re.search(r'^const D=(.*);$', html, re.MULTILINE)
        from model import viewer_model
        if not embedded or json.loads(embedded[1]) not in (profile,viewer_model(profile)):
            raise SnapshotError('Replacement requires matching generated HTML/profile data')
    hashes = {str(p.relative_to(output)):digest(p) for p in paths if p.is_file()}
    token = uuid.uuid4().hex
    stage = output.with_name(output.name+'.stage-'+token)
    archive = output.with_name(output.name+'.archive-'+token)
    if stage.parent != output.parent or archive.parent != output.parent or stage.exists() or archive.exists():
        raise SnapshotError('Unsafe replacement target')
    data = build(stage)  # Failure retains evidence and leaves the published report intact.
    if hashes != {str(p.relative_to(output)):digest(p) for p in output.rglob('*') if p.is_file()}:
        raise SnapshotError('Published report changed while staging its replacement')
    mapping = dict(schema='digitaldesign.gpu-report-replacement', version=1,
                   original_directory=str(output), archive_directory=str(archive), files=hashes)
    (stage/'report-replacement.json').write_text(json.dumps(mapping, indent=2)+'\n')
    output.rename(archive)
    try:
        if hashes != {str(p.relative_to(archive)):digest(p) for p in archive.rglob('*') if p.is_file()}:
            raise SnapshotError('Archived report identity differs')
        stage.rename(output)
    except Exception:
        archive.rename(output)
        raise
    return data


def fresh_output(path, source=None):
    path = Path(path).resolve()
    # Legacy build without --out writes beside its raw inputs, but never replaces
    # an already published report. Explicit output directories must be fresh.
    if path.exists() and any(path.iterdir()) and (source is None or path != Path(source).resolve()):
        raise SnapshotError('Report output must be a new directory: '+str(path))
    if any((path/name).exists() for name in ('profile.json', 'profiler.html', 'report-inputs.json')):
        raise SnapshotError('Report output already contains a report: '+str(path))
    return path


def _files(folder, names):
    result = {}
    for name in sorted(set(names)):
        if Path(name).name != name:
            raise SnapshotError('Report evidence must use local filenames')
        path = folder/name
        if not path.is_file():
            raise SnapshotError('Missing report evidence: '+str(path))
        result[name] = digest(path)
    return result


def _check_files(folder, expected):
    actual = _files(folder, expected)
    if actual != expected:
        raise SnapshotError('Report dependency identity changed in '+str(folder))


def _binding(path, folder, snapshot, raw):
    if path is None:
        return None, snapshot, raw
    value = json.loads(Path(path).read_text())
    if value.get('schema') != 'digitaldesign.gpu-report-inputs' or value.get('version') != 1:
        raise SnapshotError('Unsupported report input binding')
    _check_files(folder, value['capture_files'])
    if 'snapshot' in value:
        snapshot = Path(snapshot or value['snapshot']['directory'])
        _check_files(snapshot, value['snapshot']['files'])
    if 'raw' in value:
        raw = Path(raw or value['raw']['directory'])
        _check_files(raw, value['raw']['files'])
    return value, snapshot, raw


def _verify_input(capture, data, folder, scratch):
    identity = capture.manifest()['input_sha256']
    declared = data['provenance'].get('input_sha256')
    if declared is not None and declared != identity:
        raise SnapshotError('Snapshot input identity differs from the trace')
    missing = [name for name in SEEDS if not (folder/name).is_file()]
    if missing:
        return 'Snapshot-to-trace proof requires actual seed files: '+', '.join(missing)
    # Reuse the production seed generator, not a second memory interpretation.
    from replay import seed_files
    scratch.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='gpu-report-seeds-', dir=scratch) as temp:
        seed_files(capture, Path(temp))
        for name in SEEDS:
            if digest(Path(temp)/name) != digest(folder/name):
                raise SnapshotError('Snapshot seed does not match recorded capture: '+name)
    submissions = data['submissions']
    if data['partial']:
        return 'Incomplete timing capture; historical command images unavailable'
    if len(submissions) != 1:
        return 'Historical snapshot images currently require one bound submission'
    if submissions[0]['base'] != capture.header['cb_base'] or submissions[0]['bytes'] != capture.header['cb_bytes']:
        raise SnapshotError('Trace submission differs from the frozen snapshot CB')
    words = list(struct.unpack('<'+'I'*(capture.header['cb_bytes']//4),
                               capture.read(capture.header['cb_base'], capture.header['cb_bytes'])))
    commands = data['commands']
    if len(commands) != len(words) or any(c['pc'] != index*4 or int(c['word'], 16) != word
                                         for index, (c, word) in enumerate(zip(commands, words))):
        raise SnapshotError('Trace commands differ from the frozen snapshot CB')
    return None


def build_report(folder, output, *, snapshot=None, raw=None, binding=None, abi=None,
                 allow_partial=False, preview='display1', capture_format='unknown', require_images=False,
                 image_output=None, _profile=None, replace_report=False, frames=(), source_identity=None, only_frame=None):
    """Rebuild from original evidence; never take image pixels from old reports."""
    if replace_report:
        if image_output is not None:
            raise SnapshotError('Replacement images must stay within the report output')
        return _replace_report(output, lambda stage: build_report(folder, stage, snapshot=snapshot,
            raw=raw, binding=binding, abi=abi, allow_partial=allow_partial, preview=preview,
            capture_format=capture_format, require_images=require_images, _profile=_profile,
            frames=frames, source_identity=source_identity, only_frame=only_frame))
    folder = Path(folder).resolve()
    output = fresh_output(output, folder)
    metadata=json.loads((folder/'capture.json').read_text())
    if metadata.get('schema') in ('digitaldesign.system-emu-capture','digitaldesign.profiling-capture'):
        if metadata.get('source_kind')=='gpu-rtl' and snapshot is not None:
            captured=load_snapshot(snapshot,abi or Abi(DEFAULT_ABI))
            if captured.manifest()['input_sha256']!=metadata.get('input_sha256'):
                raise SnapshotError('Common RTL capture belongs to another snapshot')
            snapshot=None;abi=None
        if raw is not None and Path(raw).resolve()==folder:raw=None
        if snapshot is not None or raw is not None or abi is not None or _profile is not None:
            raise SnapshotError('This capture consumes its self-contained event/preimage evidence')
        from system_emu import build
        return build(folder, output, allow_partial=allow_partial, binding=binding, require_images=require_images,
                     frames=frames, source_identity=source_identity, only_frame=only_frame, preview=preview)
    if frames or source_identity is not None or only_frame is not None:
        raise SnapshotError('Application frame export/identity requires a full system capture')
    if capture_format not in ('known', 'unknown'):
        raise SnapshotError('Unknown capture-format declaration')
    if preview not in ('display1', 'raw', 'capture'):
        raise SnapshotError('Unknown preview policy: '+str(preview))
    display_reference.cache_clear()
    previous, snapshot, raw = _binding(binding, folder, snapshot, raw)
    data = build_capture(folder, allow_partial) if _profile is None else _profile
    if snapshot is None:
        declared = data['provenance'].get('report_snapshot')
        if declared:
            snapshot = (folder/declared).resolve()
    reason = None
    capture = None
    if snapshot is None:
        reason = 'No frozen snapshot declared; timing-only report'
    else:
        snapshot = Path(snapshot).resolve()
        capture = load_snapshot(snapshot, abi or Abi(DEFAULT_ABI))
        if previous and previous.get('input_sha256') != capture.manifest()['input_sha256']:
            raise SnapshotError('Report binding belongs to another snapshot')
        reason = _verify_input(capture, data, folder, output.parent)
        if not capture.header.get('has_target', True):
            reason = 'This submission has no render target'
    raw = Path(raw or folder).resolve()
    if reason is None and any(not (raw/(role+'.bin')).is_file() for role in ('color', 'depth')):
        reason = 'Same-run raw color.bin and depth.bin are required for complete image validation'
    if reason is None:
        # A separate raw directory must identify this same input/run, not merely
        # contain a plausible color image from another scene.
        if raw != folder:
            metadata_path = raw/'capture.json'
            if not metadata_path.is_file():
                raise SnapshotError('Separate raw folder requires its capture.json identity')
            metadata = json.loads(metadata_path.read_text())
            if metadata.get('input_sha256') != capture.manifest()['input_sha256']:
                raise SnapshotError('Raw output belongs to another snapshot')
            if digest(metadata_path) != digest(folder/'capture.json'):
                raise SnapshotError('Raw output capture identity differs from the trace')
            for name in SEEDS:
                if not (raw/name).is_file() or digest(raw/name) != digest(folder/name):
                    raise SnapshotError('Raw output seed identity mismatch: '+name)
            if not (raw/'events.txt').is_file() or digest(raw/'events.txt') != digest(folder/'events.txt'):
                raise SnapshotError('Raw output belongs to another trace')
            if not (raw/'load.csv').is_file() or digest(raw/'load.csv') != digest(folder/'load.csv'):
                raise SnapshotError('Raw output load samples belong to another trace')
        for role in ('color', 'depth'):
            if (raw/(role+'.bin')).stat().st_size != capture.header['plane_bytes']:
                raise SnapshotError('Incomplete raw '+role+' plane')
        linear, _ = preview_policy(preview, capture.header['target_format'], capture_format == 'known')
        if previous and previous.get('display_reference') and linear:
            if previous['display_reference'] != display_reference()[1]:
                raise SnapshotError('Display1 LUT dependency changed')
    if reason and require_images:
        raise SnapshotError('Complete report unavailable: '+reason)
    output.mkdir(parents=True, exist_ok=True)
    image_output = Path(image_output or output/'images')
    if reason:
        data['images'] = dict(available=False, reason=reason)
    else:
        data['images'] = build_images(capture, data, folder/'events.txt', image_output,
                                      preview_mode=preview, capture_format_known=capture_format == 'known', include_depth=True)
        if data['images']['available'] and data['images']['final']['plane_sha256'] != digest(raw/'color.bin'):
            raise SnapshotError('Historical final image differs from same-run raw color output')
        if data['images']['available'] and data['images']['final']['depth_plane_sha256'] != digest(raw/'depth.bin'):
            raise SnapshotError('Historical final depth differs from same-run raw depth output')
        if require_images and not data['images']['available']:
            raise SnapshotError('Complete report unavailable: '+data['images']['reason'])
    names = ['capture.json', 'events.txt', 'load.csv',
             *data['provenance'].get('source_sha256', {}), *data['provenance'].get('inputs_sha256', {})]
    record = dict(schema='digitaldesign.gpu-report-inputs', version=1, capture_directory=str(folder),
                  capture_files=_files(folder, names), preview_policy=preview,
                  capture_format_known=capture_format == 'known', images_available=data['images']['available'])
    if capture is not None:
        record['input_sha256'] = capture.manifest()['input_sha256']
        record['snapshot'] = dict(directory=str(snapshot), files=_files(snapshot,
            ['snapshot.json', *(r['file'] for r in capture.manifest()['ranges'])]))
    if data['images']['available']:
        record['raw'] = dict(directory=str(raw), files=_files(raw, ['color.bin', 'depth.bin']))
        if data['images']['final']['preview']['encoding'] == 'srgb':
            record['display_reference'] = display_reference()[1]
    data['provenance']['report_inputs'] = record
    write_report(data, output)
    (output/'report-inputs.json').write_text(json.dumps(record, indent=2)+'\n')
    return data


def render_report(profile, output, *, require_images=False, replace_report=False, frames=(), source_identity=None, only_frame=None):
    """Materialize the validated inline PNG bytes; never apply a second LUT."""
    if replace_report:
        return _replace_report(output, lambda stage: render_report(profile, stage, require_images=require_images,
                                                               frames=frames, source_identity=source_identity, only_frame=only_frame))
    output = fresh_output(output)
    data = load_profile(profile)
    if 'system' in data:
        from system_emu import prepare_report
        data=prepare_report(data)
    if data.get('system', {}).get('frame_view',{}).get('session_report'):
        # Re-rendering a frame elsewhere must retain a usable canonical link.
        frame = dict(data['system']['frame_view'])
        session = (Path(profile).parent / frame['session_report']).resolve()
        frame['session_report'] = os.path.relpath(session, output).replace('\\', '/')
        data = dict(data, system=dict(data['system'], frame_view=frame))
    if frames or source_identity is not None or only_frame is not None:
        if 'system' not in data:
            raise SnapshotError('Application frame export/identity requires a full system profile')
        from system_frames import attach_identity
        data = attach_identity(data, source_identity)
    if only_frame is not None:
        if frames:raise SnapshotError('--only-frame and --frames are mutually exclusive')
        from system_frames import make_gpu
        data=make_gpu(data,only_frame)
    available = data.get('images', {}).get('available', False)
    frame=data.get('system',{}).get('frame_view')
    raster_available=frame['scanout_index'] is not None if frame else bool(data.get('system',{}).get('scanouts'))
    if require_images and not available and not raster_available:
        raise SnapshotError('Complete report unavailable: profile has no image history')
    output.mkdir(parents=True, exist_ok=True)
    if 'system' in data:
        from system_images import copy_assets
        copy_assets(data,Path(profile).parent,output)
    if available:
        image_dir = output/'images'
        image_dir.mkdir(exist_ok=True)
        images = data['images']
        for image in [images['final'], *images['commands'].values()]:
            name = image['file']
            pattern=r'native-[0-9a-f]{64}\.png' if 'system' in data else r'(?:final|command-\d{6})\.png'
            if not re.fullmatch(pattern, name):
                raise SnapshotError('Unsafe embedded image filename')
            png = base64.b64decode(image['src'].split(',', 1)[1], validate=True)
            (image_dir/name).write_bytes(png)
        portable = dict(images, final={k:v for k,v in images['final'].items() if k != 'src'},
            commands={k:{a:b for a,b in image.items() if a != 'src'} for k,image in images['commands'].items()})
        (image_dir/'index.json').write_text(json.dumps(portable, indent=2)+'\n')
    write_report(data, output)
    if data['provenance'].get('report_inputs'):
        (output/'report-inputs.json').write_text(json.dumps(data['provenance']['report_inputs'], indent=2)+'\n')
    if frames:
        from system_frames import write_reports
        write_reports(data, output, frames)
    return data


def summary(data, output):
    output = Path(output).resolve()
    if 'system' in data:
        s=data['system']
        frame=s.get('frame_view');commands=[c for c in data['commands'] if not frame or c['submission'] in frame['owned_submission_ids']]
        native=data.get('images',{})
        return dict(out=str(output), profile=str(output/'profile.json'), html=str(output/'profiler.html'),
                    source_kind=data['provenance']['source_kind'], submissions=len(frame['owned_submission_ids']) if frame else len(s['submissions']),
                    commands=len(commands) if s['capabilities'].get('command_control_lineage') else None,
                    instruction_owner_lineage='observed' if s['capabilities'].get('gpu_signal_journal') else 'unavailable',partial=data['partial'],
                    images_available=(frame['scanout_index'] is not None if frame else bool(s['scanouts'])) or native.get('available',False), image_count=int(frame['scanout_index'] is not None) if frame else len(s['scanouts']),
                    native_images_available=native.get('available',False),native_image_count=len(native.get('commands',{})),
                    client_boundary=s['client_boundary'], native_boundary=s['native_boundary'])
    images = data.get('images', {})
    return dict(out=str(output), profile=str(output/'profile.json'), html=str(output/'profiler.html'),
                submissions=len(data['submissions']), commands=len(data['commands']), partial=data['partial'],
                images_available=images.get('available', False),
                image_count=1+len(images['commands']) if images.get('available') else 0,
                unavailable_reason=images.get('reason'),
                rebuild_dependencies=('External files declared in report-inputs.json; not an input data bundle'
                                      if data['provenance'].get('report_inputs') else None),
                outputs=sorted(str(path.relative_to(output)) for path in output.rglob('*') if path.is_file()))
