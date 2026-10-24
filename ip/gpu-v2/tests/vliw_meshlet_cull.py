"""Bounded current-ROM/real-MAC replay, with an independent eight-corner oracle.

Compare enabled/disabled culling on identical inputs, including retained V/T
words, rejected raster coverage and the cost paid by visible meshlets.
"""
from pathlib import Path
import argparse
import copy
import hashlib
import importlib.util
import json
import os
import subprocess
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[3]
HERE = ROOT/'ip/gpu-v2/examples/vliw_integration'


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def classify(instance):
    o = instance['config'][:3]
    shift = instance['config'][3] & 31
    m = instance['uniform']
    if shift > 14 or any(not -(1 << 24) <= x < 1 << 24 for x in o):
        return 'keep'
    def eligible(row):
        return all(-(1 << 19) <= x < 1 << 19 for x in m[row:row+3]) and -(1 << 29) <= m[row+3] < 1 << 29
    # Enumerate all corners instead of using the hardware supporting-corner rule.
    points = [[o[k]+((1023 << shift) if corner & (1 << k) else 0) for k in range(3)] for corner in range(8)]
    def values(row):
        return [sum(p[k]*m[row+k] for k in range(3))+(m[row+3] << 16) for p in points]
    if not eligible(8): return 'keep'
    z = max(values(8)) >> 16
    if z <= 0: return 'behind'
    if not eligible(0): return 'keep'
    if max(values(0))+(z << 16) < -(1 << 32): return 'left'
    if min(values(0))-(z << 16) > 1 << 32: return 'right'
    return 'keep'


