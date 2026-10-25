"""Bounded-F16/narrow-cross tests against an independent integer oracle."""
from pathlib import Path
import argparse, hashlib, importlib.util, json, os, random, re, subprocess, sys
ROOT=Path(__file__).resolve().parents[3]
HERE=ROOT/'ip/gpu-v2/examples/vliw_integration'
sys.dont_write_bytecode=True
def load(name,path):
 s=importlib.util.spec_from_file_location(name,path);m=importlib.util.module_from_spec(s);sys.modules[name]=m;s.loader.exec_module(m);return m
def coefficients(p):
 a,b,c=p
 def cross(u,v):return [u[1]*v[2]-u[2]*v[1],u[2]*v[0]-u[0]*v[2],u[0]*v[1]-u[1]*v[0]]
 raw=[cross(b,c),cross(c,a),cross(a,b)];words=[];n=[];qs=[]
 for row in raw:
  q=max(0,max((v if v>=0 else ~v).bit_length() for v in row)-26)
  nn=[(-1 if v<0 else 1)*(abs(v)>>q) for v in row];n.append(nn);qs.append(q)
  values=[nn[0],nn[1],-399*nn[0]-239*nn[1]+512*nn[2]]
  words.extend([(values[0]&((1<<36)-1))|(q<<48),values[1]&((1<<54)-1),values[2]&((1<<54)-1)])
 delta=sum(x*y for x,y in zip(a,n[0]));ed=max(0,abs(delta).bit_length()-1)
 k=abs(delta)>>(ed-30) if ed>=30 else abs(delta)<<(30-ed)
 return words+[k|((ed+qs[0])<<31)],delta

