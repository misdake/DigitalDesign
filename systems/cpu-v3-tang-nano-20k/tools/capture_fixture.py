"""Materialize portable identities for bounded CPU producer fixtures, never board receipts."""
from pathlib import Path
import argparse
import hashlib
import json
import re
import shutil
import subprocess
import re

ROOT=Path(__file__).resolve().parents[3]
SYSTEM=ROOT/'systems/cpu-v3-tang-nano-20k'


def sha(path): return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def identity(output, assets, version, rtl=None, native_tail=False, native_reserved=False):
    output=Path(output).resolve();assets=Path(assets).resolve()
    catalog=json.loads((assets/'assets.json').read_text())
    if version != 2 or catalog["capture_layout"]["abi"] != 2:
        raise ValueError('Only explicit column capture fixtures are supported')
    column=True
    source_root=SYSTEM/'rcc'
    if native_tail:
        source_root=output/'fixture-rcc';source_root.mkdir(parents=True,exist_ok=True)
        extension=(SYSTEM/'tests/fixtures/capture_native_tail.rcc').read_text()
        if native_reserved:
            marker='p.write(4, 0x6a6a);'
            if extension.count(marker)!=1:raise ValueError('Native negative fixture marker moved')
            extension=extension.replace(marker,'p.write(2, 0x80); '+marker)
        for p in (SYSTEM/'rcc').glob('*.rs'):
            text=p.read_text()
            if p.name=='gpu_capture.rs':
                marker='    GPU_CAPTURE_CONTROL[2u16] += 1;'
                if text.count(marker)!=1:raise ValueError('Native fixture capture boundary moved')
                text=text.replace(marker,'    if !profiling_native_tail(cb) { gpu_capture_fail(GPU_CAPTURE_DEPENDENCY); return false; }\n'+marker)+ '\n'+extension
            (source_root/p.name).write_text(text,newline='\n')
    if column:
        generated=output/'profile-rcc';generated.mkdir(parents=True,exist_ok=True)
        for p in source_root.glob('*.rs'):
            source=source_root/'gpu_layout.rs' if p.name=='gpu_layout.rs' else p
            shutil.copy2(source,generated/p.name)
    inputs={str(p.relative_to(SYSTEM)).replace('\\','/'):p for p in (SYSTEM/'rcc').glob('*.rs')}
    for p in (SYSTEM/'build_support').glob('*.rs'):inputs[str(p.relative_to(SYSTEM)).replace('\\','/')]=p
    for p in [SYSTEM/"build.rs",SYSTEM/"Cargo.toml",Path(__file__),SYSTEM/'tools/capture_layout.py',SYSTEM/'tools/gpu_layout.py',SYSTEM/'examples/gpu_renderer/tools/assets.py',*sorted((SYSTEM/'tests').glob('capture_*.rs'))]:
        inputs[str(p.relative_to(SYSTEM)).replace('\\','/')]=p
    for p in assets.iterdir():
        if p.is_file():inputs['assets/'+p.name]=p
    if catalog.get('source_root')=='repository':
        for name,expected in catalog['sources'].items():
            source=(ROOT/name).resolve()
            if not source.is_relative_to(ROOT) or sha(source)!=expected:
                raise ValueError('Catalog source identity mismatch: '+name)
            inputs['repository/'+source.relative_to(ROOT).as_posix()]=source
    if column:
        inputs.update({'producer-rcc/'+p.name:p for p in (output/'profile-rcc').glob('*.rs')})
    inputs['gate.rs']=output/'profiler_gate.rs';inputs['main.rs']=output/'main.rs'
    if rtl is not None:
        rtl = Path(rtl).resolve()
        names = [*json.loads((rtl/'rtl.json').read_text()), 'rtl.json', 'peer/stream-contract.json']
        if (rtl/'layout.json').is_file():names.append('layout.json')
        for name in names:inputs['replay-rtl/'+name] = rtl/name
    files=[]
    for name,path in sorted(inputs.items()):
        dest=output/'identity'/name;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(path,dest)
        files.append(dict(name=name,path=dest.relative_to(output).as_posix(),bytes=dest.stat().st_size,sha256=sha(dest)))
    command_abi=int(re.search(r'^pub const GPU_CAPTURE_RTT400_COMMAND_ABI: u16 = ([0-9]+);', (SYSTEM/'rcc/gpu_capture_abi.rs').read_text(), re.M).group(1))
    value=dict(schema='digitaldesign.gpu-capture-build',version=version,command_abi=command_abi,clock_hz=54000000,mc_clock_hz=108000000,
        mc_profile='stream-cl3-register-return',files=files,
        resources=[{**r,'name':r.get('name',r['file'])} for r in catalog['resources']],
        options=dict(fixture_only=True,capture_allocation_guards=False),toolchain=dict(compiler='repository RCC CPU V3',execution='CpuV3Sim controlled GPU/Display peer'),
        source_commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip())
    value.update(layout=catalog['capture_layout'])
    if catalog.get('capture_scene')=='canonical-full-scene':
        value['options'].update(full_scene=True,scene=catalog['scene'],cpu_capture_gates=3,
                               stimulus='fixed formal camera; neutral analog peer; middle gate resume')
    body={**value,'files':[{k:v for k,v in f.items() if k!='path'} for f in files]}
    value['manifest_id']=hashlib.sha256(json.dumps(body,sort_keys=True,separators=(',',':'),ensure_ascii=True).encode('ascii')).hexdigest()
    (output/'build.json').write_text(json.dumps(value,indent=2)+'\n')
    print(value['manifest_id'])


if __name__=='__main__':
    p=argparse.ArgumentParser(__doc__)
    p.add_argument('--output',required=True,type=Path)
    p.add_argument('--assets',required=True,type=Path)
    p.add_argument('--version',required=True,type=int,choices=[2])
    p.add_argument('--rtl',type=Path)
    p.add_argument('--native-tail',action='store_true',help='Trusted RCC/native encoder fixture, no new driver API')
    p.add_argument('--native-reserved',action='store_true',help='Deliberately invalid descriptor for no-SEND rejection')
    a=p.parse_args()
    if a.native_reserved and not a.native_tail:p.error('--native-reserved requires --native-tail')
    identity(a.output,a.assets,a.version,a.rtl,a.native_tail,a.native_reserved)
