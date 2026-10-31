"""Qualify C callers against a hash-verified, read-only A source stage.

No A/B production implementation is copied into the C source tree. The actual
System loader checks its approved root; generated local modules are ordinary
application sources. Device peers retire protocol work; --native-rtl runs the
production GPU/MC before returning completion and importing writable outputs.
"""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[3]
SYSTEM = ROOT / 'systems/cpu-v3-tang-nano-20k'


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def prepare(args):
    stage = args.a_stage.resolve()
    manifest = json.loads((stage/'manifest.json').read_text())
    for item in manifest['files']:
        source = stage/'sources'/item['path']
        if digest(source) != item['sha256']:
            raise ValueError(f'External stage changed: {source}')
    output = args.output.resolve()
    output.relative_to(ROOT/'target')
    crate = output/'probe'
    local = crate/'local'
    (crate/'src').mkdir(parents=True, exist_ok=True)
    local.mkdir(exist_ok=True)
    origins = []
    inputs = [SYSTEM/'rcc/gpu_demo_api.rs', SYSTEM/'rcc/profiler_clock.rs']
    for directory in [args.source.resolve().parent, *args.extra_dir]:
        inputs.extend(sorted(directory.glob('*.rs')))
    for source in inputs:
        destination = local/source.name
        if destination.exists() and destination.read_bytes() != source.read_bytes():
            raise ValueError(f'Use a fresh output; ordinary module differs: {destination}')
        shutil.copy2(source, destination)
        origins.append(dict(path=str(source),sha256=digest(source)))
    dependencies = '\n'.join(f'{name}={{path={json.dumps(str(ROOT/path))}}}'
                              for name,path in [('cpu-v3','ip/cpu-v3'),('rcc','compiler/rcc')])
    # Cargo shares dependency caches, but each generated root needs a distinct
    # executable/fingerprint. Equal package names can leave another root's
    # executable in the common target directory even when Cargo reports fresh.
    package = 'api-c-caller-' + hashlib.sha256(str(crate).encode()).hexdigest()[:16]
    (crate/'Cargo.toml').write_text('[package]\nname="'+package+'"\nversion="0.1.0"\nedition="2021"\n[workspace]\n[dependencies]\n'+dependencies+'\nserde_json="1"\n')
    loader = stage/'sources/systems/cpu-v3-tang-nano-20k/build_support/rcc_modules.rs'
    driver = SYSTEM/'tests/support/api_c_caller_probe.rs'
    (crate/'src/main.rs').write_text('#![allow(dead_code)]\n'+f'#[path={json.dumps(str(loader))}] mod resolver;\n'+f'#[path={json.dumps(str(driver))}] mod driver;\nfn main(){{driver::run();}}\n')
    shared = loader.parent.parent/'rcc'
    mode = '--session' if args.session else str(args.assets.resolve()) if args.assets else '--compile-only'
    cargo = ['--manifest-path',str(crate/'Cargo.toml'),'--',str(args.source.resolve()),str(shared),str(local),str(output),str(args.constants.resolve()),args.entry,mode,str(args.max_steps)]
    if args.capture_build_dir:
        cargo.extend([str(args.capture_build_dir.resolve()),str(args.capture_gate.resolve())])
    if args.native_rtl:
        if not args.capture_build_dir: raise ValueError('native callback requires a capture build')
        cargo.extend([str(args.native_rtl.resolve()),str(args.native_timeout)])
    identity = dict(A_manifest_sha256=digest(stage/'manifest.json'),C_head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),ordinary_sources=origins,driver_sha256=digest(driver),source_sha256=digest(args.source.resolve()),constants_sha256=digest(args.constants.resolve()),capture_enabled=bool(args.capture_build_dir),native_timeout_seconds=args.native_timeout,wall_timeout_seconds=args.wall_timeout,scope='fixed source caller qualification; controlled completion; not final G3')
    if args.assets:
        catalog=json.loads(args.assets.read_text())
        for resource in catalog['resources']:
            path=args.assets.parent/resource['file']
            if digest(path)!=resource['sha256']:
                raise ValueError(f'Asset changed: {path}')
        identity['asset_manifest_sha256']=digest(args.assets)
    (output/'source-identity.json').write_text(json.dumps(identity,indent=2)+'\n')
    return cargo,output


