"""Bounded 3/1/4 leaf diagnostic checks and an existing active-command replay.

Only diagnostic predicates are injected. Complete legal output traces use the
actual three-draw fixture; no alternative datapath model is introduced.
"""
from pathlib import Path
import argparse,hashlib,json,os,re,subprocess,sys,importlib.util
sys.dont_write_bytecode=True
SITES=[('raster-full','rc_raster','error',2,['tile_publish=1','tile_full=1']),('raster-row','rc_raster','error',4,['scan_read=1','row_issue=1']),('raster-count','rc_raster','error',8,['tile_count=3']),('lod-phase','rc_lod','error',1,['quad_end_valid=1','phase=0']),('compute-phase','rc_compute','compute_error',1,['sample_valid=1','phase=1']),('compute-end','rc_compute','compute_error',8,['quad_end_valid=1','phase=2','valid_pipe=0']),('compute-sign','rc_compute','compute_error',2,['phase=2','valid_pipe=1','sample_cover=1','negative_prefix=1']),('compute-bind','rc_compute','compute_error',4,['phase=0','sample_valid=1','cfg_valid=1'])]
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def ports(source,module):
 h=re.search(r'module '+module+r'(?:\s*#\s*\([^)]*\))?\s*\((.*?)\);',source,re.S)[1];h=re.sub(r'\(\*.*?\*\)','',h,flags=re.S);out=[]
 for m in re.finditer(r'\b(input|output)\b(.*?)(?=\b(?:input|output|inout)\b|$)',h,re.S):
  body=re.sub(r'\[[^]]*\]',' ',m[2]);body=re.sub(r'\b(?:wire|reg|signed)\b','',body)
  out.extend((m[1],n)for n in re.findall(r'\b[A-Za-z_]\w*\b',body))
 return out
def bench(src,site,command):
 name,module,reg,bit,forces=site;ps=ports(src,module);pins=','.join('.'+n+'('+({'clk':'clk','reset':'reset','ce':'ce'}.get(n,"'0"))+')'for direction,n in ps if direction=='input');outputs=','.join('dut.'+n for direction,n in ps if direction=='output');force=''.join('force dut.'+x+';'for x in forces);release=''.join('release dut.'+x.split('=')[0]+';'for x in forces)
 zero=('force dut.'+reg+"=4'h9;#1;release dut."+reg+";#1;if(dut."+reg+"!==4'h9)$fatal(1,\"asynchronous zero substitution\");@(posedge clk);#1;if(dut."+reg+"!==0)$fatal(1,\"CE0 zero missing\");@(negedge clk);reset=1;repeat(2)@(negedge clk);reset=0;"if command else'')
 expected=0 if command else bit
 return '''`timescale 1ns/1ps
module tb;reg clk=0,reset=1,ce=0;always #5 clk=~clk;integer edges=0;GSR GSR(.GSRI(1'b1));
'''+module+' dut('+pins+''');
always@(posedge clk)begin edges=edges+1;if(edges>40)$fatal(1,"edge bound");#1;if(!reset)$display("TRACE %0d %h",edges,{'''+outputs+'''});end
initial begin repeat(3)@(negedge clk);reset=0;
'''+zero+'''repeat(2)@(negedge clk);ce=1;'''+force+'''
$display("TARGET edge=%0d time=%0t",edges+1,$time+5);
@(posedge clk);#1;
if(dut.'''+reg+'''!==4'd'''+str(expected)+''')$fatal(1,"diagnostic result");
@(negedge clk);ce=0;'''+release+'''
repeat(3)begin @(posedge clk);#1;if(dut.'''+reg+'''!==4'd'''+str(expected)+''')$fatal(1,"CE0 diagnostic hold/clear");end
$display("PASS site='''+name+''' command='''+str(int(command))+''' edges=%0d",edges);$finish;end
initial begin #500;$fatal(1,"wall bound");end endmodule
'''
def flow_cases(exports,out):
 here=Path(__file__).resolve().parent
 sys.path.insert(0,str(here))
 spec=importlib.util.spec_from_file_location('rb06a_flow',here/'current_texture_command.py');child=importlib.util.module_from_spec(spec);spec.loader.exec_module(child)
 original=child.TB;rows=[]
 for backend in ['portable','native']:
  source=(exports/('command-'+backend+'.v')).resolve();identity=sha(source);text=source.read_text();fields=[]
  for module,path in [('rc_raster','dut.core.front.raster'),('rc_compute','dut.core.front.interpolator.compute'),('rc_lod','dut.core.front.interpolator.compute.lod')]:
   fields.extend(path+'.'+n for direction,n in ports(text,module)if direction=='output')
  fields=['ce']+fields
  fmt=' '.join(['%h']*len(fields));signals=','.join(fields)
  observer='integer leaf_fd=0,leaf_edge=0;\nalways@(posedge clk)begin\n leaf_edge=leaf_edge+1;\n #0;if(!reset&&mode==0)begin if(leaf_fd==0)leaf_fd=$fopen("leaf.trace","w");$fdisplay(leaf_fd,"pre %0d '+fmt+'",leaf_edge,'+signals+');end\n #1;if(!reset&&mode==0)begin $fdisplay(leaf_fd,"post %0d '+fmt+'",leaf_edge,'+signals+');if(dut.core.front.raster.error!==0||dut.core.front.interpolator.compute.compute_error!==0||dut.core.front.interpolator.compute.lod.error!==0)$fatal(1,"legal command diagnostic");end\nend\n'
  assert original.count('endmodule')==1
  child.TB=original.replace('endmodule',observer+'endmodule')
  for off in [False,True]:
   d=(out/(backend+('-off'if off else'-on'))).resolve();saved=sys.argv;sys.argv=['current_texture_command.py','--export',str(source),'--output',str(d),'--native','--physical']+(['--no-monitors']if off else[])
   try:child.main()
   finally:sys.argv=saved
   assert sha(source)==identity
   trace=(d/'leaf.trace').read_bytes();rows.append({'backend':backend,'monitors_off':off,'source_sha256':identity,'fields':fields,'trace_sha256':hashlib.sha256(trace).hexdigest(),'top_trace_sha256':sha(d/'valid.trace'),'rows':len(trace.splitlines())})
  assert rows[-1]['trace_sha256']==rows[-2]['trace_sha256'] and rows[-1]['top_trace_sha256']==rows[-2]['top_trace_sha256']
 (out/'receipt.json').write_text(json.dumps({'rows':rows,'scope':'Existing three-draw actual CP/geometry/R/I/LOD/Sampling/ROP/cache fixture; all leaf outputs + CE sampled pre/post, on/off; existing finite bounds.'},indent=2))