TB = r'''
`timescale 1ns/1ps
module tb;
`ifndef PORTABLE
GSR GSR(.GSRI(1'b1));
`endif
reg clk=0;always #5 clk=~clk;
reg reset=1,ce=1,start=0,slot=0,dma_we=0,output_ready=1,packet_ready=1,word_ready=1;
reg[9:0]dma_addr=0;reg[63:0]dma_data=0;
wire ready,busy,done,drained,dma_ready,packet_valid,word_valid,emit_valid;
wire[3:0]error;wire[6:0]published_vertices;wire[35:0]descriptor,emit_data;
wire[4:0]word_index;wire[53:0]word_data;wire[9:0]emit_addr;
shared_meshlet #(.BOUNDS_CULL(@ENABLE@)) dut(.*);
reg[35:0]expected_v[0:319],expected_d[0:95];reg[53:0]expected_w[0:1535];
integer cycle=0,task_id=0,fd,rc,a,nv,nt,di=0,vi=0,pi=0,wi=0,reject,first,issue,enabled=0,bounds_cycles=0;
integer mode=0;reg[63:0]value;reg[1023:0]path;
always @(negedge clk)if(!reset)begin
 ce=mode==0||cycle%7!=2;output_ready=mode==0||cycle%11!=3;
 packet_ready=mode==0||cycle%9!=3;word_ready=mode==0||cycle%13!=5;
end
always @(posedge clk)begin
 cycle=cycle+1;if(cycle>200000)$fatal(1,"meshlet bounds watchdog");
 if(!reset&&ce)begin
  if(error)$fatal(1,"meshlet%0d error%0d",task_id,error);
  if(dut.core.busy&&dut.core.bounds_run)bounds_cycles=bounds_cycles+1;
  if(busy)enabled=enabled+1;
  if(emit_valid&&output_ready)begin
   if(vi>=nv||emit_addr!==vi||emit_data!==expected_v[vi])$fatal(1,"task%0d V%0d got%h want%h",task_id,vi,emit_data,expected_v[vi]);
   vi=vi+1;
  end
  if(packet_valid&&packet_ready)begin
   if(pi>=nt||descriptor!==expected_d[pi])$fatal(1,"task%0d descriptor%0d",task_id,pi);
   pi=pi+1;
  end
  if(word_valid&&word_ready)begin
   if(wi>=nt*16||word_index!==wi%16||word_data!==expected_w[wi])$fatal(1,"task%0d T word%0d",task_id,wi);
   wi=wi+1;
  end
 end
end
task tick;begin @(posedge clk);#1;end endtask
initial begin
 rc=$value$plusargs("MODE=%d",mode);
 tick();tick();@(negedge clk);reset=0;
 for(task_id=0;task_id<@TASKS@;task_id=task_id+1)begin
  while(!drained)tick();
  vi=0;pi=0;wi=0;enabled=0;bounds_cycles=0;
  $sformat(path,"task%0d/counts.txt",task_id);fd=$fopen(path,"r");rc=$fscanf(fd,"%d %d %d",nv,nt,reject);$fclose(fd);
  if(@ENABLE@&&reject!=0)begin nv=0;nt=0;end
  $sformat(path,"task%0d/vertices.hex",task_id);$readmemh(path,expected_v);
  $sformat(path,"task%0d/descriptors.hex",task_id);$readmemh(path,expected_d);
  $sformat(path,"task%0d/words.hex",task_id);$readmemh(path,expected_w);
  slot=task_id%2;
  $sformat(path,"task%0d/dma.txt",task_id);fd=$fopen(path,"r");
  while(!$feof(fd))begin
   rc=$fscanf(fd,"%d %h",a,value);
   if(rc==2)begin
    @(negedge clk);dma_addr=a;dma_data=value;dma_we=1;
    tick();while(!dma_ready)tick();@(negedge clk);dma_we=0;
   end
  end
  $fclose(fd);while(!ready)tick();@(negedge clk);start=1;issue=cycle;
  tick();while(!ce)tick();@(negedge clk);start=0;
  while(!done)tick();first=cycle;
  while(!drained)tick();
  if(vi!=nv||pi!=nt||wi!=nt*16)$fatal(1,"task%0d counts V%0d/%0d T%0d/%0d W%0d",task_id,vi,nv,pi,nt,wi);
  if(@ENABLE@&&dut.core.bounds_reject!==(reject!=0))$fatal(1,"task%0d reject%0d",task_id,reject);
  $display("CASE %0d issue_to_done=%0d total=%0d enabled=%0d bounds=%0d V=%0d T=%0d reject=%0d",task_id,first-issue,cycle-issue,enabled,bounds_cycles,vi,pi,@ENABLE@&&reject);
  @(negedge clk);
 end
 $display("PASS BOUNDS enabled=@ENABLE@ mode=%0d tasks=@TASKS@",mode);$finish;
end
initial begin #2100000;$fatal(1,"wall timeout");end
endmodule
'''


def fixtures():
    scenes = json.loads((HERE/'reference/uv-inputs.json').read_text())['scenes'][0]['meshlets']
    scene = json.loads((ROOT/'systems/cpu-v3-tang-nano-20k/examples/gpu_demo/assets/scene.json').read_text())
    micro = dict(name='micro',group='micro',uniform=[65536,0,0,0,0,65536,0,0,0,0,65536,0,
        65536,0,0,0,65536,0,0,0,65536,65536]+[0]*10,
        vertices=[[0,511 << 20,0x123456],[100,511 << 20,0x234567],[100 << 10,511 << 20,0x345678]],
        triangles=[[0,1,2]],config=[-16384,-16384,65536,256|6],scale_s=4,color_mode=1,cull_negative=0)
    result=[]
    for label,x,z in [('visible',0,0),('behind',0,-4),('left',-8,0),('right',8,0),('margin',-1,0),('cross-z',0,-1)]:
        item=copy.deepcopy(micro);item['name']=label;item['uniform'][3]=x*65536;item['uniform'][11]=z*65536;result.append(item)
    rotated=copy.deepcopy(micro);rotated['name']='yaw-pitch';rotated['uniform'][:12]=[46341,0,-46341,0,23170,56756,23170,0,40132,-32768,40132,65536];result.append(rotated)
    cancelled=copy.deepcopy(micro);cancelled['name']='translated-cancellation'
    cancelled['config'][0]-=2*65536;cancelled['uniform'][3]=2*65536;result.append(cancelled)
    for label in ['shift-fallback','origin-fallback','matrix-fallback']:
        item=copy.deepcopy(micro);item['name']=label;item['triangles']=[]
        if label=='shift-fallback':item['config'][3]=256|15
        if label=='origin-fallback':item['config'][0]=256*65536
        if label=='matrix-fallback':item['uniform'][0]=8*65536
        result.append(item)
    for source in (item for item in scenes if item['group'] == 'bunny'):
        for label,x,z in [('front',0,0),('behind',0,-20),('left',-20,0),('right',20,0)]:
            item=copy.deepcopy(source);item['name']=source['group']+'-'+label
            item['uniform'][3]+=x*65536;item['uniform'][11]+=z*65536;item['scale_s']=5
            result.append(item)
    for source in scene['meshes'][:2]:
        assert source['group'] == 'box' and len(source['vertices']) == 48 and len(source['triangles']) == 54
        for label, z in [('front', 0), ('behind', -20)]:
            item=copy.deepcopy(source);item['name']=source['name']+'-'+label
            item['uniform']=scene['cameras'][0]['matrix']+[65536]+[0]*10
            item.update(cull_negative=1,scale_s=5)
            item['uniform'][11]+=z*65536
            result.append(item)
    return result


