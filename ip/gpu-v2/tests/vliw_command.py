"""Bounded full CP/GPU replay: three rejected triangles, no expensive fill workload."""
from pathlib import Path
import argparse,copy,hashlib,importlib.util,json,os,re,struct,subprocess,sys
ROOT=Path(__file__).resolve().parents[3]
HERE=ROOT/'ip/gpu-v2/examples/vliw_integration'
sys.path.insert(0,str(HERE))
from replay_command import CommandBuffer,texture_word,target_params

def load(name,path):
 spec=importlib.util.spec_from_file_location(name,path);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);return m

def main():
 ap=argparse.ArgumentParser(__doc__);ap.add_argument('--export',type=Path,required=True);ap.add_argument('--output',type=Path,required=True);ap.add_argument('--native',action='store_true');ap.add_argument('--system-leaf',action='store_true');args=ap.parse_args()
 out=args.output.resolve();out.mkdir(parents=True,exist_ok=False)
 O=load('cp32_oracle',HERE/'reference/scene/oracle.py');O=load('cp32_transport',HERE/'transport.py').adapt_oracle(O)
 fixture=json.loads((HERE/'reference/uv-inputs.json').read_text())['scenes'][0]['meshlets'][0]
 mem=[0]*8192
 def put(a,data):
  for j,v in enumerate(data):mem[(a+j)//8]|=v<<(((a+j)%8)*8)
 records=[];vectors=[];uniform=None
 for i in range(3):
  instance=copy.deepcopy(fixture);indices=instance['triangles'][0];instance['vertices']=[copy.deepcopy(instance['vertices'][v]) for v in indices]
  instance.update(slot=i%2,triangles=[[0,1,2]],cull_negative=1,generation=i,seq_base=i)
  meta,_=O.prepare(instance,out/f'trial{i}')
  if meta['packets']:
   instance['triangles']=[[0,2,1]]
  meta,_=O.prepare(instance,out/f'task{i}');assert meta['packets']==0
  image={int(a):int(v,16) for a,v in (s.split() for s in (out/f'task{i}/dma.txt').read_text().splitlines())}
  rows=[a-i%2*128 for a in image if i%2*128<=a<(i%2+1)*128]
  raw=b''.join(struct.pack('<Q',image.get(i%2*128+j,0)) for j in range(max(rows)+1));size=(len(raw)+127)//128*128
  address=0x8000+i*0x500;header=b''.join(struct.pack('<Q',image[256+i%2*4+k]) for k in range(4))
  put(address,header+bytes(96)+raw+bytes(size-len(raw)));records.append((address,size))
  vectors+=(out/f'task{i}/vertices.hex').read_text().splitlines()
  if uniform is None:uniform=b''.join(struct.pack('<Q',image[j]) for j in range(320,336))
 put(0x4000,struct.pack('<16I',199*65536,0,0,0,32|(192<<16),3|(8<<2)|(1<<16),*([0]*10)))
 uniform=bytearray(uniform);struct.pack_into('<II',uniform,88,0xffffffff,0)
 put(0x4280,uniform)
 put(0x4460,target_params(0x100000,0x140000))
 for address,unlit in [(0x4100,True),(0x4140,False)]:
  put(address,struct.pack('<8I',texture_word(0x1000,6,True,2),0,9|(int(unlit)<<5),0,0,0,0,0))
 cb=CommandBuffer();cb.emit('DMA',288,0x4460,32);cb.emit('WAIT',a=3);cb.emit('FLUSH');cb.emit('SET_TARGET',288);cb.emit('DMA',272,0x4000,64);cb.emit('DMA',320,0x4280,128);cb.emit('DMA',280,0x4100,32)
 cb.emit('WAIT',a=3);cb.emit('SET_GLOBAL',272);cb.emit('SET_RENDER',280);cb.emit('DMA_MESHLET',0,*records[0]);cb.emit('WAIT',a=1)
 while len(cb.records)<30:cb.emit('NOP')
 cb.emit('DMA_MESHLET',1,*records[1]);cb.emit('DRAW_MESHLET',0);cb.emit('WAIT',a=1)
 while len(cb.records)<62:cb.emit('NOP')
 cb.emit('DMA_MESHLET',0,*records[2]);cb.emit('DRAW_MESHLET',1);cb.emit('WAIT',a=1);cb.emit('DMA',280,0x4140,32);cb.emit('WAIT',a=3);cb.emit('SET_RENDER',280);cb.emit('DRAW_MESHLET',0)
 cb.emit('WAIT',a=2);cb.emit('FLUSH');cb.emit('END');data=cb.encode();put(0x1000,data+bytes((-len(data))%128))
 (out/'memory.hex').write_text(''.join(f'{v:016x}\n' for v in mem));(out/'vertices.hex').write_text('\n'.join(vectors)+'\n');(out/'commandbuffer.bin').write_bytes(data)
 source=args.export.resolve();rtl=source.read_bytes();
 if args.system_leaf:rtl += b'\n'+(ROOT/'systems/cpu-v3-tang-nano-20k/src/hardware/gpu/gpu.v').read_bytes()
 (out/'dut.v').write_bytes(rtl);sha=hashlib.sha256((out/'dut.v').read_bytes()).hexdigest()
 sys.path.insert(0,str(ROOT/'ip/gpu-v2/examples/postquad_ii4'));from wrapper import wrapper
 _,ports=wrapper(re.sub(r'\b(input|output)(?=\[)',r'\1 ',source.read_text()))
 declarations=[]
 for p in ports['ports']:
  width=f'[{p["bits"]-1}:0]' if p['bits']>1 else ''
  declarations.append(('reg ' if p['direction']=='input' else 'wire ')+width+' '+p['name']+('=0;' if p['direction']=='input' else ';'))
 tb=Path(__file__).with_name('vliw_command_tb.v').read_text().replace('@PORTS@','\n'.join(declarations)).replace('@BYTES@',str(len(data))).replace('@ERROR_ADDRESS@',str((records[1][0]+128)//2))
 from device_bus import bench as device_bench
 tb=device_bench(tb)
 if args.system_leaf:
  links=['.clk(clk)','.reset(reset)']+[f'.{n}({n})' for n in ['device_index','device_channel','device_read_enable','device_write_enable','device_write_data','device_read_data']]
  for prefix in ['ro','fb_r','fb_w']:
   for field in ['request_valid','request_ready','response_valid','response_last','address']:
    links.append(f'.gpu_{prefix}_{field}({prefix}_{field})')
   links += [f'.gpu_{prefix}_line_count_minus_1({prefix}_count)',f'.gpu_{prefix}_error({prefix}_response_error)']
   if prefix=='fb_w':links += ['.gpu_fb_w_write_data(fb_w_data)','.gpu_fb_w_write_data_ready(fb_w_data_ready)']
   else:links += [f'.gpu_{prefix}_read_data({prefix}_data)',f".gpu_{prefix}_write_data_ready(1'b0)"]
  for n in ['meshlet_done','published_vertices','emit_valid','emit_data','quad_admitted','busy','fault']:
   tb=tb.replace('gpu_v2_vliw_command dut(.*);',f'assign {n}=dut.{n};\ngpu_v2_vliw_command dut(.*);')
  tb=tb.replace('dut.', 'dut.core.')
  tb=tb.replace('gpu_v2_vliw_command dut(.*);','CpuV3Gpu dut('+','.join(links)+');')
  tb=tb.replace('ce=!force_ce0&&cycle%7!=2;', 'ce=1;')
  tb=tb.replace('repeat(3)begin tick();if(dut.core.processor.state!=1||!busy)$fatal(1,"CE0 launch was lost");end','')
 (out/'tb.v').write_text(tb)
 cmd=[os.environ['IVERILOG_EXE'],'-g2012','-s','tb','-o',str(out/'sim.vvp'),str(out/'dut.v'),str(out/'tb.v')]
 if args.native:cmd.append(str(Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v'))
 def run(name,cmd,diagnostic=False):
  assert hashlib.sha256((out/'dut.v').read_bytes()).hexdigest()==sha
  with (out/(name+'.log')).open('wb') as log:r=subprocess.run(list(map(str,cmd)),cwd=out,stdout=log,stderr=subprocess.STDOUT,timeout=90)
  text=(out/(name+'.log')).read_text(errors='replace')
  if diagnostic:assert r.returncode!=0 and 'GPU contract:' in text,text[-1800:]
  else:assert r.returncode==0,text[-1800:]
 run('compile',cmd)
 runs=[]
 for mode in ([0,1] if args.system_leaf else [0,1,2]):
  run(f'run-{mode}',[os.environ['VVP_EXE'],out/'sim.vvp',f'+MODE={mode}'],mode==1)
  runs.append('PASS CP32 transport error diagnosed by simulation monitor' if mode==1 else (out/f'run-{mode}.log').read_text().strip())
 # The monitor-free run uses exactly the production datapath. An existing
 # transport error at a real last beat must be observable, without new control.
 release_cmd=cmd.copy();release_cmd.insert(1,'-DSYNTHESIS')
 release_cmd[release_cmd.index('-o')+1]=str(out/'release.vvp')
 run('compile-release',release_cmd)
 run('run-3',[os.environ['VVP_EXE'],out/'release.vvp','+MODE=3'])
 runs.append((out/'run-3.log').read_text().strip())
 receipt=dict(status='pass',native=args.native,system_leaf=args.system_leaf,export_sha256=sha,command_bytes=len(data),cases=runs)
 (out/'receipt.json').write_text(json.dumps(receipt,indent=2));print(json.dumps(receipt))
if __name__=='__main__':main()
