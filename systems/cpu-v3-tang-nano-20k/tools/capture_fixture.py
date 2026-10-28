"""Materialize portable identities for bounded CPU producer fixtures, never board receipts."""
from pathlib import Path
import argparse
import hashlib
import json
import shutil
import subprocess

ROOT=Path(__file__).resolve().parents[3]
SYSTEM=ROOT/'systems/cpu-v3-tang-nano-20k'


def sha(path): return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def identity(output, assets, version):
    output=Path(output).resolve();assets=Path(assets).resolve()
    catalog=json.loads((assets/'assets.json').read_text())
    column=version==2 and catalog["capture_layout"]["abi"]==2
    if column:
        generated=output/'profile-rcc';generated.mkdir(parents=True,exist_ok=True)
        for p in (SYSTEM/'rcc').glob('*.rs'):
            source=SYSTEM/'rcc/gpu_layout_column16.rs' if p.name=='gpu_layout.rs' else p
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
    files=[]
    for name,path in sorted(inputs.items()):
        dest=output/'identity'/name;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(path,dest)
        files.append(dict(name=name,path=dest.relative_to(output).as_posix(),bytes=dest.stat().st_size,sha256=sha(dest)))
    value=dict(schema='digitaldesign.gpu-capture-build',version=version,clock_hz=54000000,mc_clock_hz=108000000,
        mc_profile='stream-cl3-register-return',files=files,resources=catalog['resources'],
        options=dict(fixture_only=True,capture_allocation_guards=False),toolchain=dict(compiler='repository RCC CPU V3',execution='CpuV3Sim controlled GPU/Display peer'),
        source_commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip())
    if version==2:value.update(layout=catalog['capture_layout'],command_abi=1)
    if catalog.get('capture_scene')=='canonical-full-scene':
        value['options'].update(full_scene=True,scene=catalog['scene'],cpu_capture_gates=3,
                               stimulus='fixed formal camera; neutral analog peer; middle gate resume')
    body={**value,'files':[{k:v for k,v in f.items() if k!='path'} for f in files]}
    value['manifest_id']=hashlib.sha256(json.dumps(body,sort_keys=True,separators=(',',':'),ensure_ascii=True).encode('ascii')).hexdigest()
    (output/'build.json').write_text(json.dumps(value,indent=2)+'\n')
    print(value['manifest_id'])


if __name__=='__main__':
    p=argparse.ArgumentParser(__doc__);p.add_argument('--output',required=True,type=Path);p.add_argument('--assets',required=True,type=Path);p.add_argument('--version',required=True,type=int,choices=[1,2]);a=p.parse_args();identity(a.output,a.assets,a.version)