def main():
    parser=argparse.ArgumentParser(__doc__)
    parser.add_argument('--export',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--native',action='store_true')
    args=parser.parse_args();out=args.output.resolve();out.mkdir(parents=True,exist_ok=False)
    oracle=load('bounds_scene_oracle',HERE/'reference/scene/oracle.py')
    oracle=load('bounds_transport',HERE/'transport.py').adapt_oracle(oracle)
    cases=[]
    for i,item in enumerate(fixtures()):
        item['slot']=i%2;item['seq_base']=0;item['generation']=0
        folder=out/f'task{i}';meta,packets=oracle.prepare(item,folder)
        decision=classify(item)
        if decision!='keep':assert meta['coverage_pixels']==0,(item['name'],decision,meta)
        (folder/'counts.txt').write_text(f'{meta["vertices"]*5} {meta["packets"]} {int(decision!="keep")}\n')
        cases.append(dict(name=item['name'],decision=decision,vertices=meta['vertices'],triangles=meta['triangles'],pixels=meta['coverage_pixels']))
    source=args.export.read_bytes();(out/'dut.v').write_bytes(source)
    logs={}
    for enable in (0,1):
        (out/f'tb{enable}.v').write_text(TB.replace('@ENABLE@',str(enable)).replace('@TASKS@',str(len(cases))))
        command=[os.environ.get('IVERILOG_EXE','iverilog'),'-g2012','-D__ICARUS__','-s','tb','-o',str(out/f'sim{enable}.vvp')]
        if args.native:command += [str(Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v')]
        else:command+=['-DPORTABLE']
        command += [str(out/'dut.v'),str(out/f'tb{enable}.v')]
        for label,cmd in [('compile',command)]+[(f'run{mode}',[os.environ.get('VVP_EXE','vvp'),str(out/f'sim{enable}.vvp'),f'+MODE={mode}']) for mode in (0,1)]:
            log=out/f'{enable}-{label}.log'
            with log.open('w') as stream:r=subprocess.run(cmd,cwd=out,stdout=stream,stderr=subprocess.STDOUT,timeout=120)
            text=log.read_text(errors='replace')
            if r.returncode:raise RuntimeError(text[-2400:])
            logs[f'{enable}-{label}']=[line for line in text.splitlines() if line.startswith(('CASE ','PASS '))]
    receipt=dict(export_sha256=hashlib.sha256(source).hexdigest(),native=args.native,cases=cases,logs=logs)
    (out/'receipt.json').write_text(json.dumps(receipt,indent=2))
    print('PASS BOUNDS',len(cases),'cases; enabled/disabled; CE/backpressure; receipt:',out/'receipt.json')


if __name__=='__main__':main()
