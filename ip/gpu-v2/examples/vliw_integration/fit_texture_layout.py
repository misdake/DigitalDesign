"""Matched54MHz registered cache-leaf fit; excludes packet producer and whole GPU."""
from pathlib import Path
import argparse
import hashlib
import importlib.util
import json
import os
import re
import sys

sys.dont_write_bytecode=True


def load(name,path):
    spec=importlib.util.spec_from_file_location(name,path)
    module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module);return module


def main():
    ap=argparse.ArgumentParser(__doc__)
    ap.add_argument('--baseline',type=Path,required=True)
    ap.add_argument('--candidate',type=Path,required=True)
    ap.add_argument('--output',type=Path,required=True)
    args=ap.parse_args();out=args.output.resolve();assert not out.exists();out.mkdir(parents=True)
    support=Path(__file__).resolve().parents[1]/'postquad_ii4'
    helper=load('texture_fit_helper',support/'fit_helper.py')
    wrapper=load('texture_fit_wrapper',support/'wrapper.py')
    helper.OUT=out/'fits';helper.SOURCE=out/'source';helper.wrapper=wrapper.wrapper
    helper.GW=os.environ.get('GW_SH_EXE') or str(Path(os.environ['GOWIN_HOME'])/'IDE/bin/gw_sh.exe')
    identities={};boundaries=[]
    for name,path in [('baseline',args.baseline),('candidate',args.candidate)]:
        text=path.read_text()
        leaf=re.search(r'\bmodule gpu_v2_texture_runtime_cache_unique\b.*?endmodule',text,re.S)[0]
        leaf=leaf.replace('module gpu_v2_texture_runtime_cache_unique # (parameter NATIVE=1)(', 'module probe(')
        leaf=re.sub(r'\bproposal_ce\b','ce',leaf)
        # Keep identical external next-key width and wrapper FF boundary. The
        # candidate consumes only its canonical15-bit line address internally.
        leaf=leaf.replace('input wire next_valid,input wire[14:0]next_key,','input wire next_valid,input wire[15:0]next_key,')
        leaf=leaf.replace(');',');\nlocalparam NATIVE=1;',1)
        leaf=re.sub(r'\b(input|output)(?=\[)',r'\1 ',leaf)
        directory=helper.SOURCE/name;directory.mkdir(parents=True)
        (directory/'native.v').write_text(leaf)
        _,boundary=wrapper.wrapper(leaf);boundaries.append(boundary)
        identities[name]=dict(export_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),
                              leaf_sha256=hashlib.sha256(leaf.encode()).hexdigest())
    assert boundaries[0]==boundaries[1],boundaries
    results=[]
    for name in ('baseline','candidate'):
        result=helper.run((name,'native',0,54))
        report=helper.OUT/f'{name}-native-p0-m54'/'impl/pnr/leaf.rpt.txt'
        if report.exists():
            text=report.read_text(errors='replace')
            for key,pattern in [('bsram',r'^\s*BSRAM\s*\|\s*([\d.]+)'),('cls',r'^\s*CLS\s*\|\s*(\d+)')]:
                match=re.search(pattern,text,re.M);result[key]=float(match[1]) if match else None
        results.append(result)
    receipt=dict(identities=identities,results=results,boundary=boundaries[0],
                 scope='Cache leaf only with identical registered external ports, native RAM, same device/placement0/54MHz. Includes wrapper FF; excludes packet/membership producer, full GPU/CPU/Display and board timing.')
    (out/'comparison.json').write_text(json.dumps(receipt,indent=2))
    print(json.dumps(receipt,indent=2))


if __name__=='__main__':main()
