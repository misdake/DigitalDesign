"""Bounded cache clean/invalidate through actual arbiter/stream/native pins."""
from pathlib import Path
import sys,os,re,json,subprocess,hashlib
sys.dont_write_bytecode=True
root=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(root/'hardware/vendor/gowin/tests'))
from cl3_pins import with_cl3_pins,cl3_bridge_parameters
out=Path(sys.argv[1]).resolve()
baseline_ref=sys.argv[sys.argv.index('--baseline-ref')+1] if '--baseline-ref' in sys.argv else None
source=(out/'combination.v').read_text();ports=[]
data={'data_request_valid','data_write','data_line','data_address','data_write_data','data_response_ready'}
for d,w,n in re.findall(r'\b(input|output|inout)\s+(?:wire\s+)?(\[[^]]+\]\s*)?(\w+)',source.split(');',1)[0]):
 ports.append(('reg ' if d=='input' and n not in data else 'wire ')+w+n+('=0;' if d=='input' and n not in data else ';'))
s=(Path(__file__).with_name('dcache_continuation_tb.v')).read_text().replace('@PORTS@','\n'.join(ports))
s=with_cl3_pins(s,'dut.bridge.u_controller')
(out/'combination.v').write_text(cl3_bridge_parameters(source))
(out/'tb.v').write_text(s)
files=[out/n for n in ['bridge.v','combination.v','controller.v','cpu_v3_memory_arbiter.v','pins.v','shared_port.v','tb.v']]+list(out.glob('cache_*.v'))
p=subprocess.run([os.environ['IVERILOG_EXE'],'-g2012','-D__ICARUS__','-s','tb','-o',str(out/'test.vvp'),str(Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v'),*map(str,files)],capture_output=True,timeout=90)
(out/'compile.log').write_bytes(p.stdout+p.stderr);assert p.returncode==0,p.stderr.decode()[-2500:]
passed=[]
for name,args in [('clean',['+MODE=0']),('invalidate',['+MODE=1']),('shuffle',['+MODE=0','+SHUFFLE=1']),('sparse',['+MODE=1','+LINES=8','+STRIDE=8']),('error',['+ERROR=1']),('alias-direction',['+GPUCONTEND=1'])]:
 p=subprocess.run([os.environ['VVP_EXE'],str(out/'test.vvp'),*args],capture_output=True,timeout=90)
 text=p.stdout.decode()+p.stderr.decode();(out/f'{name}.log').write_text(text);print(text[-1300:]);assert p.returncode==0,name
 passed +=[l for l in text.splitlines() if l.startswith('DCACHE_NATIVE_')]
# Replay the same pin-level service with official data and metadata DPBs;
# portable memories alone cannot establish Gowin's write/DO-hold behavior.
data_file=next(p for p in files if p.name.startswith('cache_') and 'reg [15:0] bank_0_memory' in p.read_text())
vendor_data=out/'vendor-data.v'
vendor_data.write_text(data_file.read_text().split('reg [15:0] bank_0_memory',1)[0]+(root/'ip/cpu-v3/src/hardware/cache/cpu_v3_data_cache_vendor_data_body.vh').read_text())
vendor_files=[vendor_data if p==data_file else p for p in files]
p=subprocess.run([os.environ['IVERILOG_EXE'],'-g2012','-D__ICARUS__','-DCPU_V3_CACHE_METADATA_VENDOR','-s','tb','-o',str(out/'vendor.vvp'),str(Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v'),*map(str,vendor_files)],capture_output=True,timeout=90)
(out/'vendor-compile.log').write_bytes(p.stdout+p.stderr);assert p.returncode==0,p.stderr.decode()[-2500:]
for name,args in [('clean',['+MODE=0']),('invalidate',['+MODE=1']),('shuffle',['+MODE=0','+SHUFFLE=1']),('sparse',['+MODE=1','+LINES=8','+STRIDE=8']),('error',['+ERROR=1']),('alias-direction',['+GPUCONTEND=1'])]:
 p=subprocess.run([os.environ['VVP_EXE'],str(out/'vendor.vvp'),*args],capture_output=True,timeout=90)
 text=p.stdout.decode()+p.stderr.decode();(out/f'vendor-{name}.log').write_text(text);print('VENDOR',text[-1300:]);assert p.returncode==0,name
 passed +=['VENDOR '+l for l in text.splitlines() if l.startswith('DCACHE_NATIVE_')]
if baseline_ref:
 # Same native timing, Display competition and data oracle, with the actual
 # committed single-context cache as the performance baseline.
 cache=next(p for p in files if p.name.startswith('cache_') and 'module CpuV3DataCache (' in p.read_text())
 current=cache.read_text()
 banks=re.search(r'(\w+) u_data_banks',current)[1];metadata=re.search(r'(\w+) u_metadata',current)[1]
 old=subprocess.run(['git','show',baseline_ref+':ip/cpu-v3/src/hardware/cache/cpu_v3_data_cache.v'],cwd=root,capture_output=True,check=True,text=True).stdout
 baseline=out/'baseline-cache.v';baseline.write_text(old.replace('__CACHE_DATA_BANKS__',banks).replace('__CACHE_METADATA__',metadata))
 baseline_files=[baseline if p==cache else p for p in files]
 p=subprocess.run([os.environ['IVERILOG_EXE'],'-g2012','-D__ICARUS__','-s','tb','-o',str(out/'baseline.vvp'),str(Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v'),*map(str,baseline_files)],capture_output=True,timeout=90)
 (out/'baseline-compile.log').write_bytes(p.stdout+p.stderr);assert p.returncode==0,p.stderr.decode()[-2500:]
 for name,args in [('clean',['+MODE=0']),('invalidate',['+MODE=1']),('sparse',['+MODE=1','+LINES=8','+STRIDE=8'])]:
  p=subprocess.run([os.environ['VVP_EXE'],str(out/'baseline.vvp'),'+BASELINE=1',*args],capture_output=True,timeout=90)
  text=p.stdout.decode()+p.stderr.decode();(out/f'baseline-{name}.log').write_text(text);print(text[-1300:]);assert p.returncode==0,name
  passed +=[l for l in text.splitlines() if l.startswith('DCACHE_NATIVE_')]
(out/'receipt.json').write_text(json.dumps(dict(status='pass',baseline_ref=baseline_ref,baseline_cache_sha256=hashlib.sha256(baseline.read_bytes()).hexdigest() if baseline_ref else None,summary=passed,sources={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in files+[vendor_data]}),indent=2))
