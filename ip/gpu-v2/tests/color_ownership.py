"""Bounded actual selected Color/public-ID/Sample-bank and DUT mutation tests."""
import argparse,hashlib,json,os,re,subprocess
from pathlib import Path
from pool_policy import execute
HERE=Path(__file__).resolve().parent

def main():
 p=argparse.ArgumentParser(__doc__);p.add_argument('--exports',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--mutations',action='store_true');a=p.parse_args();out=a.output.resolve();out.mkdir(parents=True,exist_ok=False);rows=[];sources={}
 tb=(HERE/'color_ownership_tb.v').read_text()
 for backend in ['native','portable']:
  for profile in ['backend','command','raw-command','public-data']:
   command=profile in ['command','raw-command']
   name=f'{profile}-{backend}';src=(a.exports/(name+'.v')).resolve()
   if profile=='public-data':
    src=out/f'{name}.input.v';src.write_text('`define GPU_V2_COMMAND_PORTS\n`define GPU_V2_LIVE_GLOBALS\n'+(a.exports/f'backend-{backend}.v').read_text())
   raw=src.read_text();assert 'assign sample_result=sfire;' in raw and 'wire[6:0]sample_address={public_upper[sk[5:2]],sk};' in raw
   assert '.write_en(sample_result)' in raw and '.write_address(sample_address)' in raw and '.result_fire(color_result)' in raw
   for monitors in [True,False]:
    pp=out/f'{name}-{int(monitors)}.pp.v';z=subprocess.run([os.environ['IVERILOG_EXE'],'-g2012','-E',*([] if monitors else ['-DSYNTHESIS']),'-o',str(pp),str(src)],capture_output=True,timeout=30);assert z.returncode==0,z.stderr.decode(errors='replace')
    source=pp.read_text();sources[name+str(monitors)]=source
    for kind in range(4):
     # Plusarg is embedded, leaving simulator/process limits in the shared runner.
     body=tb.replace('kind=0;','kind='+str(kind)+';');body=('`define COMMAND\n' if command else '')+('`define PHYSICAL_GROUPS\n' if profile=='command' else '')+body
     d=out/f'{name}-{int(monitors)}-{kind}';r=execute(d,source,body,backend=='native',monitors,monitors and (kind==3 or (command and (kind==1 or (kind==2 and profile!='command')))),'closed Color result bank did not accept completion' if kind==3 else 'GPU contract: preflight_fault');rows.append(dict(case=d.name,**r))
 mutations=[]
 if a.mutations:
  source=sources['raw-command-portableFalse']
  for name,old,new,kind in [('release','if(result_fire && quad_last)','if(result_fire)',0),('packet',"wire packet_bad=accepted&&(in_payload[7:4]>4'd10);","wire packet_bad=1'b0;",2)]:
   assert source.count(old)==1,(name,source.count(old));mutant=source.replace(old,new);body='`define COMMAND\n'+tb.replace('kind=0;','kind='+str(kind)+';');d=out/f'mutant-{name}';r=execute(d,mutant,body,False,False,True,'Color premature public ID release' if name=='release' else 'Color packet functional gate removed');mutations.append(dict(case=name,**r))
 (out/'receipt.json').write_text(json.dumps(dict(status='pass',cases=rows,mutations=mutations,test_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest()),indent=2));print(json.dumps(dict(status='pass',cases=len(rows),mutations=len(mutations))))
if __name__=='__main__':main()
