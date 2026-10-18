"""Bounded event comparison of the selected Lighting tuple with the frozen Light2 consumer."""
from pathlib import Path
import argparse,hashlib,json,os,subprocess
R=Path(__file__).resolve().parents[3];O=R/'target/gpu-chain-tight-20261018/light-tuple'
SHA=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
def run(cmd,d,log):
 p=subprocess.run(list(map(str,cmd)),cwd=d,capture_output=True,timeout=120);log.write_bytes(p.stdout+p.stderr);assert p.returncode==0,(p.stdout+p.stderr).decode(errors='replace')[-5000:];return p.stdout.decode(errors='replace')
def main():
 ap=argparse.ArgumentParser(__doc__);ap.add_argument('--tag',default='events-v1');ap.add_argument('--export-root',type=Path,default=O/'exports');a=ap.parse_args();out=O/a.tag;out.mkdir(parents=True,exist_ok=False)
 old=(R/'ip/gpu-v2/src/system/pixel/lighting_consumer.v').read_text();old=old[old.index('module '):].replace('module gpu_v2_pixel_lighting_consumer(', 'module old_light_consumer(').replace('input wire clk,reset,ce,','input wire clk,reset,ce,compute_tick,').replace('assign context_accepted=switching && core_context_ready;','assign context_accepted=ce && switching && core_context_ready;').replace('assign pixel_accepted=core_in_valid && core_in_ready;','assign pixel_accepted=ce && core_in_valid && core_in_ready;').replace('.clk(clk),.reset(reset),.ce(ce),','.clk(clk),.reset(reset),.ce(compute_tick),')
 reports=[]
 for mode in ['portable','native']:
  for stress,restart in [(0,0),(1,0),(1,1)]:
   d=out/f'{mode}-s{stress}-r{restart}';d.mkdir();src=a.export_root/f'backend-{mode}.v';(d/'dut.v').write_text(src.read_text()+'\n'+old);tb=TB.replace('@STRESS@',str(stress)).replace('@RESET@',str(restart));(d/'tb.v').write_text(tb)
   cmd=[os.environ['IVERILOG_EXE'],'-g2012','-s','tb','-o',d/'sim.vvp']
   if mode=='native':cmd+=[Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v']
   run(cmd+[d/'dut.v',d/'tb.v'],d,d/'compile.log');log=run([os.environ['VVP_EXE'],d/'sim.vvp'],d,d/'run.log');print(mode,log,flush=True);reports.append(dict(mode=mode,stress=stress,restart=restart,log=log,trace_sha256=SHA(d/'trace.txt'),dut_sha256=SHA(d/'dut.v')))
 for stress,restart in [(0,0),(1,0),(1,1)]:assert len({x['trace_sha256'] for x in reports if x['stress']==stress and x['restart']==restart})==1
 (out/'receipt.json').write_text(json.dumps(dict(status='pass',scope='actual Lighting core operands/acceptance and key/epoch/G/H compared per physical edge; old private compute_tick contract retained',reports=reports,sources={str(p.relative_to(R)):SHA(p) for p in [Path(__file__),R/'ip/gpu-v2/src/system/pixel/lighting_consumer.v',R/'ip/gpu-v2/src/system/pixel/vliw_lighting_tuple.v']}),indent=2))
TB=r'''`timescale 1ns/1ps
module tb;
`ifdef GPU_V2_GOWIN_DSP
GSR GSR(.GSRI(1'b1));
`endif
reg clk=0,reset=1,ce=0,compute_tick=0;integer wall=0,stage=0,pixel=0,resets=0,inputs=0,outputs=0,ctx=0,private_only=0,slow=0,coincident=0,fd;
reg[35:0]normal;reg[31:0]position;reg[3:0]mask;reg[4:0]slot;reg[1:0]lane;reg bank;
wire context_valid=pixel<96&&stage>=2;wire valid=pixel<96&&stage==3;
wire[2:0]oldrow={lane,stage==3};wire[35:0]olddata=stage==3?{4'd0,position}:normal;
wire ory,nry,ocw,ncw,oca,nca,opa,npa,ov,nv,ol,nl;wire[6:0]ok,nk;wire[15:0]oe,ne;wire[8:0]og,ng,oh,nh;
wire[15:0]epoch={15'd0,bank}+16'd3;
wire retire_valid=oca&&a.loaded_valid;wire retire_bank=a.loaded_bank;
old_light_consumer a(.clk(clk),.reset(reset),.ce(ce),.compute_tick(compute_tick),.source_valid(context_valid),.source_ready(ory),.source_row(oldrow),.source_data(olddata),.source_slot(slot),.source_mask(mask),.source_bank(bank),.retire_valid(retire_valid),.retire_bank(retire_bank),.context_epoch(epoch),.context_mode(2'd2),.context_code(5'd8),.light_x(16'sd0),.light_y(16'sd0),.light_z(16'sd16384),.ambient(9'd31),.directional(9'd193),.ray_x(16'sd16384),.ray_y(16'sd16384),.ray_k(16'sd16384),.context_wait(ocw),.context_accepted(oca),.pixel_accepted(opa),.out_valid(ov),.out_ready(1'b1),.out_key(ok),.out_quad_last(ol),.out_epoch(oe),.out_g(og),.out_h(oh));
gpu_v2_pixel_lighting_consumer b(.clk(clk),.reset(reset),.ce(ce),.compute_tick(compute_tick),.source_context_valid(context_valid),.source_valid(valid),.source_ready(nry),.source_lane(lane),.source_normal(normal),.source_position(position),.source_slot(slot),.source_mask(mask),.source_bank(bank),.retire_valid(retire_valid),.retire_bank(retire_bank),.context_epoch(epoch),.context_mode(2'd2),.context_code(5'd8),.light_x(16'sd0),.light_y(16'sd0),.light_z(16'sd16384),.ambient(9'd31),.directional(9'd193),.ray_x(16'sd16384),.ray_y(16'sd16384),.ray_k(16'sd16384),.context_wait(ncw),.context_accepted(nca),.pixel_accepted(npa),.out_valid(nv),.out_ready(1'b1),.out_key(nk),.out_quad_last(nl),.out_epoch(ne),.out_g(ng),.out_h(nh));
reg take,rebind;reg pa,pb;integer q;reg[11:0]nx,ny,nz;reg[15:0]xx,yy;
initial begin
 fd=$fopen("trace.txt","w");#1;clk=1;#1;clk=0;#1;
 for(wall=1;wall<20000;wall=wall+1)begin
  reset=@RESET@&&(wall==59||wall==211||wall==431);
  if(reset)begin stage=0;pixel=0;resets=resets+1;end
  ce=!reset&&(!@STRESS@||(wall%11!=3&&wall%11!=4&&!(wall>=125&&wall<146)));
  compute_tick=!reset&&(!@STRESS@||wall%7!=2);
  if(!@STRESS@)compute_tick=ce;
  q=pixel/4;slot=q%32;lane=pixel%4;bank=(pixel/12)%2;
  case(q%5)0:mask=0;1:mask=1;2:mask=4;3:mask=5;default:mask=15;endcase
  nx=pixel*151+7;ny=pixel*977-1001;nz=pixel*467+1024;
  normal={nz,ny,nx};xx=(pixel*421)%32768-16384;yy=16384-(pixel*613)%32768;position={yy,xx};
  #2;
  if(!reset)begin
   if({ocw,oca,opa}!=={ncw,nca,npa})$fatal(1,"public acceptance/context mismatch p%0d s%0d",pixel,stage);
   if(valid&&ory!==nry)$fatal(1,"pixel ready mismatch");
   if((a.core_in_valid&&a.core_in_ready)!==(b.core_in_valid&&b.core_in_ready))$fatal(1,"combinational core proposal differs");
   pa=compute_tick&&a.core_in_valid&&a.core_in_ready;pb=compute_tick&&b.core_in_valid&&b.core_in_ready;
   if(pa!==pb)$fatal(1,"actual core acceptance mismatch");
   if(pa)begin
    if({a.token,a.row0,a.row1,a.row2}!=={b.token,b.row0,b.row1,b.row2})$fatal(1,"actual operands differ");
    inputs=inputs+1;$fdisplay(fd,"I %0d %h %h %h %h",wall,a.token,a.row0,a.row1,a.row2);
   end
   if(ov!==nv)$fatal(1,"outvalid mismatch");
   if(ov)begin
    if({ok,ol,oe,og,oh}!=={nk,nl,ne,ng,nh})$fatal(1,"key/epoch/G/H differs");
    if(compute_tick)begin outputs=outputs+1;$fdisplay(fd,"O %0d %h %h %h %h",wall,ok,oe,og,oh);end
   end
   if(oca)ctx=ctx+1;if(ocw&&!oca)slow=slow+1;
   if(compute_tick&&!ce)private_only=private_only+1;
  end
  take=ce&&valid&&ory;rebind=retire_valid&&oca;
  if(rebind)coincident=coincident+1;
  clk=1;#2;clk=0;#1;
  if(rebind&&(!a.loaded_valid||!b.loaded_valid||a.loaded_bank!==bank||b.loaded_bank!==bank))$fatal(1,"same-edge retire beat context acceptance");
  if(!reset&&ce&&pixel<96)begin if(stage<3)stage=stage+1;else if(take)begin pixel=pixel+1;stage=0;end end
  if(pixel==96&&!(|a.core.valid_pipe)&&!(|b.core.valid_pipe))begin
   if(inputs<20||outputs<20||ctx<4||slow==0||coincident==0||(@STRESS@&&private_only==0)||(@RESET@&&resets!=3))$fatal(1,"coverage missing");
   $display("PASS Lighting tuple wall=%0d inputs=%0d outputs=%0d context=%0d slow=%0d privateCE=%0d reset=%0d sameRetire=%0d",wall,inputs,outputs,ctx,slow,private_only,resets,coincident);$fclose(fd);$finish;
  end
 end
 $fatal(1,"bounded timeout");
end
initial begin #200000;$fatal(1,"absolute timeout");end
endmodule
'''
if __name__=='__main__':main()
