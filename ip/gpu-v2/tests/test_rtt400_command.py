"""Real CP/R/I/FB producer -> flush -> texture consumer, shared DRAM, no CPU pixel copy."""
from pathlib import Path
import argparse
import copy
import hashlib
import importlib.util
import json
import os
import re
import struct
import subprocess
import sys

sys.dont_write_bytecode=True
ROOT=Path(__file__).resolve().parents[3]
HERE=ROOT/'ip/gpu-v2/examples/vliw_integration'
sys.path.insert(0,str(HERE))
from replay_command import CommandBuffer,texture_word,target_params


def load(name,path):
    spec=importlib.util.spec_from_file_location(name,path);module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module);return module


TB=r'''
`timescale 1ns/1ps
module tb;
@PORTS@
reg[31:0]command_base=0,command_bytes=0;
GSR GSR(.GSRI(1'b1));always #5 clk=~clk;
gpu_v2_vliw_command dut(.*);
reg[63:0]memory[0:8191],image[0:98303];
reg coverage[0:95999];
reg active=0,ra=0,wa=0;
integer cycle=0,mode=0,ba=0,bi=0,bn=0,rb=0,ri=0,wb=0,wi=0,j,k;
integer starts=0,flushes=0,invalidates=0,texture_reads=0,changed=0,pixels=-1,line_misses=0,line_fills=0;
integer checked_base=0,pixel_x=0,pixel_y=0,byte_offset=0;reg[15:0]expected,pixel_expected;
function integer image_index;input integer halfword;begin
 if(halfword>=22'h80000&&halfword<22'ha0000)image_index=halfword/4-131072;
 else if(halfword>=22'ha0000&&halfword<22'hc0000)image_index=32768+halfword/4-163840;
 else if(halfword>=22'hc0000&&halfword<22'he0000)image_index=65536+halfword/4-196608;
 else begin $fatal(1,"image address%h",halfword);image_index=0;end
end endfunction
wire texture_region=(ba>=22'h80000&&ba<22'ha0000)||(ba>=22'hc0000&&ba<22'he0000);
always@(negedge clk)begin
 ce=cycle%7!=2;ro_request_ready=!active&&cycle%5!=1;
 ro_response_valid=active&&cycle%3!=1;ro_response_last=bi==bn-1;ro_response_error=0;
 ro_data=active&&texture_region?image[image_index(ba)+bi]:memory[(ba>>2)+bi];
 fb_r_request_ready=!ra&&!wa;fb_r_response_valid=ra&&cycle%3!=2;
 fb_r_response_last=ri==15;fb_r_response_error=0;fb_r_data=ra?image[rb+ri]:0;
 fb_w_request_ready=!wa&&!ra;fb_w_data_ready=wa&&cycle%4!=2;
 fb_w_response_valid=wa&&wi==15&&fb_w_data_ready;fb_w_response_last=1;fb_w_response_error=0;
end
always@(posedge clk)begin
 cycle=cycle+1;if(cycle>100000)$fatal(1,"RTT command watchdog");
 if(!reset)begin
  if(ro_request_valid&&ro_request_ready)begin
   active=1;ba=ro_address;bi=0;bn=ro_count==3?16:ro_count==1?8:4;
   if((ba>=22'h80000&&ba<22'ha0000)||(ba>=22'hc0000&&ba<22'he0000))texture_reads=texture_reads+1;
  end else if(ro_response_valid)begin if(ro_response_last)active=0;else bi=bi+1;end
  if(fb_r_request_valid&&fb_r_request_ready)begin ra=1;rb=image_index(fb_r_address);ri=0;end
  else if(fb_r_response_valid)begin if(ri==15)ra=0;else ri=ri+1;end
  if(fb_w_request_valid&&fb_w_request_ready)begin wa=1;wb=image_index(fb_w_address);wi=0;end
  else if(wa&&fb_w_data_ready)begin image[wb+wi]=fb_w_data;wi=wi+1;if(wi==16)wa=0;end
  if(dut.core.back.front.demand.tag_write)line_misses=line_misses+1;
  if(dut.core.back.front.demand.publish)line_fills=line_fills+1;
  if(dut.texture_invalidate)invalidates=invalidates+1;
  if(dut.start_fire)begin
   if(dut.texture_fb400240!==starts[0])$fatal(1,"view install/switch");
   if(starts[0]&&(dut.texture_n!==9||dut.texture_mip||dut.texture_filter!==2'd@FILTER@||
      dut.texture_base!==(starts==5?16'h3000:16'h2000)))$fatal(1,"view descriptor");
   starts=starts+1;
  end
  if(ce&&dut.flush_request&&dut.flush_complete&&starts>0)begin
   flushes=flushes+1;changed=0;
   case(flushes)
    1:begin checked_base=0;expected=16'hf800;end
    2:begin checked_base=65536;expected=16'hf800;end
    3:begin checked_base=0;expected=16'h07e0;end
    4:begin checked_base=65536;expected=mode==1?16'hf800:16'h07e0;end
    5:begin checked_base=65536;expected=16'h001f;end
    6:begin checked_base=0;expected=16'h001f;end
    default:$fatal(1,"extra flush");
   endcase
   for(j=0;j<25600;j=j+1)for(k=0;k<4;k=k+1)begin
    byte_offset=j*8+k*2;pixel_x=(byte_offset/8192)*16+(byte_offset%32)/2;
    pixel_y=((byte_offset/512)%16)*16+(byte_offset%512)/32;
    pixel_expected=(pixel_y<240&&coverage[pixel_y*400+pixel_x])?expected:16'd0;
    if(image[checked_base+j][k*16+:16]!=0)changed=changed+1;
    if(image[checked_base+j][k*16+:16]!==pixel_expected)$fatal(1,"RTT color flush%0d xy=%0d,%0d got%h expected%h",flushes,pixel_x,pixel_y,image[checked_base+j][k*16+:16],pixel_expected);
   end
   if(pixels<0)pixels=changed;
   if(pixels!=@PIXELS@||changed!=(expected==0?0:pixels))$fatal(1,"RTT coverage conservation");
  end
 end
end
task tick;begin @(negedge clk);#1;end endtask
initial begin
 j=$value$plusargs("MODE=%d",mode);$readmemh("memory.hex",memory);$readmemh("coverage.hex",coverage);
 for(j=0;j<98304;j=j+1)image[j]=(j>=32768&&j<65536)?64'hffffffffffffffff:0;
 if(mode==2)memory[16'h4140/8]=memory[16'h4140/8]|64'h100000;
 if(mode==4)memory[16'h4140/8]=(memory[16'h4140/8]&~64'h600000)|64'h400000;
 if(mode==5)memory[16'h4140/8]=(memory[16'h4140/8]&~64'hf0000)|64'h80000;
 if(mode==6)memory[16'h4140/8]=memory[16'h4140/8]|64'h1;
 command_base=mode==1?32'h1800:mode==3?32'h2000:32'h1000;
 command_bytes=mode==1?@STALE_BYTES@:mode==3?@NOFLUSH_BYTES@:@BYTES@;
 reset=1;output_ready=1;tick;tick;reset=0;submit_commands(command_base,command_bytes);
 while(busy)tick;
 if(device_read_data[1]||fault||active||ra||wa||starts!=6||flushes!=6||invalidates!=(mode==1?2:3)||
    texture_reads!=line_misses*2||line_misses!=line_fills)$fatal(1,"RTT conservation");
 for(j=32768;j<32768+25600;j=j+1)if(image[j]!==64'hffffffffffffffff)$fatal(1,"depth changed");
 $display("PASS RTT_COMMAND mode=%0d filter=@FILTER@ pixels=%0d draws=%0d flushes=%0d invalidates=%0d line_misses=%0d sectors=%0d cycles=%0d",mode,pixels,starts,flushes,invalidates,line_misses,texture_reads,cycle);$finish;
end
endmodule
'''


