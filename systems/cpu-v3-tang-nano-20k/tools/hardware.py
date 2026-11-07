"""Audit frozen hardware and assemble/program a typed System deployment bundle."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

from application_artifact import Artifact, canonical, require

REPO = Path(__file__).resolve().parents[3]
BOOT_OFFSET = 0x100000


def identity(data):
    return dict(bytes=len(data), sha256=hashlib.sha256(data).hexdigest())


def executable(supplied):
    if supplied:
        return Path(supplied).resolve()
    subprocess.run(['cargo', 'build', '--release', '-p', 'digital-design-hardware-gowin',
                    '--bin', 'gowin-artifact'], cwd=REPO, check=True, timeout=600)
    target = Path(os.environ.get('CARGO_TARGET_DIR', REPO/'target')).resolve()
    return target/'release'/('gowin-artifact.exe' if os.name == 'nt' else 'gowin-artifact')


def audit(hardware, runtime):
    result = subprocess.run([str(runtime), str(Path(hardware).resolve())], check=True,
                            capture_output=True, text=True, encoding='utf-8', timeout=60)
    value = json.loads(result.stdout)
    require(value['schema'] == 'digitaldesign.gowin-audit' and value['version'] == 1,
            'hardware audit schema')
    require(value['project'] == 'cpu_v3_system' and value['logic_top'] == 'CpuV3System',
            'hardware is not the complete CPU V3 System')
    return value


def configuration(path):
    bits = ''.join(line.strip() for line in Path(path).read_text().splitlines()
                   if line.strip() and not line.lstrip().startswith('//'))
    require(bits and set(bits) <= {'0', '1'} and len(bits) % 8 == 0,
            'invalid Gowin FS configuration encoding')
    return bytes(int(bits[i:i+8], 2) for i in range(0, len(bits), 8))


def compose(config, package, capacity):
    require(len(config) <= BOOT_OFFSET, 'configuration overlaps application')
    require(BOOT_OFFSET + len(package) <= capacity, 'Flash capacity exceeded')
    return config + b'\xff'*(BOOT_OFFSET-len(config)) + package


def write(path, value):
    path.write_text(json.dumps(value, indent=2)+'\n', encoding='utf-8')


def assemble(hardware, application, out, runtime):
    out = Path(out).resolve()
    require(not out.exists(), 'new deployment directory required')
    artifact = Artifact(application)
    inspection = audit(hardware, runtime)
    config = configuration(inspection['bitstream'])
    package = artifact.files[artifact.manifest['package']]
    image = compose(config, package, inspection['flash_capacity_bytes'])
    out.mkdir(parents=True)
    files = {'configuration.bin': config, 'application.cb06': package, 'complete-flash.bin': image,
             'application.build.json': (artifact.root/'application.build.json').read_bytes()}
    for name, data in files.items():
        (out/name).write_bytes(data)
    value = dict(schema='digitaldesign.system-deployment', version=1,
                 hardware=inspection, artifact_id=artifact.manifest['artifact_id'],
                 application_offset=BOOT_OFFSET, files={k: identity(v) for k, v in files.items()})
    value['deployment_id'] = hashlib.sha256(canonical(value)).hexdigest()
    write(out/'deployment.json', value)
    return value


def verify(bundle, hardware, runtime):
    bundle = Path(bundle).resolve()
    value = json.loads((bundle/'deployment.json').read_text())
    require(value['schema'] == 'digitaldesign.system-deployment' and value['version'] == 1,
            'deployment schema')
    require(value['deployment_id'] == hashlib.sha256(canonical({k: v for k, v in value.items()
                                                              if k != 'deployment_id'})).hexdigest(),
            'deployment identity')
    names = {'configuration.bin', 'application.cb06', 'complete-flash.bin', 'application.build.json'}
    require(value['files'].keys() == names, 'deployment closure')
    for name, expected in value['files'].items():
        require(identity((bundle/name).read_bytes()) == expected, 'changed deployment file: '+name)
    require(value['application_offset'] == BOOT_OFFSET, 'application offset')
    current = audit(hardware, runtime)
    # A portable bundle binds contents, not the original absolute directory.
    require({k: v for k, v in current.items() if k != 'bitstream'} ==
            {k: v for k, v in value['hardware'].items() if k != 'bitstream'}, 'different hardware')
    config = configuration(current['bitstream'])
    require(config == (bundle/'configuration.bin').read_bytes(), 'configuration binding')
    require(compose(config, (bundle/'application.cb06').read_bytes(), current['flash_capacity_bytes']) ==
            (bundle/'complete-flash.bin').read_bytes(), 'Flash composition')
    return value, current


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    for name in ('audit', 'assemble', 'verify', 'program-sram', 'program-flash'):
        sub = commands.add_parser(name)
        sub.add_argument('hardware', type=Path)
        sub.add_argument('--runtime', type=Path, help='Frozen standalone gowin-artifact executable')
        if name == 'assemble':
            sub.add_argument('application', type=Path)
            sub.add_argument('--out', type=Path, required=True)
        elif name != 'audit':
            sub.add_argument('bundle', type=Path)
        if name.startswith('program-'):
            sub.add_argument('--receipt', type=Path, required=True)
            sub.add_argument('--gowin-home', type=Path)
            if name == 'program-flash':
                sub.add_argument('--kind', choices=('application', 'complete'), required=True)
    args = parser.parse_args()
    runtime = executable(args.runtime)
    if args.command == 'audit':
        value = audit(args.hardware, runtime)
    elif args.command == 'assemble':
        value = assemble(args.hardware, args.application, args.out, runtime)
    else:
        value, inspection = verify(args.bundle, args.hardware, runtime)
        if args.command.startswith('program-'):
            require(not args.receipt.exists(), 'new programming receipt required')
            args.receipt.parent.mkdir(parents=True, exist_ok=True)
            argv = [str(runtime), str(args.hardware.resolve())]
            kind = getattr(args, 'kind', 'sram')
            offset = None
            if kind == 'sram':
                payload = Path(inspection['bitstream'])
                argv += ['--program-existing']
            else:
                offset = BOOT_OFFSET if kind == 'application' else 0
                payload = args.bundle.resolve()/('application.cb06' if kind == 'application' else 'complete-flash.bin')
                argv += ['--program-flash', str(offset), str(payload)]
            if args.gowin_home:
                argv += ['--gowin-home', str(args.gowin_home.resolve())]
            receipt = dict(deployment_id=value['deployment_id'], kind=kind, offset=offset,
                           payload=identity(payload.read_bytes()), argv=argv, programmed=False)
            try:
                process = subprocess.run(argv, capture_output=True, text=True, encoding='utf-8', timeout=600)
                receipt.update(exit_code=process.returncode, stdout=process.stdout, stderr=process.stderr,
                               programmed=process.returncode == 0)
            except (OSError, subprocess.TimeoutExpired) as error:
                receipt['error'] = str(error)
            write(args.receipt, receipt)
            require(receipt['programmed'], 'programming failed; inspect receipt')
            value = receipt
    print(json.dumps(value, indent=2))


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        raise SystemExit(str(error)) from error