def main():
    parser=argparse.ArgumentParser(__doc__)
    parser.add_argument('--a-stage',type=Path,required=True)
    parser.add_argument('--source',type=Path,required=True)
    parser.add_argument('--entry',required=True)
    parser.add_argument('--constants',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--assets',type=Path)
    parser.add_argument('--extra-dir',type=Path,action='append',default=[])
    parser.add_argument('--session',action='store_true')
    parser.add_argument('--max-steps',type=int,default=20_000_000)
    parser.add_argument('--capture-build-dir',type=Path)
    parser.add_argument('--capture-gate',type=Path)
    parser.add_argument('--native-rtl',type=Path)
    parser.add_argument('--native-timeout',type=int,default=300)
    parser.add_argument('--wall-timeout',type=int)
    args=parser.parse_args()
    if not 1 <= args.native_timeout <= 3600:
        parser.error('native timeout must be 1..3600 seconds')
    if args.wall_timeout is None:
        args.wall_timeout=720 if args.native_rtl else 600
    if not 1 <= args.wall_timeout <= 7200:
        parser.error('wall timeout must be 1..7200 seconds')
    if bool(args.capture_build_dir)!=bool(args.capture_gate):
        parser.error('capture build and gate must be provided together')
    cargo,output=prepare(args)
    quote=lambda value:"'"+str(value).replace("'","''")+"'"
    command="$ProgressPreference='SilentlyContinue'; & "+quote(ROOT/'scripts/run-cargo.ps1')+' -Subcommand run -Label '+quote('api-c-caller-'+args.entry)+' -CargoArgs @('+','.join(map(quote,cargo))+'); exit $LASTEXITCODE'
    environment=dict(os.environ,CARGO_TARGET_DIR=str(ROOT/'target'))
    encoded=base64.b64encode(command.encode('utf-16le')).decode('ascii')
    # Per-replay and whole-command wall budgets are explicit and finite;
    # callers can allow simulation overhead without changing the cycle bound.
    run_result=subprocess.run([shutil.which('pwsh') or 'powershell','-NoProfile','-NonInteractive','-OutputFormat','Text','-EncodedCommand',encoded],cwd=ROOT,env=environment,check=False,timeout=args.wall_timeout)
    label='api-c-caller-'+args.entry+'-release'
    for suffix in ('.log','.json'):
        shutil.copy2(ROOT/'target/cargo-summaries'/f'{label}{suffix}',output/f'cargo{suffix}')
    if run_result.returncode:
        raise SystemExit(run_result.returncode)
    expected={
        'same_cb_rtt_once':[2],
        'renderer_member_three_frames':[2,2,2],
        'renderer_changed_frames':[2,2,2,2,4,2,2,2,2,2,2],
        'renderer_rtt_two_updates':[1,1,1,1],
        'profiler_full_three_frames':[24,24,24],
        'profiler_minimal_three_frames':[1,1,1],
    }
    if args.assets:
        for profile in ('release','debug'):
            result=json.loads((output/profile/'result.json').read_text())
            streams=json.loads((output/profile/'commands.json').read_text())
            if args.entry in expected:
                draws=[sum(1 for i in range(1,len(words),2) if words[i]>>12==3) for words in streams]
                if draws!=expected[args.entry]:
                    raise ValueError(f'{profile}: wrong actual recorded draw counts {draws}')
            fronts=result['execution']['confirmed_fronts']
            if ((args.entry in expected or args.entry.startswith('renderer_game')) and not fronts) or any(base!=(0x220000 if i%2==0 else 0x200000) for i,base in enumerate(fronts)):
                raise ValueError(f'{profile}: wrong confirmed Front sequence {fronts}')
    if args.session:
        reference=json.loads((SYSTEM/'tests/fixtures/api_c_session_reference.json').read_text())
        if digest(args.source)!=reference['scenario_sha256']:
            raise ValueError('Session scenario changed; baseline needs explicit requalification')
        for profile in ('release','debug'):
            trace=output/profile/'session-ticks.json'
            if digest(trace)!=reference['trace_sha256'] or len(json.loads(trace.read_text()))!=reference['ticks']:
                raise ValueError(f'{profile}: visible session behavior differs from baseline')
        print('Both profiles match all 3600 baseline session ticks.')


if __name__=='__main__':
    main()
