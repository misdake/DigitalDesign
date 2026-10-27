"""阶段1独立整数/C/CE/背压RTL资格，不涉及拟合。"""
from pathlib import Path
import os,random,subprocess,json,hashlib,argparse,shutil
ROOT=Path(__file__).resolve().parents[3]
OUT=ROOT/'target/gpu-vliw-integration-20261018/stage1'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def expected(lod,n,mip,filt,uv,q,lane,slot):
    distance=min(lod//8,n);fine=n-distance if mip else n
    frac=lod%8 if mip and distance<n else 0
    p1=(frac*511+4)//8 if filt==2 else 0
    physical=max(1,fine);mask=(1<<physical)-1;values=[]
    for axis in uv:
        value=((axis<<2)>>(10-physical))-(0 if filt==0 else 128)
        values.append((value>>8&mask,value&255))
    owner=(511-p1)|(p1<<9)|((filt==0)<<18)|(fine<<19)|(max(0,fine-1)<<23)|(slot<<27)|((q*4+lane)<<31)|((p1==0)<<37)
    result=values[0][1]|(values[1][1]<<8)|(values[0][0]<<16)|(values[1][0]<<26)
    return owner,result
TB=r'''`timescale 1ns/1ps
module tb;
reg clk=0;always #5 clk=~clk;
`ifdef NATIVE
GSR GSR(.GSRI(1'b1));
`endif
reg reset=1,ce=1,allow=1,iv=0,ready=0;wire ir,adv,bad,fault,ov;wire[37:0]owner;wire[35:0]result;wire[1:0]phase;
reg[31:0]uv=0;reg[3:0]q=0,mask=0,slot=0,maxn=0;reg[1:0]lane=0,filt=0;reg[7:0]lod=0;reg mip=0;
wire issue,returned;wire[4:0]occupied,completed;
gpu_v2_texture_direct_lod dut(clk,reset,ce,allow,iv,uv,q,mask,slot,lane,lod,maxn,mip,filt,ir,adv,bad,fault,ready,ov,owner,result,phase,issue,returned,occupied,completed);
reg[66:0]inputs[0:@N@-1];reg[73:0]gold[0:@N@-1];integer sent=0,got=0,cycles=0,enabled=0,holds=0,stalls=0,fd,steady=0,lastissue=-1;
reg[31:0]rng=32'h3f123812;reg[86:0]snap;reg oldce;reg held=0;reg[73:0]heldvalue;
always @(posedge clk)if(!reset)begin
 if(held&&(!ov||{owner,result}!==heldvalue))$fatal(1,"held result changed");
 if(ov)begin held=!(adv&&ready);heldvalue={owner,result};end else held=0;
 if(adv&&iv&&ir)begin if(phase!=0)$fatal(1,"issue phase");if(lastissue>=0&&enabled-lastissue==2)steady=steady+1;lastissue=enabled;sent=sent+1;end
 if(adv&&ov&&ready)begin
  if(got>=@N@||{owner,result}!==gold[got])$fatal(1,"result %0d got %h,%h expected %h",got,owner,result,gold[got]);
  $fdisplay(fd,"R %0d %h %h",got,owner,result);got=got+1;
 end
end
initial begin
 fd=$fopen("trace.txt","w");$readmemh("inputs.mem",inputs);$readmemh("gold.mem",gold);
 repeat(2)@(negedge clk);reset=0;
 while(got<@N@)begin
  @(negedge clk);rng=(rng<<1)^(rng[31]?32'h04c11db7:0);
  ce=rng[2:0]!=0;allow=cycles%67<61;ready=(cycles%911>127)&&(rng[7:4]!=0);iv=sent<@N@&&rng[10:8]!=0;
  if(sent<@N@){q,lane,slot,maxn,mip,filt,lod,uv,mask}=inputs[sent];
  #1;snap={phase,occupied,completed,ov,owner,result};oldce=adv;
  if(adv)enabled=enabled+1;if(ov&&!ready)holds=holds+1;if(!ir&&iv)stalls=stalls+1;
  @(posedge clk);#2;cycles=cycles+1;
  if(!oldce&&{phase,occupied,completed,ov,owner,result}!==snap)$fatal(1,"CE/commit hold");
  if(fault||bad)$fatal(1,"unexpected fault");if(cycles>200000)$fatal(1,"bounded timeout");
 end
 if(steady<100||holds<100||stalls<100)$fatal(1,"coverage");
 @(negedge clk);ready=0;ce=1;allow=1;iv=1;{q,lane,slot,maxn,mip,filt,lod,uv,mask}=inputs[0];
 repeat(5)@(negedge clk);iv=0;reset=1;ce=0;
 @(posedge clk);#2;if(ov||occupied||completed||phase!=0||fault)$fatal(1,"CE0 reset");
 $display("PASS direct count=%0d cycles=%0d enabled=%0d holds=%0d stalls=%0d II2=%0d",got,cycles,enabled,holds,stalls,steady);$fclose(fd);$finish;
end
initial begin #2500000;$fatal(1,"wall limit");end
endmodule
'''
def main():
    ap=argparse.ArgumentParser();ap.add_argument('--tag',default='v3')
    ap.add_argument('--production-source',type=Path)
    opts=ap.parse_args()
    rng=random.Random(0x318199);vectors=[]
    for n in range(11):
        for mip in (0,1):
            for filt in range(3):
                for lod in [0,1,2,3,4,5,6,7,8,15,16,79,80,81,255]:
                    vectors.append((lod,n,mip,filt,[rng.randrange(65536),rng.randrange(65536)],rng.randrange(16),rng.randrange(4),rng.randrange(16)))
    for _ in range(512):vectors.append((rng.randrange(256),rng.randrange(11),rng.randrange(2),rng.randrange(3),[rng.randrange(65536),rng.randrange(65536)],rng.randrange(16),rng.randrange(4),rng.randrange(16)))
    packed=[];gold=[]
    for lod,n,mip,filt,uv,q,lane,slot in vectors:
        value=q
        for x,width in [(lane,2),(slot,4),(n,4),(mip,1),(filt,2),(lod,8),(uv[0]|uv[1]<<16,32),(1<<lane,4)]:value=(value<<width)|x
        packed.append(value);a,b=expected(lod,n,mip,filt,uv,q,lane,0 if opts.production_source else slot)
        gold.append((a<<36)|b)
    reports=[]
    for mode in ('portable','native'):
        d=OUT/f'coordinate-{opts.tag}-{mode}';d.mkdir(exist_ok=False)
        shutil.copy2(opts.production_source or OUT/f'test-export/direct-{mode}.v',d/'dut.v')
        bench=TB
        if opts.production_source:
            bench=bench.replace('uv,q,mask,slot,lane','uv,q,mask,lane')
            bench=bench.replace('if(sent<@N@){','if(sent<@N@&&phase==0){')
            # The real pool holds UV/config through V, then replaces Q after
            # that edge. A producer may change its next item on a U phase.
            bench=bench.replace('reg[66:0]inputs',f'defparam dut.queue.NATIVE={int(mode=="native")};\nreg[66:0]inputs')
        (d/'tb.v').write_text(bench.replace('@N@',str(len(vectors))))
        (d/'inputs.mem').write_text(''.join(f'{v:017x}\n' for v in packed));(d/'gold.mem').write_text(''.join(f'{v:019x}\n' for v in gold))
        cmd=[os.environ['IVERILOG_EXE'],'-g2012','-s','tb','-o',str(d/'test.vvp')]
        if mode=='native':cmd+=['-DNATIVE',str(Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v')]
        cmd +=[str(d/'dut.v'),str(d/'tb.v')]
        for stage,args in [('compile',cmd),('run',[os.environ['VVP_EXE'],str(d/'test.vvp')])]:
            cp=subprocess.run(args,cwd=d,capture_output=True,text=True,timeout=90);(d/f'{stage}.log').write_text(cp.stdout+cp.stderr)
            assert cp.returncode==0,cp.stdout+cp.stderr
        reports.append({'mode':mode,'summary':cp.stdout,'trace_sha256':sha(d/'trace.txt'),'actual_source_sha256':sha(d/'dut.v')});print(mode,cp.stdout,flush=True)
    assert reports[0]['trace_sha256']==reports[1]['trace_sha256']
    (OUT/f'coordinate-{opts.tag}-receipt.json').write_text(json.dumps({'status':'pass','vectors':len(vectors),'reports':reports,'sources':{p.name:sha(p) for p in (OUT/'test-export').glob('direct-*.v')},'boundary':'独立最终LOD解码/C整数金值与真实同步队列；非完整GPU/板测'},ensure_ascii=False,indent=2)+'\n',encoding='utf8')
if __name__=='__main__':main()
