"""Bounded lit command comparison across drained global replacements."""
from pathlib import Path
import argparse, hashlib, importlib.util, json, os, re, struct, subprocess, sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT/'ip/gpu-v2/examples/vliw_integration'))
from replay_command import CommandBuffer, target_params, texture_word

TB = r'''
`timescale 1ns/1ps
module tb;
@PORTS@
GSR GSR(.GSRI(1'b1));
always #5 clk=~clk;
gpu_v2_vliw_command dut(.*);
reg[63:0]memory[0:8191],image[0:163839];
integer cycle=0,ba=0,bi=0,bn=0,rb=0,ri=0,wb=0,wi=0,j,k,draws=0,lights=0,shades;
integer colored[0:3];reg active=0,ra=0,wa=0;
function integer image_index;
 input integer halfword;
 integer bytes;
 begin
  bytes=2*halfword;
  if(bytes>=32'h100000&&bytes<32'h23ee00)image_index=(bytes-32'h100000)/8;
  else begin $fatal(1,"global test target address %h",bytes);image_index=0;end
 end
endfunction
always @(negedge clk)begin
 ce=cycle%7!=2;ro_request_ready=!active&&cycle%5!=1;
 ro_response_valid=active&&cycle%3!=1;ro_response_last=bi==bn-1;ro_response_error=0;
 ro_data=memory[(ba>>2)+bi];
 fb_r_request_ready=!ra&&!wa&&cycle%5!=2;fb_r_response_valid=ra&&cycle%3!=2;
 fb_r_response_last=ri==15;fb_r_response_error=0;fb_r_data=ra?image[rb+ri]:0;
 fb_w_request_ready=!wa&&!ra&&cycle%4!=1;fb_w_data_ready=wa&&cycle%4!=2;
 fb_w_response_valid=wa&&wi==15&&fb_w_data_ready;fb_w_response_last=1;fb_w_response_error=0;
end
always @(posedge clk)begin
 cycle=cycle+1;if(cycle>20000)$fatal(1,"global comparison watchdog");
 if(!reset)begin
  if(ro_request_valid&&ro_request_ready)begin active=1;ba=ro_address;bi=0;bn=ro_count==3?16:ro_count==1?8:4;end
  else if(ro_response_valid)begin if(ro_response_last)active=0;else bi=bi+1;end
  if(fb_r_request_valid&&fb_r_request_ready)begin ra=1;rb=image_index(fb_r_address);ri=0;end
  else if(fb_r_response_valid)begin if(ri==15)ra=0;else ri=ri+1;end
  if(fb_w_request_valid&&fb_w_request_ready)begin wa=1;wb=image_index(fb_w_address);wi=0;end
  else if(wa&&fb_w_data_ready)begin image[wb+wi]=fb_w_data;wi=wi+1;if(wi==16)wa=0;end
  if(dut.start_fire)draws=draws+1;
  if(light_result)begin
   lights=lights+1;
   $fdisplay(shades,"%h %h %h",dut.core.back.lk,dut.core.back.lg,dut.core.back.lh);
  end
 end
end
task tick;begin @(negedge clk);#1;end endtask
initial begin
 $readmemh("memory.hex",memory);shades=$fopen("shade.txt","w");
 for(j=0;j<163840;j=j+1)image[j]=j>=131072?64'hffffffffffffffff:0;
 reset=1;output_ready=1;command_base=32'h1000;command_bytes=@BYTES@;
 tick;tick;reset=0;while(!submit_ready||!ce)tick;
 submit=1;@(posedge clk);#1;submit=0;
 while(!result_valid)tick;
 if(active||ra||wa||result_error||draws!=4||lights<3)$fatal(1,"global test conservation draws%0d lights%0d",draws,lights);
 for(k=0;k<4;k=k+1)begin
  colored[k]=0;
  for(j=0;j<24000;j=j+1)if(image[k*32768+j]!=0)colored[k]=colored[k]+1;
  if((k==2&&colored[k]!=0)||(k!=2&&colored[k]==0))$fatal(1,"far/lighting coverage case%0d words%0d",k,colored[k]);
  for(j=24000;j<32768;j=j+1)if(image[k*32768+j]!=0)$fatal(1,"plane guard changed");
 end
 $fclose(shades);$writememh("image.hex",image);
 $display("PASS DIRECT_GLOBAL cycles=%0d draws=%0d lights=%0d color_words=%0d/%0d/%0d/%0d",cycle,draws,lights,colored[0],colored[1],colored[2],colored[3]);
 $finish;
end
endmodule
'''


