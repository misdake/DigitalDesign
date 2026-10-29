"""Independent fixed400x240 coordinate/packet and complete sampler validation."""
from pathlib import Path
import argparse
import hashlib
import importlib.util
import json
import os
import random
import re
import subprocess
import sys

sys.dont_write_bytecode=True
HERE=Path(__file__).resolve().parent


def load(path):
    spec=importlib.util.spec_from_file_location(path.stem,path)
    result=importlib.util.module_from_spec(spec);spec.loader.exec_module(result);return result


def run(command,directory,label,timeout=180):
    with (directory/(label+'.log')).open('wb') as output:
        result=subprocess.run(list(map(str,command)),cwd=directory,stdout=output,stderr=subprocess.STDOUT,timeout=timeout)
    assert result.returncode==0,(directory/(label+'.log')).read_text(errors='replace')[-2500:]


def clamp_uv(raw):
    raw=raw if raw<131072 else raw-262144
    return min(65535,max(0,raw))


def coordinate(raw,size,nearest,view):
    uv=clamp_uv(raw) if view else raw&65535
    q=uv*size//256-(0 if nearest else 128)
    if view:q=min((size-1)*256,max(0,q))
    return (q//256 if view else (q//256)&(size-1)),q%256


def code(x,y):
    return (((x*3+y*7)&31)<<11)|(((x*11+y*5)&63)<<5)|((x*13+y*3)&31)


def color(u,v,nearest):
    x,fx=coordinate(u,400,nearest,True);y,fy=coordinate(v,240,nearest,True)
    if nearest:weights=[511,0,0,0]
    else:
        bottom=511*fy//256;top=511-bottom;right=top*fx//256;br=bottom*fx//256
        weights=[top-right,right,bottom-br,br]
    channels=[0,0,0]
    for tap,weight in enumerate(weights):
        word=code(min(399,x+tap%2),min(239,y+tap//2))
        r,g,b=word>>11,(word>>5)&63,word&31
        rgb=[(r<<3)|(r>>2),(g<<2)|(g>>4),(b<<3)|(b>>2)]
        for c,value in enumerate(rgb):channels[c]+=(weight*value+256)//512
    return sum(min(255,c)<<(8*i) for i,c in enumerate(channels))


COORD_TB=r'''
`timescale 1ns/1ps
module tb;
reg clk=0,reset=1,ce=1,valid=0;always #5 clk=~clk;
reg[41:0]stim[0:@LAST@];reg[35:0]gold[0:@LAST@];
reg mode=0,nearest=0;reg[3:0]n=9;reg[17:0]u=0,v=0;
wire ready,ov,fault;wire[7:0]fx,fy;wire[9:0]x,y;
integer cycle=0,sent=0,got=0;
integer enabled_edges=0;integer due[0:@LAST@];
@CLAMP@
gpu_v2_texture_serial_coord dut(.clk(clk),.reset(reset),.ce(ce),.in_valid(valid),.in_ready(ready),.fault(fault),.out_valid(ov),
 .texture_fb400240(mode),.in_nearest(nearest),.in_size_log2(n),
 .in_wrapped_uv_0(mode?view_clamp_uv(u):u[15:0]),.in_wrapped_uv_1(mode?view_clamp_uv(v):v[15:0]),
 .out_f0_0(fx),.out_f0_1(fy),.out_t0_0_0(x),.out_t0_1_0(y));
always@(negedge clk)begin
 ce=cycle%7!=2&&cycle%13!=6;valid=!reset&&ready&&sent<@COUNT@;
 if(ready&&sent<@COUNT@){mode,n,nearest,u,v}=stim[sent];
end
always@(posedge clk)begin
 cycle=cycle+1;if(cycle>@MAX@)$fatal(1,"coord watchdog");
 if(!reset)begin
  if(fault)$fatal(1,"coord fault");
  if(ce)enabled_edges<=enabled_edges+1;
  if(ce&&valid&&ready)begin due[sent]=enabled_edges+2;sent<=sent+1;end
  if(ce&&ov)begin
   if(enabled_edges!=due[got])$fatal(1,"coord latency%0d got%0d due%0d",got,enabled_edges,due[got]);
   if({y,x,fy,fx}!==gold[got])$fatal(1,"coord%0d got%h gold%h",got,{y,x,fy,fx},gold[got]);
   got<=got+1;
  end
  if(got==@COUNT@)begin $display("PASS RTT_COORD vectors=%0d cycles=%0d",got,cycle);$finish;end
 end
end
initial begin $readmemh("stim.hex",stim);$readmemh("gold.hex",gold);repeat(3)@(negedge clk);reset=0;end
endmodule
'''


SAMPLER_TB=r'''
`timescale 1ns/1ps
module tb;
reg clk=0,reset=1;always #5 clk=~clk;GSR GSR(.GSRI(1'b1));
reg[35:0]stim[0:@LAST@];reg[23:0]gold[0:@LAST@];
reg[17:0]raw_u=0,raw_v=0;
integer cycle=0,sent=0,got=0,beat=0,requests=0,groups=0;
reg filling=0,ack=0;reg[31:0]fill_address=0;
wire enabled=cycle%17<13;wire ready,cv,cr,public_free,pv,pr,kv,kr,result;
wire cf,bf,kf,ofault;wire[37:0]owner;wire[35:0]coords;wire[34:0]packet;
wire next_valid;wire[14:0]next_key;wire ww;wire[6:0]wa;wire[35:0]wd;
wire[71:0]payload;wire[15:0]t0,t1,t2,t3;wire[5:0]key;wire[23:0]rgb;
wire request;wire[31:0]address;wire[3:0]quad=(sent/4)%16;wire[1:0]lane=sent%4;
wire source_valid=sent<@COUNT@&&(lane!=0||public_free);
@CLAMP@
always@(negedge clk)if(!reset&&ready&&sent<@COUNT@){raw_v,raw_u}=stim[sent];
gpu_v2_texture_direct_lod coordinate(.clk(clk),.reset(reset),.ce(enabled),.commit_allow(1'b1),
 .texture_fb400240(1'b1),.in_valid(source_valid),.in_ready(ready),.in_uv({view_clamp_uv(raw_v),view_clamp_uv(raw_u)}),
 .in_quad(quad),.in_mask(4'hf),.in_lane(lane),.in_lod(8'd0),.in_max_n(4'd9),.in_has_mip(1'b0),.in_filter(2'd@FILTER@),
 .out_valid(cv),.out_ready(cr),.out_owner(owner),.out_result(coords),.fault(cf));
gpu_v2_texture_runtime_backend_unique coefficients(.texture_base(16'h2000),.texture_mip(1'b0),.texture_fb400240(1'b1),
 .clk(clk),.reset(reset),.ce(enabled),.commit_allow(1'b1),.abort(1'b0),.in_valid(cv),.in_owner(owner),.in_result(coords),.in_ready(cr),
 .packet_enable(1'b1),.out_ready(pr),.out_valid(pv),.out_payload(packet),.next_valid(next_valid),.next_key(next_key),
 .weight_write(ww),.weight_address(wa),.weight_value(wd),.fault(bf));
function[15:0]word_at;input integer a;integer off,t,x,y;begin
 off=a-32'h100000;t=off/512;x=(t/16)*16+(off%32)/2;y=(t%16)*16+(off%512)/32;
 if(x<0||x>=400||y<0||y>=240)$fatal(1,"view physical range");
 word_at=(((x*3+y*7)%32)<<11)|(((x*11+y*5)%64)<<5)|((x*13+y*3)%32);
end endfunction
wire[63:0]memory_data={word_at(fill_address+beat*8+6),word_at(fill_address+beat*8+4),word_at(fill_address+beat*8+2),word_at(fill_address+beat*8)};
gpu_v2_texture_runtime_cache_unique cache(.clk(clk),.reset(reset),.proposal_ce(enabled),.commit_allow(1'b1),.abort(1'b0),
 .texture_invalidate(1'b0),.in_valid(pv),.in_payload(packet),.next_valid(next_valid),.next_key(next_key),
 .weight_write(ww),.weight_address(wa),.weight_value(wd),.in_ready(pr),.out_ready(kr),.out_valid(kv),.out_payload(payload),
 .out_tex0(t0),.out_tex1(t1),.out_tex2(t2),.out_tex3(t3),.mem_req_valid(request),.mem_req_addr(address),
 .mem_req_ready(!filling&&!ack&&cycle%5!=1),.mem_resp_valid(filling&&cycle%3!=1),.mem_resp_index(beat[3:0]),
 .mem_resp_data(memory_data),.mem_resp_complete(ack),.mem_resp_ok(1'b1),.fault(kf));
gpu_v2_texture_runtime_color color(.clk(clk),.reset(reset),.proposal_ce(enabled),.commit_allow(1'b1),.check_quad(quad),.public_free(public_free),
 .publish_valid(enabled&&source_valid&&ready&&lane==0),.publish_quad(quad),.publish_mask(4'hf),
 .in_valid(kv),.in_payload(payload),.in_texels({t3,t2,t1,t0}),.in_ready(kr),
 .out_ready(1'b1),.out_valid(result),.out_key(key),.out_rgb(rgb),.fault(ofault));
always@(posedge clk)begin
 cycle<=cycle+1;if(cycle>@MAX@)$fatal(1,"sampler watchdog");
 if(!reset)begin
  if(cf||bf||kf||ofault)$fatal(1,"sampler fault");
  ack<=0;
  if(request&&!filling&&!ack&&cycle%5!=1)begin
   if(address<32'h100000||address>=32'h132000||address[6:0]!=0)$fatal(1,"refill address %h",address);
   filling<=1;fill_address<=address;beat<=0;requests<=requests+1;
  end
  if(filling&&cycle%3!=1)begin if(beat==15)begin filling<=0;ack<=1;end else beat<=beat+1;end
  if(cache.read_issue)groups<=groups+1;
  if(enabled&&source_valid&&ready)sent<=sent+1;
  if(enabled&&result)begin
   if(key!==got%64||rgb!==gold[got])$fatal(1,"sampler%0d key%h rgb%h gold%h",got,key,rgb,gold[got]);
   got<=got+1;
  end
  if(got==@COUNT@)begin
   if(sent!=got||requests%2)$fatal(1,"sampler conservation");
   $display("PASS RTT_SAMPLER pixels=%0d filter=@FILTER@ groups=%0d sectors=%0d cycles=%0d",got,groups,requests,cycle);$finish;
  end
 end
end
initial begin $readmemh("stim.hex",stim);$readmemh("gold.hex",gold);repeat(4)@(negedge clk);{raw_v,raw_u}=stim[0];reset=0;end
endmodule
'''


def main():
    ap=argparse.ArgumentParser(__doc__);ap.add_argument('--export',type=Path,required=True);ap.add_argument('--output',type=Path,required=True)
    ap.add_argument('--skip-sampler',action='store_true');ap.add_argument('--exhaustive-axis',action='store_true');args=ap.parse_args()
    source=args.export.resolve();out=args.output.resolve();out.mkdir(parents=True,exist_ok=False);text=source.read_text()
    clamp=re.search(r' function\[15:0\]view_clamp_uv;.*?endfunction',text,re.S)[0]
    rng=random.Random(400240);rows=[]
    # Every logical texel, both filters, plus signed/out-of-range and POT cases.
    for nearest in (0,1):
        if args.exhaustive_axis:
            for u in range(65536):
                v=65535-u
                xx,fx=coordinate(u,400,nearest,True);yy,fy=coordinate(v,240,nearest,True)
                rows.append(((1<<41)|(9<<37)|(nearest<<36)|(u<<18)|v,(yy<<26)|(xx<<16)|(fy<<8)|fx))
        for y in range(240):
            for x in range(400):
                u=round((x+.5)*65536/400);v=round((y+.5)*65536/240)
                xx,fx=coordinate(u,400,nearest,True);yy,fy=coordinate(v,240,nearest,True)
                rows.append(((1<<41)|(9<<37)|(nearest<<36)|(u<<18)|v,(yy<<26)|(xx<<16)|(fy<<8)|fx))
        for _ in range(1024):
            u,v=rng.randrange(262144),rng.randrange(262144)
            xx,fx=coordinate(u,400,nearest,True);yy,fy=coordinate(v,240,nearest,True)
            rows.append(((1<<41)|(9<<37)|(nearest<<36)|(u<<18)|v,(yy<<26)|(xx<<16)|(fy<<8)|fx))
        for n in range(1,11):
            for _ in range(64):
                u,v=rng.randrange(262144),rng.randrange(262144)
                xx,fx=coordinate(u,1<<n,nearest,False);yy,fy=coordinate(v,1<<n,nearest,False)
                rows.append(((n<<37)|(nearest<<36)|(u<<18)|v,(yy<<26)|(xx<<16)|(fy<<8)|fx))
    directory=out/'coordinate';directory.mkdir()
    (directory/'stim.hex').write_text(''.join(f'{a:011x}\n' for a,b in rows))
    (directory/'gold.hex').write_text(''.join(f'{b:09x}\n' for a,b in rows))
    tb=COORD_TB.replace('@CLAMP@',clamp).replace('@LAST@',str(len(rows)-1)).replace('@COUNT@',str(len(rows))).replace('@MAX@',str(len(rows)*5+1000))
    (directory/'tb.v').write_text(tb)
    run([os.environ['IVERILOG_EXE'],'-g2012','-s','tb','-o','sim.vvp',source,'tb.v'],directory,'compile')
    run([os.environ['VVP_EXE'],'sim.vvp'],directory,'run')
    cases=[dict(kind='coordinate',vectors=len(rows),summary=[l for l in (directory/'run.log').read_text().splitlines() if l.startswith('PASS ')])]
    # Packet addresses use integer multiplication/addition as an independent
    # reference, including every column macro, half-line and visible image boundary.
    packet=load(HERE/'texture_packet.py');vectors=[]
    for y in range(240):
        for x in range(400):
            cross_x=x%16==15 and x<399;cross_y=y%8==7 and y<239
            active=1|(2 if cross_x else 0)|(4 if cross_y else 0)|(8 if cross_x and cross_y else 0)
            for tap in range(4):
                if not(active>>tap&1):continue
                first,last=1,int(active>>(tap+1)==0);quad=(x+y)%16;lane=x%4
                member=(active<<36)|((x//16)<<40)|((y//8)<<46)|((x%16)<<53)|((y%8)<<57)|(9<<64)|(quad<<68)|(lane<<72)|(1<<74)|(1<<75)
                xx=min(399,x+tap%2);yy=min(239,y+tap//2)
                half=(0x100000+512*((xx//16)*16+yy//16)+256*((yy%16)//8))//256
                sx=x%16!=15;sy=y%8!=7
                mask=sum(int((tap&1 or sx) if i&1 else (not(tap&1) or sx))*int((tap>>1 or sy) if i&2 else (not(tap>>1) or sy))<<i for i in range(4))
                owner=(lane<<6)|(quad<<2)|(last<<1)|int(tap==0)
                result=half|((x%16)<<15)|((y%8)<<19)|(owner<<22)|(1<<30)|(mask<<31)
                vectors.append((member|(tap<<76)|(0x2000<<78),result))
    directory=out/'packet';directory.mkdir()
    (directory/'stim.hex').write_text(''.join(f'{a:024x}\n' for a,b in vectors));(directory/'expected.hex').write_text(''.join(f'{b:09x}\n' for a,b in vectors))
    tb=packet.TB.replace('.texture_base(base),.texture_mip(mip),','.texture_base(base),.texture_mip(mip),.texture_fb400240(1\'b1),')
    tb=tb.replace('@LAST@',str(len(vectors)-1)).replace('@COUNT@',str(len(vectors))).replace('cycle>3000',f'cycle>{len(vectors)*3+1000}')
    (directory/'tb.v').write_text(tb)
    run([os.environ['IVERILOG_EXE'],'-g2012','-s','tb','-o','sim.vvp',source,'tb.v'],directory,'compile');run([os.environ['VVP_EXE'],'sim.vvp'],directory,'run')
    cases.append(dict(kind='packet',vectors=len(vectors),summary=[l for l in (directory/'run.log').read_text().splitlines() if l.startswith('PASS ')]))
    if not args.skip_sampler:
        values=[(round((x+.37)*65536/400),round((y+.61)*65536/240)) for y in range(240) for x in range(400)]
        values += [(u&262143,v&262143) for u,v in [(-65536,-1),(0,0),(65536,65536),(131071,131071)]*16]
        for nearest in (0,1):
            directory=out/f'sampler-{nearest}';directory.mkdir()
            (directory/'stim.hex').write_text(''.join(f'{(v<<18)|u:09x}\n' for u,v in values))
            (directory/'gold.hex').write_text(''.join(f'{color(u,v,nearest):06x}\n' for u,v in values))
            tb=SAMPLER_TB.replace('@CLAMP@',clamp).replace('@LAST@',str(len(values)-1)).replace('@COUNT@',str(len(values))).replace('@MAX@',str(len(values)*50)).replace('@FILTER@',str(0 if nearest else 1))
            # Avoid querying the model at address0 before the first descriptor.
            tb=tb.replace('reg[31:0]fill_address=0;','reg[31:0]fill_address=32\'h100000;')
            (directory/'tb.v').write_text(tb)
            run([os.environ['IVERILOG_EXE'],'-g2012','-s','tb','-o','sim.vvp',source,'tb.v',Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v'],directory,'compile')
            run([os.environ['VVP_EXE'],'sim.vvp'],directory,'run',600)
            cases.append(dict(kind='sampler',nearest=nearest,pixels=len(values),summary=[l for l in (directory/'run.log').read_text().splitlines() if l.startswith('PASS ')]))
    receipt=dict(status='pass',export_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),cases=cases)
    (out/'receipt.json').write_text(json.dumps(receipt,indent=2));print(json.dumps(receipt,indent=2))


if __name__=='__main__':main()
