"""Bounded application profiling and comparison of frozen, verified runs."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

from application_artifact import Artifact, canonical, require

SYSTEM = Path(__file__).resolve().parents[1]
REPO = SYSTEM.parents[1]


def identity(path):
    path = Path(path)
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    return dict(bytes=path.stat().st_size, sha256=digest.hexdigest())


def write(path, value):
    Path(path).write_text(json.dumps(value, indent=2) + '\n', encoding='utf-8')


def verified_run(folder, complete=True):
    folder = Path(folder).resolve()
    value = json.loads((folder/'run.json').read_text())
    require(value.get('schema') == 'digitaldesign.profile-run' and value.get('version') == 1, 'run schema')
    require(value['run_id'] == hashlib.sha256(canonical({k: v for k, v in value.items() if k != 'run_id'})).hexdigest(), 'run identity')
    require(not complete or value['complete'], 'partial run is not a performance sample')
    for name, expected in value['files'].items():
        require(not name.startswith('/') and '\\' not in name and ':' not in name
                and all(p not in ('', '.', '..') for p in name.split('/')), 'relative run file: '+name)
        path = (folder/name).resolve()
        require(path.is_relative_to(folder) and identity(path) == expected, 'changed run input/output: '+name)
    return value


def compare(left, right, difference=None):
    a, b = verified_run(left), verified_run(right)
    for key in ('scene', 'backend', 'frame', 'clock_hz', 'boundary', 'input', 'schedule'):
        require(a['request'][key] == b['request'][key], 'incompatible '+key)
    require(a['runtime'] == b['runtime'], 'different model executables')
    require(a['tools'] == b['tools'], 'different measurement/report tools')
    require(a.get('selected_state') == b.get('selected_state'), 'different selected application state')
    if a['package'] != b['package']:
        require(difference and difference.strip(), 'program/resource differences require --difference DESCRIPTION')
    require(a['metrics'].keys() == b['metrics'].keys(), 'different metrics')
    changes = {}
    for name, before in a['metrics'].items():
        after = b['metrics'][name]
        require(type(before) is int and type(after) is int and before >= 0 and after >= 0, 'invalid metric '+name)
        changes[name] = dict(before=before, after=after, delta=after-before,
                             percent=None if before == 0 else 100*(after-before)/before)
    return dict(schema='digitaldesign.profile-comparison', version=1, left=a['run_id'], right=b['run_id'],
                request=a['request'], difference=difference, metrics=changes,
                scope='Same backend and observed boundary. Host wall time is not a machine performance metric.')


def runtime(name, supplied):
    if supplied:
        path = Path(supplied).resolve()
    else:
        subprocess.run(['cargo', 'build', '--release', '-p', 'cpu-v3-tang-nano-20k', '--bin', name],
                       cwd=REPO, check=True, timeout=600)
        target = Path(os.environ.get('CARGO_TARGET_DIR', REPO/'target'))
        path = target.resolve()/'release'/(name+('.exe' if os.name == 'nt' else ''))
    require(path.is_file(), 'missing runtime '+str(path))
    return path


def seal(out, result):
    result['files'] = {p.relative_to(out).as_posix(): identity(p) for p in sorted(out.rglob('*')) if p.is_file()}
    result['run_id'] = hashlib.sha256(canonical(result)).hexdigest()
    write(out/'run.json', result)
    print(json.dumps(dict(run_id=result['run_id'], complete=result['complete'], metrics=result['metrics'], out=str(out))))
    return 0 if result['complete'] else 2


def measure(args):
    artifact = Artifact(args.artifact)
    ordinary = args.scene == 'bloxorz29-shadow'
    require(not ordinary or args.backend == 'system' and args.frame is None,
            'bloxorz29-shadow selects rolling-B with the system backend; omit --frame')
    frame = 'rolling-B' if ordinary else (2 if args.frame is None else args.frame)
    require((ordinary or 0 < frame <= 1024) and 0 < args.budget <= 80_000_000, 'frame/cycle/step limit')
    require(0 < args.seconds <= 7200 and 0 < args.events <= (112_000_000 if ordinary else 10_000_000),
            'host/event limit for the selected recorder tier')
    out = args.out.resolve()
    require(not out.exists(), 'new run directory required')
    scenario = None
    if ordinary:
        import importlib.util
        path = SYSTEM/'applications/bloxorz/tools/plan.py'
        spec = importlib.util.spec_from_file_location('bloxorz_plan', path)
        scenario = importlib.util.module_from_spec(spec); spec.loader.exec_module(scenario)
        plan = scenario.bind(artifact)
    executable = runtime('bloxorz-profile-runner' if ordinary else 'cpu-v3-system-emu' if args.backend == 'system' else 'cpu-v3-display-sim', args.runtime)
    out.mkdir(parents=True)
    shutil.copy2(executable, out/('runtime.exe' if os.name == 'nt' else 'runtime'))
    executable = out/('runtime.exe' if os.name == 'nt' else 'runtime')
    package = out/'application.cb06'
    package.write_bytes(artifact.files[artifact.manifest['package']])
    (out/'application.build.json').write_bytes((artifact.root/'application.build.json').read_bytes())
    tools = {p.relative_to(SYSTEM/'tools').as_posix(): identity(p)
             for p in [Path(__file__), SYSTEM/'tools/application_artifact.py',
                       *sorted((SYSTEM/'tools/gpu_profiler').glob('*.py')),
                       *sorted((SYSTEM/'tools/gpu_profiler').glob('*.js')),
                       *sorted((SYSTEM/'tools/gpu_profiler').glob('*.html'))]}
    if ordinary:
        tools.update({p.relative_to(SYSTEM).as_posix(): identity(p) for p in
                      sorted((SYSTEM/'applications/bloxorz/tools').glob('*.py'))})
    whole = args.backend == 'system'
    request = dict(scene=args.scene, backend=args.backend, frame=frame, clock_hz=54_000_000 if args.backend != 'sim' else None,
                   boundary='applied-to-applied' if whole else 'host-applied-to-applied-functional-gpu',
                   input='level29-neutral-warm-right-read-ack-release' if ordinary else 'disconnected',
                   schedule=None if whole else dict(host_tick_us=16667, samples=10000, budget=args.budget),
                   budget=args.budget, host_seconds=args.seconds)
    result = dict(schema='digitaldesign.profile-run', version=1, request=request,
                  artifact_id=artifact.manifest['artifact_id'], runtime=identity(executable), tools=tools,
                  package=identity(package), complete=False, metrics={})
    write(out/'request.json', result)
    if ordinary:
        require(60 <= args.report_seconds and args.seconds+args.report_seconds <= 7200,
                'capture plus report budgets must fit 7200 seconds; reserve at least sixty for reporting')
        plan.update(runtime=dict(file=executable.name, **identity(executable)),
                    program=dict(file=package.name, **identity(package)),
                    bindings=[dict(file='application.build.json', **identity(out/'application.build.json'))],
                    output='capture', report_output='report', max_logic_cycles=args.budget,
                    max_host_seconds=args.seconds, max_total_host_seconds=args.seconds+args.report_seconds,
                    max_events=args.events, max_native_command_images=2048)
        write(out/'plan.json', plan)
        argv = [sys.executable, '-B', str(SYSTEM/'applications/bloxorz/tools/profile.py'), str(out/'plan.json')]
    elif whole:
        argv = [str(executable), str(package), '--entry', 'preloaded-post-boot', '--max-logic-cycles', str(args.budget),
                '--max-host-seconds', str(args.seconds), '--profile-out', str(out/'capture'),
                '--profile-frame', str(frame), '--profile-max-events', str(args.events)]
    else:
        argv = [str(executable), '--package', str(package), '--cpu-mode', 'native' if args.backend == 'cpu' else 'instruction',
                '--max-cpu-cycles' if args.backend == 'cpu' else '--max-cpu-steps', str(args.budget),
                '--stop-after-present', str(frame), '--frames', '10000', '--out-dir', str(out/'capture'), '--ppm']
    result['argv'] = argv
    if args.command == 'plan':
        write(out/'request.json', result)
        print(json.dumps(dict(prepared=True, request=request, argv=argv, out=str(out))))
        return 0
    try:
        with (out/'stdout.log').open('wb') as stdout, (out/'stderr.log').open('wb') as stderr:
            process = subprocess.run(argv, stdout=stdout, stderr=stderr,
                                     timeout=args.seconds+(args.report_seconds+10 if ordinary else 10))
        result['exit_code'] = process.returncode
        if ordinary:
            if process.returncode == 0:
                data = json.loads((out/'report/profile.json').read_text())
                selected = next(e for e in data['system']['runner']['selections'] if e['label'] == 'rolling-B')
                result['selected_state'] = selected['state']
                result['selected_presentation'] = selected['presentation']
                result['metrics'] = system_metrics(data, selected['presentation'])
                result['complete'] = not data['partial']
        elif whole and (out/'capture/capture.json').is_file():
            sys.path.insert(0, str(SYSTEM/'tools/gpu_profiler'))
            from reporting import build_report
            data = build_report(out/'capture', out/'report', allow_partial=True, only_frame=frame)
            if process.returncode == 0 and not data['partial']:
                result['metrics'] = system_metrics(data, frame)
                result['complete'] = True
        elif not whole and process.returncode == 0:
            counters = json.loads((out/'capture/frame-counters.json').read_text())
            before, after = counters[frame-1:frame+1]
            require(after['presentation'] == frame, 'missing selected presentation')
            result['metrics'] = {name: after[name]-before[name] for name in ('cpu_advances', 'retired_words', 'logic_cycles', 'native_cycles')
                                 if after[name] is not None and before[name] is not None}
            summary = json.loads((out/'capture'/('summary-native.json' if args.backend == 'cpu' else 'summary-instruction.json')).read_text())
            require(not summary['gpu_error'] and not summary['gpu_busy'], 'functional GPU incomplete/error')
            result['complete'] = True
            write(out/'report.json', dict(request=request, metrics=result['metrics'], scope=summary['gpu_display']))
        if result['complete']:
            captured = out/'capture'/('program.bin' if whole else 'application.cb06')
            require(identity(captured) == result['package'], 'capture package identity')
    except (OSError, ValueError, KeyError, IndexError, TypeError, subprocess.TimeoutExpired) as error:
        result['complete'] = False
        result['error'] = str(error)
    if not result['complete'] and 'error' not in result:
        result['error'] = 'Selected scope incomplete; inspect capture limits, stop reason and execution logs.'
    return seal(out, result)


def system_metrics(data, frame):
    sys.path.insert(0, str(SYSTEM/'tools/gpu_profiler'))
    from system_frames import describe
    view = describe(data, frame)
    jobs = [j for j in data['system']['submissions'] if j['id'] in view['owned_submission_ids']]
    return dict(frame_logic_cycles=(view['end_tick']-view['start_tick'])//148,
                gpu_work_cycles=sum((j['end_tick']-j['start_tick'])//148 for j in jobs))


def report(folder, output):
    value = verified_run(folder)
    output = Path(output).resolve()
    require(not output.exists(), 'new report directory required')
    if value['request']['backend'] == 'system':
        sys.path.insert(0, str(SYSTEM/'tools/gpu_profiler'))
        from reporting import build_report
        frame = value.get('selected_presentation', value['request']['frame'])
        build_report(Path(folder)/'capture', output, only_frame=frame)
    else:
        output.mkdir(parents=True)
        write(output/'report.json', dict(source_run=value['run_id'], request=value['request'], metrics=value['metrics']))
    print(json.dumps(dict(source_run=value['run_id'], out=str(output))))
    return 0


def recover(folder, output, seconds):
    """Re-analyze a sealed complete capture; never execute its model again."""
    folder, output = Path(folder).resolve(), Path(output).resolve()
    original = verified_run(folder, complete=False)
    require(original['request']['backend'] == 'system', 'recovery requires a system capture')
    require(0 < seconds <= 7200 and not output.exists(), 'analysis budget/new output directory')
    meta = json.loads((folder/'capture/capture.json').read_text())
    require(meta['reached'] and not meta['truncated'], 'incomplete capture cannot be recovered as a sample')
    ordinary = original['request']['scene'] == 'bloxorz29-shadow'
    frame = meta['rolling_present'] if ordinary else original['request']['frame']
    if ordinary:
        require(meta['stable_present'] is not None and frame is not None
                and set(meta['runner_selected_presentations']) <= set(meta['selected_rasters']),
                'missing required scene rasters')
    output.mkdir(parents=True)
    shutil.copytree(folder/'capture', output/'capture')
    for name in ('runtime.exe' if os.name == 'nt' else 'runtime', 'application.cb06', 'application.build.json', 'plan.json'):
        if (folder/name).is_file():
            shutil.copy2(folder/name, output/name)
    write(output/'source-run.json', original)
    result = {k: v for k, v in original.items() if k not in ('run_id','files','error','complete','metrics')}
    result.update(complete=False, metrics={}, capture_source_run_id=original['run_id'],
                  analysis_seconds=seconds, source_tools=original['tools'])
    result['tools'] = dict(original['tools'])
    result['tools']['profile.py'] = identity(Path(__file__))
    argv = [sys.executable, '-B', str(SYSTEM/'tools/gpu_profiler/profiler.py'), 'build',
            str(output/'capture'), '--out', str(output/'report'), '--only-frame', str(frame), '--require-images']
    result['analysis_argv'] = argv
    try:
        with (output/'analysis.stdout.log').open('wb') as stdout, (output/'analysis.stderr.log').open('wb') as stderr:
            process = subprocess.run(argv, stdout=stdout, stderr=stderr, timeout=seconds)
        result['analysis_exit_code'] = process.returncode
        require(process.returncode == 0, 'offline analysis failed; inspect analysis logs')
        data = json.loads((output/'report/profile.json').read_text())
        require(not data['partial'], 'report contains incomplete evidence')
        result['metrics'] = system_metrics(data, frame)
        result['selected_presentation'] = frame
        if ordinary:
            selected = next(e for e in data['system']['runner']['selections'] if e['label'] == 'rolling-B')
            require(selected['presentation'] == frame, 'scene presentation differs')
            result['selected_state'] = selected['state']
        require(identity(output/'capture/program.bin') == result['package'], 'capture package identity')
        result['complete'] = True
    except (OSError, ValueError, KeyError, TypeError, subprocess.TimeoutExpired) as error:
        result['error'] = str(error)
    return seal(output, result)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    for action in ('run', 'plan'):
        run = commands.add_parser(action)
        run.add_argument('artifact', type=Path)
        run.add_argument('--scene', choices=('default','bloxorz29-shadow'), default='default')
        run.add_argument('--backend', choices=('system', 'cpu', 'sim'), default='system')
        run.add_argument('--frame', type=int)
        run.add_argument('--budget', type=int, required=True)
        run.add_argument('--seconds', type=int, default=600)
        run.add_argument('--report-seconds', type=int, default=1800, help='Separate ordinary-scene report budget')
        run.add_argument('--events', type=int, default=2_000_000)
        run.add_argument('--out', type=Path, required=True)
        run.add_argument('--runtime', type=Path, help='Use an explicitly frozen model executable')
    check = commands.add_parser('compare')
    check.add_argument('left', type=Path); check.add_argument('right', type=Path)
    check.add_argument('--difference'); check.add_argument('--out', type=Path)
    render = commands.add_parser('report')
    render.add_argument('folder', type=Path); render.add_argument('--out', type=Path, required=True)
    retry = commands.add_parser('recover', help='Analyze an already complete capture without another model run')
    retry.add_argument('folder', type=Path); retry.add_argument('--out', type=Path, required=True)
    retry.add_argument('--seconds', type=int, default=1800)
    args = parser.parse_args()
    if args.command in ('run','plan'):
        return measure(args)
    if args.command == 'report':
        return report(args.folder,args.out)
    if args.command == 'recover':
        return recover(args.folder,args.out,args.seconds)
    value = compare(args.left, args.right, args.difference)
    if args.out:
        require(not args.out.exists(), 'new comparison file required')
        write(args.out, value)
    print(json.dumps(value, indent=2))
    return 0


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        raise SystemExit(str(error)) from error
