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
localparam INTERRUPT=@INTERRUPT@;
@PORTS@
GSR GSR(.GSRI(1'b1));
always #5 clk=~clk;
gpu_v2_vliw_command dut(.*);
reg[63:0]memory[0:8191],image[0:163839];
integer cycle=0,ba=0,bi=0,bn=0,rb=0,ri=0,wb=0,wi=0,j,k,draws=0,lights=0,shades;
integer trace_fd,inputs_fd;
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
 output_ready=cycle%11!=4;
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
 if(reset)begin active=0;ra=0;wa=0;draws=0;lights=0;end
 if(!reset)begin
  $fdisplay(trace_fd,"%0d %b %b %b %h %h %h %h %b %h %h %b %b %b %b %b %b %b %b %h %h",
   cycle,ce,busy,pipeline_idle,dut.processor.cfg_far_z,dut.processor.context_epoch,
   dut.processor.context_mode,global_used,dut.core.back.front.light.core.ce,
   dut.core.back.front.light.core.phase,dut.core.back.front.light.core.valid_pipe,
   dut.core.back.front.light.pixel_accepted,light_result,final_issue,final_result,
   fb_r_request_valid,fb_r_request_ready,fb_w_request_valid,fb_w_request_ready,
   fb_w_address,fb_w_data);
  if(dut.core.back.front.light.pixel_accepted)$fdisplay(inputs_fd,
   "%h %h %h %h %h %h %h %h %h %h %h %h %h %h %h",
   dut.processor.context_epoch,dut.processor.context_mode,dut.processor.context_code,
   dut.processor.ambient,dut.processor.directional,dut.processor.light_x,
   dut.processor.light_y,dut.processor.light_z,dut.processor.ray_x,
   dut.processor.ray_y,dut.processor.ray_k,dut.core.back.front.light.core.in_row0,
   dut.core.back.front.light.core.in_row1,dut.core.back.front.light.core.in_row2,
   dut.core.back.front.light.core.in_id);
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
task send;input[3:0]channel;input[15:0]data;begin
 device_index=4;device_channel=channel;device_write_data=data;device_write_enable=1;
 @(posedge clk);#1;device_write_enable=0;tick;
end endtask
initial begin
 $readmemh("memory.hex",memory);shades=$fopen("shade.txt","w");trace_fd=$fopen("trace.txt","w");inputs_fd=$fopen("inputs.txt","w");
 for(j=0;j<163840;j=j+1)image[j]=j>=131072?64'hffffffffffffffff:0;
 reset=1;output_ready=1;
 tick;tick;reset=0;tick;
 if(busy||fault)$fatal(1,"GPU not ready after reset");
 send(0,(@BYTES@/4)-1);send(1,16'h0020);
 if(!busy)$fatal(1,"device SEND did not start command stream");
 if(INTERRUPT!=0)begin
  while(!(dut.core.back.front.light.core.valid_pipe!=0))tick;
  if(INTERRUPT==2)begin
   abort=1;repeat(32)tick;
   while(active||ra||wa)tick;
   if(!fault||dut.core.back.front.light.core.ce!==0)$fatal(1,"abort did not stop compute and drain owned memory");
  end
  // System reset also resets the external memory transaction model.
  reset=1;repeat(3)tick;abort=0;reset=0;tick;
  send(0,(@BYTES@/4)-1);send(1,16'h0020);
  if(!busy)$fatal(1,"reset did not admit the next command stream");
 end
 while(busy)tick;
 if(active||ra||wa||fault||draws!=4||lights<3)$fatal(1,"global test conservation draws%0d lights%0d",draws,lights);
 for(k=0;k<4;k=k+1)begin
  colored[k]=0;
  for(j=0;j<24000;j=j+1)if(image[k*32768+j]!=0)colored[k]=colored[k]+1;
  if((k==2&&colored[k]!=0)||(k!=2&&colored[k]==0))$fatal(1,"far/lighting coverage case%0d words%0d",k,colored[k]);
  for(j=24000;j<32768;j=j+1)if(image[k*32768+j]!=0)$fatal(1,"plane guard changed");
 end
 $fclose(shades);$fclose(trace_fd);$fclose(inputs_fd);$writememh("image.hex",image);
 $display("PASS DIRECT_GLOBAL cycles=%0d draws=%0d lights=%0d color_words=%0d/%0d/%0d/%0d",cycle,draws,lights,colored[0],colored[1],colored[2],colored[3]);
 $finish;
end
endmodule
'''


def main():
    p=argparse.ArgumentParser(__doc__)
    p.add_argument('--base',required=True,type=Path)
    p.add_argument('--candidate',type=Path)
    p.add_argument('--backend',choices=['native','portable'],default='native')
    p.add_argument('--interrupt',choices=['none','reset','abort-reset'],default='none')
    p.add_argument('--synthesis',action='store_true')
    p.add_argument('--projection-fixture',choices=['original','oracle-domain'],default='original')
    p.add_argument('--reuse-base',type=Path)
    p.add_argument('--mutation',choices=['far','clock'],help='expect a real candidate data/clock mutation to fail with production monitors off')
    p.add_argument('--early-global',action='store_true',help='expect unchanged CP monitor to reject SET_GLOBAL without WAIT')
    p.add_argument('--output',required=True,type=Path)
    args=p.parse_args()
    if args.early_global and (args.synthesis or args.interrupt!='none' or args.reuse_base):
        p.error('early-global requires monitors, no interrupt and a fresh baseline')
    if args.mutation and (not args.candidate or not args.synthesis or args.early_global or args.interrupt!='none'):
        p.error('mutation requires a candidate, SYNTHESIS, no early-global and no interrupt')
    out=args.output.resolve();out.mkdir(parents=True,exist_ok=False)
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
    # Original raw signed16 inputs remain a DUT-equivalence case. The standalone
    # numerical oracle has a narrower projection domain; use a separate family,
    # not a weaker oracle or a replacement for the original inputs.
    if args.projection_fixture=='oracle-domain':
        rays=[(10000,12000,10000),(-12000,8000,9000),(9000,-12000,12000),(-8000,-12000,11000)]
        contexts=[(far,light,ray,ambient,directional,mode,code)
                  for (far,light,_,ambient,directional,mode,code),ray in zip(contexts,rays)]
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
        if args.early_global and i==0:cb.emit('SET_GLOBAL',272)
        cb.emit('WAIT',a=2);cb.emit('FLUSH')
    cb.emit('END');program=cb.encode();put(0x1000,program+bytes((-len(program))%128))
    sys.path.insert(0,str(ROOT/'ip/gpu-v2/examples/postquad_ii4'))
    from wrapper import wrapper
    results={}
    for label,path in [('base',args.base)]+([('candidate',args.candidate)] if args.candidate else []):
        case=out/label;case.mkdir();source=path.resolve().read_bytes()
        if label=='candidate' and args.mutation:
            old,new={'far':('wire[31:0]far_z=cfg_far_z;',"wire[31:0]far_z=32'd256;"),
                     'clock':('wire advance = ce;','wire advance = ce && phase!=2;')}[args.mutation]
            text=source.decode();assert text.count(old)==1,'mutation must change exactly one real owner'
            source=text.replace(old,new,1).encode()
        (case/'dut.v').write_bytes(source)
        (case/'memory.hex').write_text(''.join(f'{int.from_bytes(memory[j:j+8],"little"):016x}\n' for j in range(0,len(memory),8)))
        _,ports=wrapper(re.sub(r'\b(input|output)(?=\[)',r'\1 ',source.decode()))
        declarations=[]
        for port in ports['ports']:
            declarations.append(('reg ' if port['direction']=='input' else 'wire ')
                +(f'[{port["bits"]-1}:0] ' if port['bits']>1 else '')+port['name']
                +('=0;' if port['direction']=='input' else ';'))
        (case/'tb.v').write_text(TB.replace('@PORTS@','\n'.join(declarations)).replace('@BYTES@',str(len(program))).replace('@INTERRUPT@',str(['none','reset','abort-reset'].index(args.interrupt))))
        commands=[('compile',[os.environ['IVERILOG_EXE'],'-g2012',*(['-DSYNTHESIS'] if args.synthesis else []),'-s','tb','-o',case/'sim.vvp',case/'dut.v',case/'tb.v',Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v']),
                  ('run',[os.environ['VVP_EXE'],case/'sim.vvp'])]
        reused=False
        if label=='base' and args.reuse_base:
            previous=args.reuse_base.resolve()
            previous_receipt=json.loads((previous.parent/'receipt.json').read_text())
            assert previous_receipt.get('synthesis',False)==args.synthesis,'baseline compile mode changed'
            # Reuse is allowed only for identical actual DUT, stimulus and bench.
            for name in ['dut.v','memory.hex','tb.v']:
                assert (case/name).read_bytes()==(previous/name).read_bytes(),('stale baseline',name)
            assert 'PASS DIRECT_GLOBAL' in (previous/'run.log').read_text()
            for name in ['image.hex','shade.txt','trace.txt','inputs.txt','run.log']:
                (case/name).write_bytes((previous/name).read_bytes())
            reused=True
        else:
            for name,command in commands:
                with (case/(name+'.log')).open('wb') as log:
                    result=subprocess.run(list(map(str,command)),cwd=case,stdout=log,stderr=subprocess.STDOUT,timeout=60)
                if args.mutation and label=='candidate' and name=='run':
                    log_text=(case/'run.log').read_text(errors='replace')
                    reason={'far':'global test conservation','clock':'global comparison watchdog'}[args.mutation]
                    assert result.returncode!=0 and reason in log_text and 'GPU contract' not in log_text,log_text[-2000:]
                elif args.early_global and name=='run':
                    log_text=(case/'run.log').read_text(errors='replace')
                    assert result.returncode!=0 and 'CP state command contract' in log_text,log_text[-2000:]
                else:
                    assert result.returncode==0,(case/(name+'.log')).read_text(errors='replace')[-2000:]
        results[label]=dict(reused_identical_baseline=reused,export_sha256=hashlib.sha256(source).hexdigest(),memory_hex_sha256=hashlib.sha256((case/'memory.hex').read_bytes()).hexdigest(),
            cases=[l for l in (case/'run.log').read_text().splitlines() if l.startswith('PASS ')])
    if args.mutation:
        receipt=dict(status='pass-hardware-mutation-rejected',mutation=args.mutation,synthesis=True,results=results)
        (out/'receipt.json').write_text(json.dumps(receipt,indent=2));print(json.dumps(receipt));return
    if args.early_global:
        rejection_times=[]
        for label in results:
            log=(out/label/'run.log').read_text()
            rejection_times.append(re.search(r'Time: (\d+)',log).group(1))
        assert len(set(rejection_times))==1,'CP rejection edge changed'
        receipt=dict(status='pass-expected-rejection',reason='CP state command contract',time=rejection_times[0],results=results)
        (out/'receipt.json').write_text(json.dumps(receipt,indent=2));print(json.dumps(receipt));return
    for name in ['image.hex','shade.txt','trace.txt','inputs.txt']:
        a=(out/'base'/name).read_bytes()
        if args.candidate:
            b=(out/'candidate'/name).read_bytes()
            assert a==b,'Global borrowing changed '+name
        results[name+'_sha256']=hashlib.sha256(a).hexdigest()
    shading=[tuple(int(x,16) for x in line.split()) for line in (out/'base/shade.txt').read_text().splitlines()]
    assert len(shading)>8 and any(h for _,_,h in shading), 'Missing pipelined lighting/specular coverage'
    receipt=dict(status='pass',projection_fixture=args.projection_fixture,program_sha256=hashlib.sha256(program).hexdigest(),backend=args.backend,interrupt=args.interrupt,synthesis=args.synthesis,test_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),results=results)
    (out/'receipt.json').write_text(json.dumps(receipt,indent=2));print(json.dumps(receipt))


if __name__=='__main__':main()