def main():
    ap=argparse.ArgumentParser(__doc__);ap.add_argument('--export',type=Path,required=True);ap.add_argument('--output',type=Path,required=True);ap.add_argument('--filter',type=int,choices=(0,1),default=0);ap.add_argument('--tint',type=int,nargs=3,default=[255,255,255]);ap.add_argument('--alpha',type=int,default=255);args=ap.parse_args();assert all(0<=v<=255 for v in [*args.tint,args.alpha])
    out=args.output.resolve();out.mkdir(parents=True,exist_ok=False)
    fixture=load('rtt_clear_fixture',HERE.parents[1]/'tests/vliw_clear.py')
    memory,put,records,_=fixture.assets(out)
    original=records[3];size=original[1]
    source_record=bytes(memory[original[0]:original[0]+128+size])
    oracle=load('rtt_geometry_oracle',HERE/'reference/scene/oracle.py')
    oracle=load('rtt_transport',HERE/'transport.py').adapt_oracle(oracle)
    instance=copy.deepcopy(json.loads((HERE/'reference/uv-inputs.json').read_text())['scenes'][0]['meshlets'][1])
    indices=instance['triangles'][0];instance['vertices']=[instance['vertices'][i] for i in indices]
    # Retain real packed geometry, shrinking this4035-pixel face to a small
    # triangle with a3x3 interior so both filters can sample GPU-written color.
    anchor=[(instance['vertices'][0][0]>>(10*k))&1023 for k in range(3)]
    for vertex in instance['vertices'][1:]:
        xyz=[(vertex[0]>>(10*k))&1023 for k in range(3)]
        vertex[0]=sum((a+(b-a)//8)<<(10*k) for k,(a,b) in enumerate(zip(anchor,xyz)))
    instance.update(slot=1,triangles=[[0,1,2]],color_mode=1,cull_negative=1,generation=3,seq_base=3)
    for vertex in instance['vertices']:vertex[2]=0xe07030
    meta,packets=oracle.prepare(instance,out/'pick-trial')
    if not packets:instance['triangles']=[[0,2,1]]
    meta,packets=oracle.prepare(instance,out/'pick')
    image={int(a):int(v,16) for a,v in (line.split() for line in (out/'pick/dma.txt').read_text().splitlines())}
    rows=[a-128 for a in image if 128<=a<256]
    raw=b''.join(struct.pack('<Q',image.get(128+j,0)) for j in range(max(rows)+1))
    size=(len(raw)+127)//128*128
    header=b''.join(struct.pack('<Q',image[260+j]) for j in range(4))
    source_record=header+bytes(96)+raw+bytes(size-len(raw))
    uniform=bytearray(b''.join(struct.pack('<Q',image[j]) for j in range(320,336)))
    struct.pack_into('<II',uniform,88,0xffffffff,0);put(0x4280,uniform)
    uniform_tinted=bytearray(uniform);struct.pack_into('<I',uniform_tinted,88,int.from_bytes(bytes([*args.tint,args.alpha]),'little'));put(0x4600,uniform_tinted)
    pixels=set()
    for packet in packets:
        for qi in packet['masks']:pixels.update(oracle.expected_quad(packet,qi)['depth'])
    (out/'coverage.hex').write_text(''.join(f'{int((x,y) in pixels)}\n' for y in range(240) for x in range(400)))
    point=max(pixels,key=lambda p:sum((p[0]+dx,p[1]+dy) in pixels for dx in (-1,0,1) for dy in (-1,0,1)))
    score=sum((point[0]+dx,point[1]+dy) in pixels for dx in (-1,0,1) for dy in (-1,0,1));assert score==9,(point,score)
    uv=round((point[0]+.5)*65536/400)|(round((point[1]+.5)*65536/240)<<16)
    addresses=[0xa000,0xa800,0xb000,0xb800]
    for address,attribute,rgb in zip(addresses,[0xff,0xff00,uv,0xff0000],[True,True,False,True]):
        record=bytearray(source_record)
        if not rgb:struct.pack_into('<Q',record,8,struct.unpack_from('<Q',record,8)[0]&~16)
        for i in range(3):struct.pack_into('<I',record,128+i*12+8,attribute)
        put(address,record)
    put(0x4100,struct.pack('<8I',texture_word(0x1000,6,False,0),0,7|(1<<5),0,0,0,0,0))
    for address,base in [(0x4140,0x100000),(0x4180,0x180000)]:
        word=texture_word(base,9,False,args.filter)|(1<<23)
        put(address,struct.pack('<8I',word,0,7|(1<<5)|(int(args.alpha<255)<<4),0,0,0,0,0))
    put(0x4460,target_params(0x100000,0x140000));put(0x44a0,target_params(0x180000,0x140000))
    def program(stale=False,no_flush=False):
        cb=CommandBuffer();cb.emit('WAIT',a=3);cb.emit('FLUSH')
        cb.emit('DMA',272,0x4000,64);cb.emit('DMA',320,0x4280,128);cb.emit('WAIT',a=3);cb.emit('SET_GLOBAL',272)
        for stage,(target,render,record) in enumerate([(0x4460,0x4100,0xa000),(0x44a0,0x4140,0xb000),(0x4460,0x4100,0xa800),(0x44a0,0x4140,0xb000),(0x44a0,0x4100,0xb800),(0x4460,0x4180,0xb000)]):
            cb.emit('DMA',320,0x4600 if stage%2 else 0x4280,128);cb.emit('DMA',288,target,32);cb.emit('DMA',280,render,32);cb.emit('WAIT',a=3);cb.emit('SET_TARGET',288);cb.emit('SET_RENDER',280)
            if stage%2 and not(stale and stage==3):cb.emit('TEX_INVALIDATE')
            cb.emit('DMA_MESHLET',0,record,size);cb.emit('WAIT',a=1);cb.emit('DRAW_MESHLET',0);cb.emit('WAIT',a=3)
            if not(no_flush and stage==0):cb.emit('FLUSH')
        cb.emit('END');return cb.encode()
    buffers=[program(),program(stale=True),program(no_flush=True)]
    for address,data in zip((0x1000,0x1800,0x2000),buffers):put(address,data+bytes((-len(data))%128))
    (out/'memory.hex').write_text(''.join(f'{int.from_bytes(memory[i:i+8],"little"):016x}\n' for i in range(0,len(memory),8)))
    source=args.export.resolve();source_bytes=source.read_bytes();source_hash=hashlib.sha256(source_bytes).hexdigest()
    (out/'dut.v').write_bytes(source_bytes)
    wrapper=load('rtt_wrapper',HERE.parents[0]/'postquad_ii4/wrapper.py')
    _,ports=wrapper.wrapper(re.sub(r'\b(input|output)(?=\[)',r'\1 ',source.read_text()))
    declarations=[]
    for p in ports['ports']:
        width=f'[{p["bits"]-1}:0]' if p['bits']>1 else ''
        declarations.append(('reg ' if p['direction']=='input' else 'wire ')+width+' '+p['name']+('=0;' if p['direction']=='input' else ';'))
    tb=TB.replace('@PORTS@','\n'.join(declarations)).replace('@FILTER@',str(args.filter)).replace('@PIXELS@',str(len(pixels)))
    def expand(word):
        r,g,b=word>>11,(word>>5)&63,word&31
        return [(r<<3)|(r>>2),(g<<2)|(g>>4),(b<<3)|(b>>2)]
    def mix(src,dst):
        a=(args.alpha+2)//4 if args.alpha<255 else 64
        channels=[]
        for value,tint,previous in zip(expand(src),args.tint,expand(dst)):
            value=(value*tint+127)//255
            channels.append((64*previous+(value-previous)*a+32)//64)
        return sum(((v*n+127)//255)<<shift for v,n,shift in zip(channels,[31,63,31],[11,5,0]))
    first=mix(0xf800,0)
    normal=mix(0x07e0,first);stale=mix(0xf800,first);last=mix(0x001f,0x07e0)
    tb=tb.replace("2:begin checked_base=65536;expected=16'hf800;end",f"2:begin checked_base=65536;expected=16'h{first:04x};end")
    tb=tb.replace("4:begin checked_base=65536;expected=mode==1?16'hf800:16'h07e0;end",f"4:begin checked_base=65536;expected=mode==1?16'h{stale:04x}:16'h{normal:04x};end")
    tb=tb.replace("6:begin checked_base=0;expected=16'h001f;end",f"6:begin checked_base=0;expected=16'h{last:04x};end")
    for token,data in zip(('@BYTES@','@STALE_BYTES@','@NOFLUSH_BYTES@'),buffers):tb=tb.replace(token,str(len(data)))
    tb=load('rtt_device_bus',HERE/'device_bus.py').bench(tb);(out/'tb.v').write_text(tb)
    compile_command=[os.environ['IVERILOG_EXE'],'-g2012','-s','tb','-o',out/'sim.vvp',out/'dut.v',out/'tb.v',Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v']
    with (out/'compile.log').open('wb') as log:r=subprocess.run(list(map(str,compile_command)),cwd=out,stdout=log,stderr=subprocess.STDOUT,timeout=180)
    assert r.returncode==0,(out/'compile.log').read_text(errors='replace')[-2000:]
    cases=[]
    for mode in (0,1,2,3,4,5,6):
        with (out/f'mode{mode}.log').open('wb') as log:r=subprocess.run([os.environ['VVP_EXE'],str(out/'sim.vvp'),f'+MODE={mode}'],cwd=out,stdout=log,stderr=subprocess.STDOUT,timeout=180)
        text=(out/f'mode{mode}.log').read_text(errors='replace')
        if mode in (0,1):assert r.returncode==0,text[-2500:]
        else:
            expected='CP target drain contract' if mode==3 else 'CP state data contract'
            assert r.returncode!=0 and expected in text,text[-2500:]
        cases.append(dict(mode=mode,status='pass' if mode<2 else 'expected_diagnostic',summary=[l for l in text.splitlines() if l.startswith('PASS ')]))
    assert hashlib.sha256(source.read_bytes()).hexdigest()==source_hash
    receipt=dict(status='pass',filter=args.filter,tint=args.tint,alpha=args.alpha,export_sha256=source_hash,covered_pixels=len(pixels),sample_point=point,cases=cases,
                 inputs_sha256={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in [Path(__file__),out/'tb.v',out/'memory.hex',out/'coverage.hex',out/'dut.v']},
                 boundary='Real command/geometry/R/I/Sampling/ROP/cache with a stalled64-bit shared DRAM peer; initialization only before reset, every later image write comes from GPU FB writeback; no CPU pixel copying, no physical MC timing or board claim.')
    (out/'receipt.json').write_text(json.dumps(receipt,indent=2));print(json.dumps(receipt,indent=2))


if __name__=='__main__':main()