def main():
    p=argparse.ArgumentParser(__doc__)
    p.add_argument('--base',required=True,type=Path)
    p.add_argument('--candidate',required=True,type=Path)
    p.add_argument('--output',required=True,type=Path)
    args=p.parse_args();out=args.output.resolve();out.mkdir(parents=True,exist_ok=False)
    spec=importlib.util.spec_from_file_location('global_fixture',Path(__file__).with_name('vliw_clear.py'))
    fixture=importlib.util.module_from_spec(spec);spec.loader.exec_module(fixture)
    memory,put,records,_=fixture.assets(out)
    record,size=records[3];raw=bytes(memory[record:record+128+size])
    # Enlarge projected X/Y only: exercise overlapping lighting tokens while
    # retaining the fixture's unchanged normal transform and depth range.
    uniform=bytearray(memory[0x4280:0x4300])
    for word in range(8):
        struct.pack_into('<i',uniform,4*word,4*struct.unpack_from('<i',uniform,4*word)[0])
    put(0x4280,uniform)
    put(0x4100,struct.pack('<8I',texture_word(0x2000,4,True,0),0xffffff,7,0,0,0,0,0))
    # Full, different full, all far-rejected, and ambient-only contexts.
    contexts=[(199*65536, (0,-16384,0),(12000,16000,10000),32,192,3,0),
              (199*65536, (0,-11585,-11585),(-14000,8000,9000),80,160,3,4),
              (256, (0,16384,0),(9000,-14000,12000),64,192,3,16),
              (199*65536, (0,0,-16384),(-8000,-12000,11000),128,0,1,12)]
    cb=CommandBuffer();cb.emit('DMA',320,0x4280,128);cb.emit('DMA',280,0x4100,32)
    cb.emit('WAIT',a=3);cb.emit('SET_RENDER',280)
    for i,(far,light,ray,ambient,directional,mode,code) in enumerate(contexts):
        packed=[far,(light[0]&65535)|((light[1]&65535)<<16),
                (light[2]&65535)|((ray[0]&65535)<<16),
                (ray[1]&65535)|((ray[2]&65535)<<16),ambient|(directional<<16),mode|(code<<2)|((i+1)<<16)]
        ga=0x5000+i*64;ta=0x5400+i*32;ma=0xa000+i*0x500
        put(ga,struct.pack('<16I',*packed,*([0]*10)))
        put(ta,target_params(0x100000+i*0x40000,0x200000));put(ma,raw)
        cb.emit('DMA',272,ga,64);cb.emit('DMA',288,ta,32);cb.emit('WAIT',a=3)
        cb.emit('FLUSH');cb.emit('SET_TARGET',288);cb.emit('SET_GLOBAL',272)
        cb.emit('DMA_MESHLET',i%2,ma,size);cb.emit('WAIT',a=1);cb.emit('DRAW_MESHLET',i%2)
        cb.emit('WAIT',a=2);cb.emit('FLUSH')
    cb.emit('END');program=cb.encode();put(0x1000,program+bytes((-len(program))%128))
    sys.path.insert(0,str(ROOT/'ip/gpu-v2/examples/postquad_ii4'))
    from wrapper import wrapper
    results={}
    for label,path in [('base',args.base),('candidate',args.candidate)]:
        case=out/label;case.mkdir();source=path.resolve().read_bytes()
        (case/'dut.v').write_bytes(source)
        (case/'memory.hex').write_text(''.join(f'{int.from_bytes(memory[j:j+8],"little"):016x}\n' for j in range(0,len(memory),8)))
        _,ports=wrapper(re.sub(r'\b(input|output)(?=\[)',r'\1 ',source.decode()))
        declarations=[]
        for port in ports['ports']:
            declarations.append(('reg ' if port['direction']=='input' else 'wire ')
                +(f'[{port["bits"]-1}:0] ' if port['bits']>1 else '')+port['name']
                +('=0;' if port['direction']=='input' else ';'))
        (case/'tb.v').write_text(TB.replace('@PORTS@','\n'.join(declarations)).replace('@BYTES@',str(len(program))))
        commands=[('compile',[os.environ['IVERILOG_EXE'],'-g2012','-s','tb','-o',case/'sim.vvp',case/'dut.v',case/'tb.v',Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v']),
                  ('run',[os.environ['VVP_EXE'],case/'sim.vvp'])]
        for name,command in commands:
            with (case/(name+'.log')).open('wb') as log:
                result=subprocess.run(list(map(str,command)),cwd=case,stdout=log,stderr=subprocess.STDOUT,timeout=60)
            assert result.returncode==0,(case/(name+'.log')).read_text(errors='replace')[-2000:]
        results[label]=dict(export_sha256=hashlib.sha256(source).hexdigest(),
            cases=[l for l in (case/'run.log').read_text().splitlines() if l.startswith('PASS ')])
    for name in ['image.hex','shade.txt']:
        a=(out/'base'/name).read_bytes();b=(out/'candidate'/name).read_bytes()
        assert a==b,'Global borrowing changed '+name
        results[name+'_sha256']=hashlib.sha256(a).hexdigest()
    shading=[tuple(int(x,16) for x in line.split()) for line in (out/'base/shade.txt').read_text().splitlines()]
    assert len(shading)>8 and any(h for _,_,h in shading), 'Missing pipelined lighting/specular coverage'
    receipt=dict(status='pass',test_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),results=results)
    (out/'receipt.json').write_text(json.dumps(receipt,indent=2));print(json.dumps(receipt))


if __name__=='__main__':main()
