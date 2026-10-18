"""Bounded TDP ROP/cache integer golden check with paid 128-byte service."""
from pathlib import Path
import argparse,hashlib,json,os,random,subprocess
R=Path(__file__).resolve().parents[3];O=R/'target/gpu-framebuffer-tdp-20261018';S=R/'ip/gpu-v2/src/system/pixel';SHA=lambda p:hashlib.sha256(p.read_bytes()).hexdigest();FB=192000;DB=262144

def rne(n,d):
 q,r=divmod(n,d);return q+int(2*r>d or 2*r==d and q&1)
def expand(c):return [(c>>11<<3)|(c>>13),(((c>>5)&63)<<2)|((c>>9)&3),((c&31)<<3)|((c>>2)&7)]
def arithmetic(oldc,oldd,rgba,sd,func,covered,dw,blend):
 passed=covered and [False,sd<oldd,sd==oldd,sd<=oldd,sd>oldd,sd!=oldd,sd>=oldd,True][func]
 old=expand(oldc);a=((rgba>>24)+2)//4 if blend else 64;new=[(v*64+(((rgba>>(8*i))&255)-v)*a+32)//64 for i,v in enumerate(old)]
 c=(rne(new[0]*31,255)<<11)|(rne(new[1]*63,255)<<5)|rne(new[2]*31,255)
 return c,passed,passed and dw