TB=r'''
`timescale 1ns/1ps
module tb;
`ifndef PORTABLE
GSR GSR(.GSRI(1'b1));
`endif
reg clk=0;always #5 clk=~clk;
reg reset=1,ce=1,start=0,active=0;reg[8:0]pc=128;
wire[71:0]ins;wire[8:0]next_pc,pos_addr;wire pos_re;
wire[3:0]rf_aa,rf_ba,rf_wa,work_ra,work_wa,coefficient_row;
wire mac_constant,mac_clear,acc_capture;
wire rf_we,work_re,work_we,mac_en,acc_we,coefficient_we,publish,done,negative;
wire[31:0]rf_wd,rf_a,rf_b,mac_a;wire[35:0]work_q,work_wd,mac_b;
wire[53:0]mac_c,mac_y,acc_wd,coefficient_data;
reg[31:0]unpacked=0;reg[53:0]acc=0;reg[35:0]pq=0;reg[3:0]work_addr=0;

reg[31:0]positions[0:8];reg[53:0]expected[0:9];reg cull=0;
shared_control_rom #(.SP_COMPACT(1)) rom(clk,reset,ce,active?next_pc:9'd128,ins);
shared_t32_core dut(.*,.mac_c(mac_c),.refs(18'b000010_000001_000000));
shared_rf rf(clk,ce&&rf_we,rf_wa,rf_aa,rf_ba,rf_wd,rf_a,rf_b);
iz_params_async #(.WIDTH(36)) work(clk,reset,ce,ce&&work_we,work_wa,work_wd,work_addr,work_q);
shared_mac mac(clk,reset,ce&&mac_en,mac_constant?mac_a:rf_a,mac_b,mac_c,mac_y);
integer cycles=0,mode=0,fd,rc,i,j,tc=0,got=0,sign_expected,reject_expected;
task check_range_prefix;
 begin
  if(dut.cross_more !== (acc[53:26]!={28{acc[26]}}) ||
     dut.cross_coarse !== (acc[53:29]!={25{acc[29]}}) ||
     dut.det_more !== ((acc[53:31]!={23{acc[31]}})||(acc[31:0]==32'h80000000)) ||
     dut.det_coarse !== (acc[53:34]!={20{acc[34]}}))
   $fatal(1,"shared range prefix changed a signed predicate acc=%h",acc);
  if(!dut.cross_coarse)begin
   if(acc[53:26]=={28{acc[26]}})begin
    if(dut.cross_tail!=0)$fatal(1,"guard-tail shift should be zero");
   end else if(acc[53:27]=={27{acc[27]}})begin
    if(dut.cross_tail!=1)$fatal(1,"guard-tail shift should be one");
   end else if(acc[53:28]=={26{acc[28]}})begin
    if(dut.cross_tail!=2)$fatal(1,"guard-tail shift should be two");
   end else if(dut.cross_tail!=3)$fatal(1,"guard-tail shift should be three");
  end
 end
endtask
integer trace=0;
always @(negedge clk)if(!reset)ce=(mode==0||cycles%7!=2);
always @(posedge clk)begin
 cycles=cycles+1;if(cycles>2000000)$fatal(1,"P32 watchdog");
 if(!reset&&ce)begin
  if(active)pc<=next_pc;
  if(pos_re)pq<={{4{positions[pos_addr[5:0]*3+pos_addr[7:6]][31]}},positions[pos_addr[5:0]*3+pos_addr[7:6]]};
  if(ins[50])unpacked<=pq[31:0];
  if(work_re)work_addr<=work_ra;
  if(acc_capture)acc<=mac_y;if(acc_we)acc<=acc_wd;
  if(trace&&active)$display("TRACE tc%0d pc%0d op%0d edge%0d acc%h q%h wd%h y%h",tc,pc,ins[5:0],0,acc,work_q,work_wd,mac_y);
  if(coefficient_we)begin
   if(coefficient_data!==expected[coefficient_row])$fatal(1,"case%0d row%0d got%h want%h pc%0d",tc,coefficient_row,coefficient_data,expected[coefficient_row],pc);
   got=got+1;
  end
 end
end
initial begin
 rc=$value$plusargs("MODE=%d",mode);rc=$value$plusargs("TRACE=%d",trace);
 fd=$fopen("cases.txt","r");repeat(3)@(negedge clk);reset=0;
 while(!$feof(fd))begin
  rc=$fscanf(fd,"%d %d %d",cull,sign_expected,reject_expected);
  if(rc==3)begin
   for(i=0;i<9;i=i+1)rc=$fscanf(fd,"%h",positions[i]);
   for(i=0;i<10;i=i+1)rc=$fscanf(fd,"%h",expected[i]);
   got=0;pc=128;start=1;@(posedge clk);while(!ce)@(posedge clk);@(negedge clk);start=0;active=1;
   j=cycles;
   @(posedge clk);while(!(ce&&done))@(posedge clk);
   if(!reject_expected&&(got!=10||negative!==sign_expected[0]))$fatal(1,"case%0d outputs%0d sign%0d",tc,got,negative);
   if(reject_expected&&publish)$fatal(1,"rejected triangle published");
   $display("CASE %0d cycles=%0d outputs=%0d",tc,cycles-j,got);
   @(negedge clk);active=0;tc=tc+1;repeat(2)@(negedge clk);
  end
 end
 // Probe every range boundary and randomized 54-bit values against the
 // original predicates. This observes the production combinational circuit.
 for(i=0;i<54;i=i+1)begin
  acc=54'd1<<i;#1;check_range_prefix();
  acc=acc-1'b1;#1;check_range_prefix();
  acc=acc+2'd2;#1;check_range_prefix();
  acc=-acc;#1;check_range_prefix();
  acc=acc+1'b1;#1;check_range_prefix();
  acc=acc+1'b1;#1;check_range_prefix();
 end
 for(i=0;i<4096;i=i+1)begin acc={$urandom,$urandom};#1;check_range_prefix();end
 $display("PASS P32 cases=%0d mode=%0d range_probes=4420",tc,mode);$finish;
end
initial begin #21000000;$fatal(1,"P32 wall timeout");end
endmodule
'''
def run(output,native,export):
 output=Path(output).resolve();output.mkdir(parents=True,exist_ok=False)
 (output/'dut.v').write_bytes(Path(export).read_bytes());(output/'tb.v').write_text(TB)
 rng=random.Random(20261025)
 cases=[[(0,0,65536),(65536,0,65536),(0,65536,65536)],
 [(100*65536,-99*65536,100*65536),(-100*65536,99*65536,99*65536),(99*65536,100*65536,-100*65536)],
 [(-(1<<25),0,1),((1<<25)-1,1,2),(0,-(1<<25),3)]]
 cases += [[tuple(rng.randint(-(1<<25),(1<<25)-1) for _ in range(3)) for _ in range(3)] for _ in range(12)]
 rows=[]
 for exponent in [0,10,11,14,20]:
  power=1<<exponent
  axis=[(power,0,0),(0,power,0),(0,0,power)]
  cases.extend([axis,list(reversed(axis))])
 cases += [[(100*65536,0,64),(100*65536+128,64,128),(100*65536-128,-64,-64)],
 [(346*65536,0,100*65536),(346*65536+64,64,100*65536),(346*65536-64,128,100*65536+64)],
 [(0,0,65536),(65536,0,65536),(2*65536,0,65536)],list(reversed(cases[0]))]
 for index,p in enumerate(cases):
  words,det=coefficients(p)
  cull=int(index==len(cases)-1)
  reject=int(all(v[2]<=0 for v in p) or det==0 or cull and det<0)
  rows.append(str(cull)+' '+str(int(det<0))+f' {reject} '+' '.join(f'{v&0xffffffff:08x}' for row in p for v in row)+' '+' '.join(f'{v:014x}' for v in words))
 (output/'cases.txt').write_text('\n'.join(rows)+'\n')
 cmd=[os.environ.get('IVERILOG_EXE','iverilog'),'-g2012','-D__ICARUS__','-s','tb','-o','sim.vvp']
 if native:cmd += [str(Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v')]
 else:cmd+=['-DPORTABLE']
 cmd+=['dut.v','tb.v']
 for name,command in [('compile',cmd)]+[(f'run{m}',[os.environ.get('VVP_EXE','vvp'),'sim.vvp',f'+MODE={m}']) for m in [0,1]]:
  with (output/f'{name}.log').open('w') as f:r=subprocess.run(command,cwd=output,stdout=f,stderr=subprocess.STDOUT,timeout=90)
  log=(output/f'{name}.log').read_text(errors='replace')
  if r.returncode:raise RuntimeError(log[-2500:])
  print(name,[line for line in log.splitlines() if line.startswith('PASS')] if name!='compile' else 'PASS')
 (output/'review.json').write_text(json.dumps(dict(native=native,cases=len(cases),passed=True,dut_sha256=hashlib.sha256((output/'dut.v').read_bytes()).hexdigest())))
def meshlet(output,native,pose,export,enabled=False,recorded=None):
 import copy
 output=Path(output).resolve();output.mkdir(parents=True,exist_ok=False)
 bounds=load('p32_bounds',ROOT/'ip/gpu-v2/tests/vliw_meshlet_cull.py')
 oracle=load('p32_oracle',HERE/'reference/scene/oracle.py')
 load('p32_transport',HERE/'transport.py').adapt_oracle(oracle)
 cases=copy.deepcopy(json.loads(Path(recorded).read_text())['instances']) if recorded else copy.deepcopy(bounds.fixtures()[:8]) if enabled else [copy.deepcopy(bounds.fixtures()[0])]
 for name,x,z in ([] if recorded else [('positive100',50,100),('negative100',-100,100)]):
  item=copy.deepcopy(cases[0]);item['name']=name;item['config'][:3]=[x*65536,-16384,z*65536];cases.append(item)
 if pose:
  words=json.loads(Path(pose).read_text())['statuses'][-1]['matrix_words']
  uniform=[oracle.F.signed(words[i]|words[i+1]<<16,32) for i in range(0,42,2)]+[65536]+[0]*10
  scene=json.loads((ROOT/'systems/cpu-v3-tang-nano-20k/examples/gpu_demo/assets/scene.json').read_text())
  for source in scene['meshes']:
   if source['name'] not in ['bunny_full_0','bunny_full_16']:continue
   item=copy.deepcopy(source);item.update(uniform=uniform,cull_negative=1);cases.append(item)
 records=[]
 for index,item in enumerate(cases):
  item.update(slot=index%2,seq_base=0,generation=0)
  empty=copy.deepcopy(item);empty['triangles']=[]
  folder=output/f'task{index}';meta,_=oracle.prepare(empty,folder)
  vertices=[int(s,16) for s in (folder/'vertices.hex').read_text().splitlines()]
  vraw=oracle.F.oracle(item['uniform'],item['config'],item['vertices'])
  # Keep all original DMA/index bytes with the reserved metadata bits cleared.
  offset=(3*len(item['vertices'])+1)//2;base=item['slot']*128
  rows=(folder/'dma.txt').read_text().splitlines()
  for j,line in enumerate(rows):
   a,value=line.split()
   if int(a)==256+4*item['slot']:
    word=int(value,16);word=(word&~(127<<8))|(len(item['triangles'])<<8);rows[j]=f'{a} {word:016x}'
  for j in range((len(item['triangles'])+2)//3):
   value=sum(sum(v<<(6*k) for k,v in enumerate(item['triangles'][j*3+l]))<<(18*l) for l in range(3) if j*3+l<len(item['triangles']))
   rows.append(f'{base+offset+j} {value:016x}')
  (folder/'dma.txt').write_text('\n'.join(rows)+'\n')
  descriptors=[];values=[]
  for tid,refs in enumerate(item['triangles']):
   p=[tuple(oracle.F.signed(vraw[6*vi+k],32) for k in range(3)) for vi in refs]
   coeff,delta=coefficients(p)
   if all(v[2]<=0 for v in p) or not delta or delta<0 and item['cull_negative']:continue
   desc=sum(v<<(6*k) for k,v in enumerate(refs))|(int(delta<0)<<18)|(int(item['color_mode'] not in [0,'uv'])<<19)|(int(bool(item['config'][3]&256))<<20)|(tid<<21)
   descriptors.append(desc)
   packet=coeff+[v for vi in refs for v in vertices[5*vi+3:5*vi+5]]
   if 'input[31:0]mesh_color' in Path(export).read_text():
    packet[10]|=((1<<18)-1)<<36;packet[11]|=((1<<14)-1)<<36
   values+=packet
  for name,data,n in [('vertices.hex',vertices,320),('descriptors.hex',descriptors,96),('words.hex',values,1536)]:
   width=14 if name=='words.hex' else 8 if name=='descriptors.hex' else 9
   (folder/name).write_text(''.join(f'{v:0{width}x}\n' for v in data+[0]*(n-len(data))))
  reject=int(bounds.classify(item)!='keep')
  (folder/'counts.txt').write_text(f'{len(item["vertices"])*5} {len(descriptors)} {reject}\n')
  records.append(dict(name=item['name'],triangles=len(item['triangles']),published=len(descriptors)))
 (output/'dut.v').write_bytes(Path(export).read_bytes())
 tb=bounds.TB.replace('@ENABLE@',str(int(enabled))).replace('@TASKS@',str(len(cases))).replace('cycle>200000','cycle>2000000').replace('#2100000','#21000000')
 if 'input[31:0]mesh_color' in Path(export).read_text():
  tb=tb.replace('shared_meshlet #',"reg[31:0]mesh_color=32'hffffffff;reg cp_re=0;reg[8:0]cp_addr=0;wire[63:0]cp_q;wire cp_ready;\nshared_meshlet #")
 tb=tb.replace('dut.core.busy&&dut.core.pc[8]', 'dut.core.busy&&!dut.core.triangle_mode&&dut.core.pc[8]')
 tb=tb.replace('$fatal(1,"task%0d T word%0d",task_id,wi);','$fatal(1,"task%0d T word%0d got%h want%h",task_id,wi,word_data,expected_w[wi]);')
 (output/'tb.v').write_text(tb)
 cmd=[os.environ.get('IVERILOG_EXE','iverilog'),'-g2012','-D__ICARUS__','-s','tb','-o','sim.vvp']
 if native:cmd += [str(Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v')]
 else:cmd+=['-DPORTABLE']
 cmd+=['dut.v','tb.v'];results={}
 for name,command in [('compile',cmd)]+[(f'run{m}',[os.environ.get('VVP_EXE','vvp'),'sim.vvp',f'+MODE={m}']) for m in [0,1]]:
  with (output/f'{name}.log').open('w') as f:r=subprocess.run(command,cwd=output,stdout=f,stderr=subprocess.STDOUT,timeout=120)
  log=(output/f'{name}.log').read_text(errors='replace')
  if r.returncode:raise RuntimeError(log[-2500:])
  results[name]=[s for s in log.splitlines() if s.startswith(('CASE','PASS'))];print(name,[s for s in results[name] if s.startswith('PASS')] or ['PASS'])
 (output/'review.json').write_text(json.dumps(dict(native=native,cases=records,results=results,dut_sha256=hashlib.sha256((output/'dut.v').read_bytes()).hexdigest()),indent=2))
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--output',required=True,type=Path);p.add_argument('--native',action='store_true');p.add_argument('--export',required=True,type=Path);p.add_argument('--pose',type=Path);p.add_argument('--recorded',type=Path);p.add_argument('--meshlet',action='store_true');p.add_argument('--bounds',action='store_true');args=p.parse_args()
 if args.meshlet:meshlet(args.output,args.native,args.pose,args.export,args.bounds,args.recorded)
 else:run(args.output,args.native,args.export)