def main():
 ap=argparse.ArgumentParser();ap.add_argument('--exports',type=Path,required=True);ap.add_argument('--output',type=Path,required=True);ap.add_argument('--flow-only',action='store_true');a=ap.parse_args();out=a.output.resolve();out.mkdir(exist_ok=False,parents=True);rows=[]
 if a.flow_only:flow_cases(a.exports,out);return
 for backend in ['portable','native']:
  for site in SITES:
   for mode in ['command-on','command-off','generic']:
    command=mode.startswith('command');source=(a.exports/(('command-'if command else'generic-')+backend+'.v')).resolve();identity=sha(source);d=out/(backend+'-'+site[0]+'-'+mode);d.mkdir();(d/'tb.v').write_text(bench(source.read_text(),site,command));cmd=[os.environ['IVERILOG_EXE'],'-g2012','-s','tb','-o','sim.vvp',str(source),'tb.v',str(Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v')]
    if mode!='command-on':cmd.insert(1,'-DSYNTHESIS')
    for label,c in [('compile',cmd),('run',[os.environ['VVP_EXE'],'sim.vvp'])]:
     with(d/(label+'.log')).open('wb')as f:p=subprocess.run(c,cwd=d,stdout=f,stderr=subprocess.STDOUT,timeout=30)
     log=(d/(label+'.log')).read_text(errors='replace')
     if label=='compile':assert p.returncode==0,log[-1500:]
    assert sha(source)==identity
    target=re.search(r'TARGET edge=(\d+) time=(\d+)',log);assert target,(d,log[-1200:])
    if mode=='command-on':assert p.returncode!=0 and'GPU contract: '+site[2]in log and re.search(r'Time:\s*'+target[2]+r'\b',log),(d,log[-1800:])
    else:assert p.returncode==0 and'PASS site='in log,(d,log[-1800:])
    trace='\n'.join(l for l in log.splitlines()if l.startswith(('TRACE ','TARGET ','PASS site=')))+'\n';rows.append({'site':site[0],'module':site[1],'diagnostic':site[2],'bit_value':site[3],'backend':backend,'mode':mode,'source_sha256':identity,'tb_sha256':sha(d/'tb.v'),'expected_edge':int(target[1]),'expected_time':int(target[2]),'fatal_same_edge':mode=='command-on','trace_sha256':hashlib.sha256(trace.encode()).hexdigest()});print('PASS',backend,site[0],mode,flush=True)
 (out/'receipt.json').write_text(json.dumps({'rows':rows,'limits':{'edges':40,'wall_ns':500,'process_seconds':30},'injection':'Directed leaf predicate force; checked generic sticky bits vs command synchronous-zero register; active legal datapath verified separately.'},indent=2))
if __name__=='__main__':main()