def make_vectors(d):
 rng=random.Random(1718);memory=bytearray(DB+FB)
 for tile in range(375):
  for pixel in range(256):
   for base,value in [(0,(tile*313+pixel*17)&65535),(DB,32000+(tile*7+pixel)%20000)]:
    a=base+tile*512+pixel*2;memory[a:a+2]=value.to_bytes(2,'little')
 gold=bytearray(memory);quads=[];pixels=[]
 # Cold fills, both bank rotations, and repeated identical quad at true II2.
 tiles=[0]+[0]*12+[1,2,3,4,5,6,7]+[0]*8+[8]+[8]*8+[2,9,2,10,0,3,11,3,24,25,49,350,374,25,350]
 for q,tile in enumerate(tiles):
  x=(tile%25)*16+(2 if q%2 and q>12 else 0);y=(tile//25)*16+(2 if q%3 and q>12 else 0);mask=15 if q%4 else 5;f=q%8;dw=(q%5)!=0;blend=bool(q%2)
  raw_tile=(tile//25)*32+tile%25
  quads.append(x|(y<<9)|(mask<<17)|(f<<21)|(dw<<24)|(blend<<25)|(raw_tile<<26))
  for lane in range(4):
   px=x+lane%2;py=y+lane//2;off=tile*512+(py%16)*32+(px%16)*2
   oc=int.from_bytes(gold[off:off+2],'little');od=int.from_bytes(gold[DB+off:DB+off+2],'little')
   alpha=[0,255,128,63][(q+lane)%4];rgba=rng.getrandbits(24)|(alpha<<24);sd=max(0,min(65535,od+[-1,0,1,50][(q+lane)%4]))
   nc,cw,dwflag=arithmetic(oc,od,rgba,sd,f,bool(mask>>lane&1),dw,blend)
   pixels.append(rgba|(sd<<32)|(oc<<48)|(od<<64)|(nc<<80)|(int(cw)<<96)|(int(dwflag)<<97))
   if cw:gold[off:off+2]=nc.to_bytes(2,'little')
   if dwflag:gold[DB+off:DB+off+2]=sd.to_bytes(2,'little')
 for n,v,w in [('initial.mem',memory,2),('gold.mem',gold,2),('quads.mem',quads,9),('pixels.mem',pixels,25)]:
  (d/n).write_text(''.join(f'{a:0{w}x}\n' for a in v))
 return len(quads)

def run(cmd,cwd,path,t=120):
 p=subprocess.run(list(map(str,cmd)),cwd=cwd,capture_output=True,timeout=t);path.write_bytes(p.stdout+p.stderr);assert p.returncode==0,(p.stdout+p.stderr).decode(errors='replace')[-6000:];return p.stdout.decode(errors='replace')

def main():
 ap=argparse.ArgumentParser(__doc__);ap.add_argument('--tag',default='scalar-v1');ap.add_argument('--mode',choices=['portable','native','both'],default='both');ap.add_argument('--hint-pattern',choices=['adversarial','lookahead'],default='adversarial');ap.add_argument('--prefetch',choices=['off','clean','dirty'],default='off');ap.add_argument('--export-root',type=Path,default=R/'target/gpu-vliw-integration-20261018/stage2/backend-export');a=ap.parse_args();root=O/a.tag;root.mkdir(parents=True,exist_ok=False);records=[]
 for mode in ['portable','native'] if a.mode=='both' else [a.mode]:
  d=root/mode;d.mkdir();nq=make_vectors(d)
  # Use the actual shared product bank from the selected frozen main export.
  base=a.export_root/f'backend-{mode}.v';s=base.read_text();x=s.index('module gpu_v2_final_rop_bank(');y=s.index('endmodule',x)+9
  source='\n'.join((S/f).read_text() for f in ['framebuffer_tdp_cache.v','framebuffer_tdp_leaf.v','framebuffer_tdp_bank.v'])+'\n'+s[x:y]
  (d/'dut.v').write_text(('`define GPU_VLIW_POOL_PORTABLE\n' if mode=='portable' else '`define NATIVE\n')+source)
  tb=TB.replace('@NQ@',str(nq)).replace('@BYTES@',str(DB+FB)).replace('@DEPTH@',str(DB)).replace('@PFEN@',str(int(a.prefetch!='off'))).replace('@PFDIRTY@',str(int(a.prefetch=='dirty'))).replace('@LOOK@',str(int(a.hint_pattern=='lookahead')));(d/'tb.v').write_text(tb)
  cmd=[os.environ['IVERILOG_EXE'],'-g2012','-s','tb','-o',d/'sim.vvp']
  if mode=='native':cmd+=[Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v']
  run(cmd+[d/'dut.v',d/'tb.v'],d,d/'compile.log');log=run([os.environ['VVP_EXE'],d/'sim.vvp'],d,d/'run.log');print(mode,log,flush=True)
  records.append(dict(mode=mode,log=log,dut_sha256=SHA(d/'dut.v'),trace_sha256=SHA(d/'trace.txt'),shared_bank_source_sha256=SHA(base)))
 if len(records)==2:assert records[0]['trace_sha256']==records[1]['trace_sha256']
 (root/'receipt.json').write_text(json.dumps(dict(status='pass',prefetch=a.prefetch,hint_pattern=a.hint_pattern,reports=records,sources={str(p.relative_to(R)):SHA(p) for p in [S/'framebuffer_tdp_cache.v',S/'framebuffer_tdp_leaf.v',S/'framebuffer_tdp_bank.v',Path(__file__)]}),indent=2))
TB=r'''`timescale 1ns/1ps
module tb;
`ifdef NATIVE
GSR GSR(.GSRI(1'b1));
`endif
reg clk=0,reset=1,ce=0,commit_allow=1,shade_phase=0;
reg prefetch_abort=0,prefetch_valid=0;reg[8:0]prefetch_tile=0;
reg row_valid=0;reg[8:0]row_tile,row_x;reg[7:0]row_y;reg[3:0]row_mask;reg[2:0]row_index=0;reg[31:0]row_data;
wire row_accept,row_ready;reg[2:0]depth_func;reg depth_write,blend;
wire[53:0]shared_rop_product;wire shared_rop_offer,shared_rop_ce;wire[26:0]shared_rop_a;wire[7:0]shared_rop_b;
wire preflight_bad,commit_valid,fault,idle,flush_complete;wire[8:0]commit_x;wire[7:0]commit_y;wire[3:0]commit_mask;reg flush_request=0;
wire mem_req_valid,mem_req_write;wire[31:0]mem_req_addr;reg mem_req_ready=0;reg[63:0]mem_read_data=0;reg mem_read_valid=0;
wire mem_write_valid;wire[63:0]mem_write_data;reg mem_write_ready=0,mem_complete_valid=0,mem_complete_ok=1;
gpu_v2_framebuffer_tdp #(.COLOR_BASE(0),.DEPTH_BASE(@DEPTH@),.WIDTH(400),.HEIGHT(240),.PREFETCH_ENABLE(@PFEN@),.PREFETCH_DIRTY(@PFDIRTY@)) dut(.*);
gpu_v2_final_rop_bank shared(.clk(clk),.reset(reset),.rop_ce(shared_rop_ce),.rop_a(shared_rop_a),.rop_b(shared_rop_b),.rop_product(shared_rop_product),.spec_ce(1'b0),.spec_a(9'd0),.spec_b(24'd0),.spec_product());
reg[7:0]memory[0:@BYTES@-1],gold[0:@BYTES@-1];reg[35:0]quads[0:@NQ@-1];reg[99:0]pixels[0:@NQ@*4-1];
reg[35:0]q;reg[99:0]p;integer wall=0,ct=0,rows=0,returned=0,committed=0,requests=0,ab=0,ii2=0,lastc=-99,lastquad=-99,same8=0;
integer input_after=@LOOK@*350,prefetchuse=0;
integer hintpush=0,hintpop=0,hintstart=0,hintdirty=0,hitwhilemt=0;
integer qn,lane,pix,pending=0,address=0,beat=0,delayc=0,k,i,fd;reg[1:0]oldcal;
initial begin
 $readmemh("initial.mem",memory);$readmemh("gold.mem",gold);$readmemh("quads.mem",quads);$readmemh("pixels.mem",pixels);fd=$fopen("trace.txt","w");
 #1;clk=1;#1;clk=0;#1;reset=0;
 for(wall=1;wall<30000;wall=wall+1)begin
  ce=wall%43!=5&&wall%43!=6&&wall%43!=7;row_valid=0;
  qn=rows/8;lane=(rows%8)/2;pix=qn*4+lane;row_index=rows%8;
  if(qn<@NQ@)begin
   q=quads[qn];p=pixels[pix];row_x=q[8:0];row_y=q[16:9];row_mask=q[20:17];depth_func=q[23:21];depth_write=q[24];blend=q[25];row_tile=q[34:26];
   row_data=row_index[0]?{16'd0,p[47:32]}:p[31:0];row_valid=(row_index[0]==shade_phase)&&(wall>=input_after);
  end
  // Hints use wall-clock pulses even while the backend CE is paused.
  prefetch_valid=@PFEN@&&rows<@NQ@*8&&wall%3!=0;
  // Early stale target, look-ahead useful tiles, and dirty-only late pressure.
  if(@LOOK@&&qn<@NQ@)prefetch_tile=quads[qn][34:26];
  else if(wall<10)prefetch_tile=0;
  else if(qn<30)prefetch_tile=(qn+2)%12;
  else prefetch_tile=12+(wall/180)%6;
  mem_req_ready=pending==0&&wall%5!=1;mem_read_valid=pending==1&&delayc==0&&wall%7!=2;
  mem_write_ready=pending==3&&delayc==0&&wall%11!=3;mem_complete_valid=(pending==2||pending==4)&&delayc==0;
  mem_read_data=0;if(mem_read_valid)for(k=0;k<8;k=k+1)mem_read_data[k*8+:8]=memory[address+beat*8+k];
  #2;
  if(fault||preflight_bad)$fatal(1,"cache fault row%0d cal%0d valid%0d",rows,dut.cal,row_valid);
  if(dut.hint_push)hintpush=hintpush+1;
  if(dut.hint_pop)hintpop=hintpop+1;
  if(dut.hint_start)begin hintstart=hintstart+1;if(dut.dirc[dut.spec_line]||dut.dird[dut.spec_line])hintdirty=hintdirty+1;end
  if(row_accept&&dut.mt_active)hitwhilemt=hitwhilemt+1;
  if(row_accept)begin
   if(!row_index[0])begin if(ct-lastc==2)ii2=ii2+1;lastc=ct;end
   if(row_index==0&&dut.prefetch_protect_valid&&dut.hit_line==dut.prefetch_protect_line)prefetchuse=prefetchuse+1;
   if(row_index==0)begin if(!@LOOK@&&qn>0&&qn<=12)begin if(ct-lastquad!=8)$fatal(1,"same quad not II8");same8=same8+1;end lastquad=ct;end
   if(row_index==7&&@LOOK@)input_after=wall+350;
   rows=rows+1;
  end
  if(dut.return_fire)begin
   p=pixels[returned];if(dut.leaf_cw!==p[96]||dut.leaf_dw!==p[97])$fatal(1,"ROP flags pixel%0d",returned);
   if(p[96]&&dut.leaf_new_color!==p[95:80])$fatal(1,"ROP color pixel%0d got%h want%h",returned,dut.leaf_new_color,p[95:80]);
   if(p[97]&&dut.leaf_new_depth!==p[47:32])$fatal(1,"ROP depth pixel%0d",returned);
   $fdisplay(fd,"R %0d %h %b%b",returned,dut.leaf_new_color,dut.leaf_cw,dut.leaf_dw);returned=returned+1;
  end
  if(commit_valid)begin q=quads[committed];if({commit_x,commit_y,commit_mask}!=={q[8:0],q[16:9],q[20:17]})$fatal(1,"commit header");committed=committed+1;end
  if((dut.banks[0].a_en&&dut.banks[0].b_en)||(dut.banks[1].a_en&&dut.banks[1].b_en)||(dut.banks[2].a_en&&dut.banks[2].b_en)||(dut.banks[3].a_en&&dut.banks[3].b_en))ab=ab+1;
  if(mem_req_valid&&mem_req_ready)begin address=mem_req_addr;beat=0;delayc=2;pending=mem_req_write?3:1;requests=requests+1;end else if(delayc>0)delayc=delayc-1;
  if(mem_read_valid)begin beat=beat+1;if(beat==16)begin pending=2;delayc=3;end end
  if(mem_write_valid&&mem_write_ready)begin for(k=0;k<8;k=k+1)memory[address+beat*8+k]=mem_write_data[k*8+:8];beat=beat+1;if(beat==16)begin pending=4;delayc=3;end end
  if(mem_complete_valid)pending=0;
  if(rows==@NQ@*8&&idle)flush_request=1;
  if(flush_complete)begin
   if(committed!=@NQ@||returned!=@NQ@*4||ii2<50||(!@LOOK@&&same8!=12)||(!@LOOK@&&ab==0))$fatal(1,"coverage q%0d ret%0d ii2%0d ab%0d",committed,returned,ii2,ab);
   for(i=0;i<@BYTES@;i=i+1)if(memory[i]!==gold[i])$fatal(1,"memory byte%0d got%h want%h",i,memory[i],gold[i]);
   $display("PASS TDP Q=%0d pixels=%0d wall=%0d II2=%0d sameQ8=%0d AB=%0d requests=%0d",committed,returned,wall,ii2,same8,ab,requests);
   $display("HINT push=%0d pop=%0d start=%0d dirty=%0d hitWhileMT=%0d used=%0d",hintpush,hintpop,hintstart,hintdirty,hitwhilemt,prefetchuse);
   if(@PFEN@&&(hintpush==0||hintpop==0||hintstart==0||(!@LOOK@&&hitwhilemt==0)||(@LOOK@&&prefetchuse==0)))$fatal(1,"hint activity missing");
   if(@PFDIRTY@&&hintdirty==0)$fatal(1,"dirty speculative victim not covered");$fclose(fd);$finish;
  end
  oldcal=dut.cal;clk=1;#2;clk=0;#1;if(ce)begin ct=ct+1;shade_phase=!shade_phase;end
  if(!ce&&dut.cal!==oldcal)$fatal(1,"CE0 A calendar changed");
 end
 $fatal(1,"bounded timeout");
end
initial begin #300000;$fatal(1,"absolute timeout");end
endmodule
'''
if __name__=='__main__':main()
