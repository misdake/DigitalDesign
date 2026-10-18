"""Bounded, independent producer/event scoreboard; no old ingress is instantiated."""
from pathlib import Path
import hashlib,json,os,subprocess
ROOT=Path(__file__).resolve().parents[3]
OUT=ROOT/'target/gpu-vliw-integration-20261018/stage2'/os.environ.get('VLIW_POOL_TEST_TAG','pool-completion-contract-v1')
TB=r'''`timescale 1ns/1ps
module tb;
reg clk=0;always #5 clk=~clk;
`ifdef NATIVE
GSR GSR(.GSRI(1'b1));
`endif
reg reset=1,fc=0,bc=0;reg[1:0]phase=0;reg qv=0,rgb=RGB;wire qr;wire[2:0]qid;
reg[7:0]qx;reg[6:0]qy;reg bank,unlit;
reg ev=0;reg[2:0]eid;reg[3:0]emask;reg rw=0;reg[6:0]ra;reg[35:0]rd;
reg dv=0;reg[4:0]dpid;reg[15:0]dz;reg dpass;reg lv=0;reg[2:0]lid;reg[7:0]lod;
reg pr=0;wire pub;wire[4:0]ps;wire[22:0]pb;reg[4:0]di=0;wire[22:0]dis,dhs,dns;wire[8:0]tile,nexttile;
reg br=0;reg[7:0]ba;wire bv,bcol;wire[23:0]bd;
reg lr=0,sr=0;wire lvalid,llast,svalid,slast;wire[35:0]ldata,sdata;wire[2:0]lrow;wire[1:0]srow;wire[9:0]ldesc;wire[18:0]sdesc;
reg retire=0,skip=0;reg[4:0]rid;wire empty,skipready;wire[4:0]rs;wire[5:0]used,prod,lp,sp;wire busy,fault;
gpu_v2_vliw_quad_pool dut(.clk(clk),.reset(reset),.front_ce(fc),.backend_ce(bc),.phase(phase),.draw_bank(bank),.draw_unlit(unlit),
 .quad_valid(qv),.quad_ready(qr),.quad_id(qid),.quad_x(qx),.quad_y(qy),.quad_rgb(rgb),.quad_end_valid(ev),.quad_end_id(eid),.quad_end_mask(emask),
 .result_we(rw),.result_wa(ra),.result_wd(rd),.depth_event_valid(dv),.depth_event_pid(dpid),.depth_event_z16(dz),.depth_event_pass(dpass),
 .lod_valid(lv),.lod_id(lid),.lod_value(lod),.publish_ready(pr),.published(pub),.published_slot(ps),.published_begin(pb),
 .descriptor_issue_slot(di),.descriptor_issue(dis),.descriptor_head(dhs),.descriptor_head_tile(tile),.descriptor_next(dns),.descriptor_next_tile(nexttile),
 .basic_read(br),.basic_read_address(ba),.basic_return_valid(bv),.basic_return_data(bd),.basic_collision(bcol),
 .light_ready(lr),.light_valid(lvalid),.light_data(ldata),.light_row(lrow),.light_last(llast),.light_descriptor(ldesc),
 .sample_ready(sr),.sample_valid(svalid),.sample_data(sdata),.sample_row(srow),.sample_last(slast),.sample_descriptor(sdesc),
 .retire_valid(retire),.retire_id(rid),.empty_skip(skip),.empty_head(empty),.empty_skip_ready(skipready),.retire_slot(rs),
 .global_used(used),.producer_used(prod),.light_pending(lp),.sample_pending(sp),.busy(busy),.fault(fault));
integer reset_count=0;integer starts[0:NQ-1];integer ft=0,bt=0,sent=0,published=0,retired=0,cycles=0,laststart=-16;
integer lq=0,lrows=0,sq=0,srows=0,brow=0,breturned=0,emptycount=0,peaku=0,peakp=0,waits=0,holds=0;
integer j,k,delta,ln,wr,addrq,addrr,fd,last_lq=-2,last_ltick=-100,ii16=0;
reg[31:0]rng=32'h1482fd6e;reg b_pending=0;reg[23:0]b_expected;
reg lhold=0,shold=0;reg[49:0]lh;reg[57:0]sh;reg[3:0]m;
function[3:0]geom;input integer q;begin if(STRESS==0)geom=15;else if(q%5==0)geom=0;else geom=(q%15)+1;end endfunction
function covered;input integer q,l;reg[3:0]g;begin g=geom(q);covered=g[l];end endfunction
function pass;input integer q,l;begin pass=covered(q,l)&&(STRESS==0||(q+l)%7!=0);end endfunction
function[3:0]mask;input integer q;integer n;begin mask=0;for(n=0;n<4;n=n+1)mask[n]=pass(q,n);end endfunction
function unlight;input integer q;begin unlight=STRESS!=0&&q%7==0;end endfunction
function[7:0]x;input integer q;begin x=(q*7)%200;end endfunction
function[6:0]y;input integer q;begin y=(q*11)%120;end endfunction
function[7:0]level;input integer q;begin level=q*19;end endfunction
function[11:0]nval;input integer q,l,a;begin nval=(q*117+l*811+a*299+2047)&4095;end endfunction
function[15:0]uval;input integer q,l;begin uval=q*233+l*997;end endfunction
function[15:0]vval;input integer q,l;begin vval=65535-q*89-l*71;end endfunction
function[15:0]zval;input integer q,l;begin zval=q*197+l*3+1;end endfunction
function[7:0]red;input integer q,l;begin red=q*3+l*71;end endfunction
function[7:0]green;input integer q,l;begin green=q*7+l*23;end endfunction
function[7:0]blue;input integer q,l;begin blue=q*13+l*17;end endfunction
function[22:0]header;input integer q;begin header={y(q),x(q),1'b0,1'bRGB,unlight(q),q[0],mask(q)};end endfunction
function[15:0]rne;input integer n,d;integer a,r,q;begin a=n<0?-n:n;q=a/d;r=a%d;if(2*r>d||(2*r==d&&(q%2)))q=q+1;if(n<0)q=-q;rne=q;end endfunction
function[31:0]ndc;input integer q,l;reg[15:0]xx,yy;begin xx=rne((2*(2*x(q)+(l%2))+1-400)*16384,400);yy=rne((240-2*(2*y(q)+(l/2))-1)*16384,240);ndc={yy,xx};end endfunction
function[23:0]basic;input integer q,row;begin if(row%2)basic={8'd0,zval(q,row/2)};else if(RGB)basic={blue(q,row/2),green(q,row/2),red(q,row/2)};else basic=24'hffffff;end endfunction
task drive;
begin
 rng=(rng<<1)^(rng[31]?32'h04c11db7:0);
 fc=STRESS==0||rng[2:0]!=0;bc=STRESS==0||rng[6:4]!=0;
 phase=ft%4;qv=sent<NQ&&phase==0&&ft-laststart>=16;
 qx=x(sent);qy=y(sent);bank=sent%2;unlit=unlight(sent);rgb=RGB;
 ev=0;rw=0;dv=0;lv=0;
 for(j=0;j<sent;j=j+1)begin
  delta=ft-starts[j];
  if(delta==14)begin ev=1;eid=j%8;emask=geom(j);end
  if(delta==(RGB?15:50))begin lv=1;lid=j%8;lod=RGB?8'd0:level(j);end
  for(ln=0;ln<4;ln=ln+1)begin
   if(delta==13+4*ln)begin dv=1;dpid=((j%8)*4+ln);dz=zval(j,ln);dpass=pass(j,ln);end
   for(wr=0;wr<3;wr=wr+1)if(delta==14+4*ln+wr)begin
    rw=1;ra=((j%8)*16+ln*4+wr);
    if(wr==0)rd={pass(j,ln),11'd0,nval(j,ln,1),nval(j,ln,0)};
    else if(wr==1)begin if(RGB)rd={16'd0,red(j,ln),nval(j,ln,2)};else rd={8'd0,uval(j,ln),nval(j,ln,2)};end
    else begin if(RGB)rd={4'd0,zval(j,ln),blue(j,ln),green(j,ln)};else rd={2'd0,zval(j,ln),2'd0,vval(j,ln)};end
   end
  end
 end
 pr=STRESS==0||(ft>=160&&cycles%113>23&&rng[9:8]!=0);
 lr=(lrow[0]==0||bt%4==0)&&(STRESS==0||(ft>=600&&(cycles%211>47&&rng[12:10]!=0)));
 sr=bt%4==0&&(STRESS==0||(ft>=600&&(cycles%197>51&&rng[15:13]!=0)));
 // Random producer-independent queries exercise both metadata read views.
 di=published>retired?(retired+(rng[20:17]%(published-retired)))%32:retired%32;
 br=0;retire=0;skip=0;rid=retired%32;
 if(published>retired)begin
  if(mask(retired)==0)skip=skipready;
  else begin
   if(brow<8&&(STRESS==0||rng[23:21]!=0))begin br=1;ba=((retired%32)*8)+brow;end
   if(breturned==8&&dut.light_cursor!=dut.retired&&dut.sample_cursor!=dut.retired)retire=1;
  end
 end
end endtask
always @(posedge clk)if(!reset)begin
 if(fault||bcol)$fatal(1,"pool fault");
 if(lhold&&(!lvalid||{ldesc,lrow,llast,ldata}!==lh))$fatal(1,"Light hold changed");
 if(shold&&(!svalid||{sdesc,srow,slast,sdata}!==sh))$fatal(1,"Sample hold changed");
 lhold=lvalid&&!(bc&&lr);lh={ldesc,lrow,llast,ldata};shold=svalid&&!(bc&&sr);sh={sdesc,srow,slast,sdata};
 if(lhold||shold)holds=holds+1;
 if(bv!==b_pending)$fatal(1,"Basic return validity");
 if(b_pending)begin if(bd!==b_expected)$fatal(1,"Basic got%h expected%h q%0d",bd,b_expected,retired);breturned=breturned+1;end
 b_pending=bc&&br;if(b_pending)begin b_expected=basic(retired,brow);brow=brow+1;end
 if(published>retired)begin
  if(dhs!==header(retired))$fatal(1,"head descriptor %0d %h %h",retired,dhs,header(retired));
  if(tile!==((y(retired)/8)*25+x(retired)/8))$fatal(1,"head tile %0d",retired);
  if(published>retired+1)begin
   if(dns!==header(retired+1))$fatal(1,"next descriptor %0d",retired+1);
   if(nexttile!==((y(retired+1)/8)*25+x(retired+1)/8))$fatal(1,"next tile %0d",retired+1);
  end
  addrq=retired+((di-retired%32+32)%32);if(dis!==header(addrq))$fatal(1,"issue descriptor");
 end
 if(fc&&qv&&qr)begin if(qid!==sent%8)$fatal(1,"qid");starts[sent]=ft;laststart=ft;sent=sent+1;end
 if(pub)begin
  if(ps!==published%32||pb!==header(published))$fatal(1,"publication %0d",published);
  $fdisplay(fd,"P %0d %h",published,pb);published=published+1;
 end
 if(bc)begin
  while(lq<published&&(mask(lq)==0||unlight(lq)))lq=lq+1;
  while(sq<published&&(mask(sq)==0||RGB))sq=sq+1;
  if(lvalid&&lr)begin
   if(lq>=published||lrow!==lrows[2:0]||ldesc!=={lq[0],mask(lq),lq[4:0]})$fatal(1,"Light descriptor q%0d row%0d",lq,lrows);
   if(!lrow[0])begin
    if(ldata!=={nval(lq,lrows/2,2),nval(lq,lrows/2,1),nval(lq,lrows/2,0)})$fatal(1,"normal q%0d row%0d %h",lq,lrows,ldata);
    if(lrows==0)begin if(STRESS==0&&lq>1&&last_lq==lq-1)begin if(bt-last_ltick!=16)$fatal(1,"Light II not16 q%0d bt%0d prev%0d gap%0d",lq,bt,last_ltick,bt-last_ltick);ii16=ii16+1;end last_lq=lq;last_ltick=bt;end
   end else if(ldata!=={4'd0,ndc(lq,lrows/2)})$fatal(1,"NDC q%0d lane%0d got%h exp%h",lq,lrows/2,ldata,ndc(lq,lrows/2));
   $fdisplay(fd,"L %0d %0d %h",lq,lrows,ldata);if(lrows==7)begin lrows=0;lq=lq+1;end else lrows=lrows+1;
  end
  if(svalid&&sr)begin
   if(sq>=published||srow!==srows[1:0]||sdesc!=={level(sq),1'b0,sq[0],mask(sq),sq[4:0]})$fatal(1,"Sample descriptor q%0d row%0d",sq,srows);
   if(sdata!=={2'd0,vval(sq,srows),2'd0,uval(sq,srows)})$fatal(1,"UV q%0d row%0d",sq,srows);
   $fdisplay(fd,"S %0d %0d %h",sq,srows,sdata);if(srows==3)begin srows=0;sq=sq+1;end else srows=srows+1;
  end
  if(retire||skip)begin if(rs!==retired%32)$fatal(1,"retire ordinal");if(skip)begin if(!empty||!skipready||mask(retired)!=0)$fatal(1,"empty skip");emptycount=emptycount+1;end $fdisplay(fd,"R %0d",retired);retired=retired+1;brow=0;breturned=0;end
  bt=bt+1;
 end
 if(fc)ft=ft+1;
 if(used>peaku)peaku=used;if(prod>peakp)peakp=prod;if(qv&&!qr)waits=waits+1;
end
initial begin
 fd=$fopen("trace.txt","w");for(k=0;k<NQ;k=k+1)starts[k]=-99999;
 repeat(2)@(negedge clk);reset=0;
 while(retired<NQ)begin @(negedge clk);
 if(RESTART&&((reset_count==0&&cycles==19)||(reset_count==1&&cycles==58)||(reset_count==2&&cycles==149)))begin
  reset=1;fc=0;bc=0;rw=0;ev=0;dv=0;lv=0;qv=0;retire=0;skip=0;br=0;
  @(posedge clk);#2;if(used||prod||fault||lvalid||svalid||pub||bv)$fatal(1,"inflight CE0 reset");
  ft=0;bt=0;sent=0;published=0;retired=0;laststart=-16;lq=0;lrows=0;sq=0;srows=0;brow=0;breturned=0;
  b_pending=0;lhold=0;shold=0;last_lq=-2;last_ltick=-100;ii16=0;peaku=0;peakp=0;emptycount=0;holds=0;
  for(k=0;k<NQ;k=k+1)starts[k]=-99999;
  reset_count=reset_count+1;$fdisplay(fd,"RESET %0d",reset_count);@(negedge clk);reset=0;
 end
 drive();@(posedge clk);#2;cycles=cycles+1;if(cycles>100000)$fatal(1,"pool bounded timeout sent%0d pub%0d ret%0d",sent,published,retired);end
 @(negedge clk);fc=0;bc=0;qv=0;rw=0;ev=0;dv=0;lv=0;pr=0;br=0;retire=0;skip=0;
 repeat(3)@(negedge clk);
 if(sent!=NQ||published!=NQ||lq!=NQ||sq!=NQ||busy)$fatal(1,"pool final conservation");
 if(RESTART&&reset_count!=3)$fatal(1,"reset coverage");
 if(STRESS==0&&ii16<NQ-3)$fatal(1,"steady II coverage");
 if(STRESS!=0&&(emptycount<10||holds<100||peaku!=24||peakp!=8))$fatal(1,"stress coverage");
 // Reset both domains while deliberately disabled.
 reset=1;@(posedge clk);#2;if(used||prod||fault||lvalid||svalid||pub||bv)$fatal(1,"CE0 reset");
 $display("PASS POOL rgb=%0d stress=%0d Q=%0d cycles=%0d peak=%0d unpub=%0d empty=%0d holds=%0d II16=%0d",RGB,STRESS,NQ,cycles,peaku,peakp,emptycount,holds,ii16);$fclose(fd);$finish;
end
initial begin #1200000;$fatal(1,"absolute timeout");end
endmodule
'''
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
 reports=[]
 for mode in ('portable','native'):
  for rgb in (0,1):
   for stress,restart in ((0,0),(1,0),(1,1)):
    d=OUT/f'{mode}-rgb{rgb}-stress{stress}-restart{restart}';d.mkdir(parents=True,exist_ok=True)
    tb=TB.replace('NQ','96').replace('RGB',str(rgb)).replace('STRESS',str(stress)).replace('RESTART',str(restart))
    (d/'tb.v').write_text(tb)
    args=[os.environ['IVERILOG_EXE'],'-g2012','-s','tb','-o',str(d/'test.vvp')]
    if mode=='native':args+=['-DNATIVE',str(Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v')]
    args += [str(OUT/f'{mode}.v'),str(d/'tb.v')]
    for stage,command in [('compile',args),('run',[os.environ['VVP_EXE'],str(d/'test.vvp')])]:
     p=subprocess.run(command,cwd=d,capture_output=True,text=True,timeout=90)
     (d/f'{stage}.log').write_text(p.stdout+p.stderr);assert p.returncode==0,p.stdout+p.stderr
    assert 'PASS POOL' in p.stdout
    r=dict(mode=mode,rgb=rgb,stress=stress,restart=restart,summary=p.stdout,trace_sha256=sha(d/'trace.txt'));reports.append(r);print(p.stdout,flush=True)
 for rgb in (0,1):
  for stress,restart in ((0,0),(1,0),(1,1)):assert len({r['trace_sha256'] for r in reports if r['rgb']==rgb and r['stress']==stress and r['restart']==restart})==1
 (OUT/'receipt.json').write_text(json.dumps(dict(status='pass',reports=reports,generated={str(OUT/f'{mode}.v'):sha(OUT/f'{mode}.v') for mode in ('portable','native')},sources={str(p.relative_to(ROOT)):sha(p) for p in [ROOT/'ip/gpu-v2/src/system/pixel/vliw_quad_pool.v',ROOT/'ip/gpu-v2/src/system/pixel/vliw_quad_pool.rs',Path(__file__)]},scope='Independent pool formatting/port/lifetime/NDC qualification; not full GPU arithmetic'),indent=2)+'\n')
 print('PASS POOL MATRIX',flush=True)
if __name__=='__main__':main()
