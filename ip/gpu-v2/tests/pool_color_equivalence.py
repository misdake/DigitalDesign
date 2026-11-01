"""Exact complete-factory PP, private-profile scope and frozen edge equivalence."""
import argparse,hashlib,json,os,subprocess
from pathlib import Path
from maintenance_profiles import tokens

PRIVATE=['GPU_V2_VLIW_POOL_COMMAND','GPU_V2_POOL_CLEAR','GPU_V2_POOL_BAD','GPU_V2_VLIW_COLOR_COMMAND','GPU_V2_COLOR_CLEAR','GPU_V2_COLOR_BAD','GPU_V2_COLOR_TERMINAL_CLEAR','GPU_V2_COLOR_TERMINAL_BAD']
def main():
 p=argparse.ArgumentParser(__doc__);p.add_argument('--baseline',type=Path,required=True);p.add_argument('--candidate',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--trace-pair',nargs=2,type=Path,action='append',default=[]);a=p.parse_args();a.output.mkdir(parents=True,exist_ok=False);rows=[]
 for base in sorted(a.baseline.glob('*.v')):
  if base.name.startswith('raw-command'):continue
  for monitors in [True,False]:
   streams=[]
   for name,src in [('baseline',base),('candidate',a.candidate/base.name)]:
    output=a.output/f'{name}-{int(monitors)}-{base.name}';inp=a.output/f'input-{name}-{int(monitors)}-{base.name}'
    suffix='\n'+''.join(f'`ifdef {flag}\nPROFILE_LEAK_{flag}\n`endif\n' for flag in PRIVATE)
    inp.write_text(src.read_text()+suffix)
    z=subprocess.run([os.environ['IVERILOG_EXE'],'-g2012','-E',*([] if monitors else ['-DSYNTHESIS']),'-o',str(output.resolve()),str(inp.resolve())],capture_output=True,timeout=30);assert z.returncode==0,z.stderr.decode(errors='replace');t=tokens(output.read_text());assert not any(x.startswith('PROFILE_LEAK_') for x in t),(src,'profile leak');streams.append(t)
   if streams[0]!=streams[1]:
    i=next((i for i,(x,y) in enumerate(zip(*streams)) if x!=y),min(map(len,streams)));raise AssertionError((base.name,monitors,i,streams[0][i-12:i+24],streams[1][i-12:i+24]))
   rows.append(dict(file=base.name,monitors=monitors,tokens=len(streams[0]),token_sha256=hashlib.sha256(json.dumps(streams[0]).encode()).hexdigest()))
 traces=[]
 for before,after in a.trace_pair:
  expected=sorted(before.rglob('edges.trace'));assert expected,(before,'no traces')
  for src in expected:
   dst=after/src.relative_to(before);assert src.read_bytes()==dst.read_bytes(),('edge difference',src,dst);traces.append(dict(file=str(src.relative_to(before)),sha256=hashlib.sha256(src.read_bytes()).hexdigest()))
 (a.output/'receipt.json').write_text(json.dumps(dict(status='pass',full_pp=rows,traces=traces),indent=2));print(json.dumps(dict(status='pass',full_pp=len(rows),traces=len(traces))))
if __name__=='__main__':main()
