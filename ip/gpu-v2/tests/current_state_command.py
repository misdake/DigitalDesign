"""Two bounded command smokes: relocated targets and overlapping mesh uniforms."""
from pathlib import Path
import argparse
import hashlib
import importlib.util
import json
import os
import re
import struct
import subprocess
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT/'ip/gpu-v2/examples/vliw_integration'))
from replay_command import CommandBuffer, target_params, texture_word

TB = r'''
`timescale 1ns/1ps
module tb;
@PORTS@
reg[31:0]command_base=0,command_bytes=0;
@GSR@
always #5 clk=~clk;
gpu_v2_vliw_command dut(.*);
reg[63:0]memory[0:8191],image[0:131071],saved[0:65535];
reg[31:0]colors[0:2];
reg active=0,ra=0,wa=0;
integer cycle=0,mode=0,ba=0,bi=0,bn=0,rb=0,ri=0,wb=0,wi=0,j,k;
integer starts=0,allocs=0,accepted=0,flushes=0,alpha_checks=0,changed=0,overlap=0;
reg[15:0]expected;
function integer image_index;
 input integer halfword;
 begin
  if(halfword>=@AC@/2&&halfword<(@AC@+192000)/2)image_index=(halfword-@AC@/2)/4;
  else if(halfword>=@AD@/2&&halfword<(@AD@+192000)/2)image_index=32768+(halfword-@AD@/2)/4;
  else if(halfword>=@BC@/2&&halfword<(@BC@+192000)/2)image_index=65536+(halfword-@BC@/2)/4;
  else if(halfword>=@BD@/2&&halfword<(@BD@+192000)/2)image_index=98304+(halfword-@BD@/2)/4;
  else begin $fatal(1,"old or invalid target address %h",halfword);image_index=0;end
 end
endfunction
always @(negedge clk)begin
 ce=cycle%7!=2;ro_request_ready=!active&&cycle%5!=1;
 ro_response_valid=active&&cycle%3!=1;ro_response_last=bi==bn-1;ro_response_error=0;
 ro_data=memory[(ba>>2)+bi];
 // Force an old shaded quad to coexist with a later mesh's V/T and uniform.
 fb_r_request_ready=!ra&&!wa&&(mode==0||starts>=2);fb_r_response_valid=ra&&cycle%3!=2;
 fb_r_response_last=ri==15;fb_r_response_error=0;fb_r_data=ra?image[rb+ri]:0;
 fb_w_request_ready=!wa&&!ra;fb_w_data_ready=wa&&cycle%4!=2;
 fb_w_response_valid=wa&&wi==15&&fb_w_data_ready;fb_w_response_last=1;fb_w_response_error=0;
end
always @(posedge clk)begin
 cycle=cycle+1;if(cycle>20000)$fatal(1,"state smoke bound");
 if(!reset)begin
  if(ro_request_valid&&ro_request_ready)begin active=1;ba=ro_address;bi=0;bn=ro_count==3?16:ro_count==1?8:4;end
  else if(ro_response_valid)begin if(ro_response_last)active=0;else bi=bi+1;end
  if(fb_r_request_valid&&fb_r_request_ready)begin ra=1;rb=image_index(fb_r_address);ri=0;end
  else if(fb_r_response_valid)begin if(ri==15)ra=0;else ri=ri+1;end
  if(fb_w_request_valid&&fb_w_request_ready)begin wa=1;wb=image_index(fb_w_address);wi=0;end
  else if(wa&&fb_w_data_ready)begin image[wb+wi]=fb_w_data;wi=wi+1;if(wi==16)wa=0;end
  if(dut.start_fire)begin
   if(dut.mesh_color!==colors[starts])$fatal(1,"wrong uniform at DRAW %0d",starts);
   if(mode==1&&starts>0&&accepted<starts)overlap=overlap+1;
   starts=starts+1;
  end
  if(dut.core.back.front.quad.allocate)begin
   if(dut.core.quad_color!==colors[allocs])$fatal(1,"T/I color epoch mismatch quad%0d got%h",allocs,dut.core.quad_color);
   allocs=allocs+1;
  end
  if(dut.core.back.sink.cache.row_accept&&dut.core.back.sink.cache.row_index[0])begin
   if(dut.core.back.sink.depth_alpha!==colors[accepted][31:24])$fatal(1,"ROP alpha owner mismatch");
   alpha_checks=alpha_checks+1;
  end
  if(quad_accepted)accepted=accepted+1;
  if(ce&&dut.flush_request&&dut.flush_complete&&starts!=0)begin
   flushes=flushes+1;
   if(mode==0)begin
    expected=flushes==1?16'h8000:flushes==2?16'h18fb:16'hf800;changed=0;
    for(j=0;j<24000;j=j+1)for(k=0;k<4;k=k+1)begin
     if(image[(flushes==2?65536:0)+j][k*16+:16]!=0)begin
      changed=changed+1;
      if(image[(flushes==2?65536:0)+j][k*16+:16]!==expected)$fatal(1,"wrong target/color at flush%0d",flushes);
     end
    end
    if(changed==0)$fatal(1,"empty state smoke");
    if(flushes==1)for(j=0;j<65536;j=j+1)saved[j]=image[j];
    if(flushes==2)for(j=0;j<65536;j=j+1)if(saved[j]!==image[j])$fatal(1,"B changed retained A color/depth");
   end
  end
 end
end
task tick;begin @(negedge clk);#1;end endtask
initial begin
 j=$value$plusargs("MODE=%d",mode);$readmemh("memory.hex",memory);$readmemh("colors.hex",colors);
 for(j=0;j<131072;j=j+1)image[j]=j%65536>=32768?64'hffffffffffffffff:0;
 reset=1;output_ready=1;command_base=32'h1000;command_bytes=@BYTES@;
 tick;tick;reset=0;submit_commands(command_base,command_bytes);
 while(busy)tick;
 if(active||ra||wa||device_read_data[1]||starts!=3||allocs!=3||accepted!=3||alpha_checks==0)$fatal(1,"state smoke conservation");
 if(mode==1)begin
  if(overlap==0||flushes!=1)$fatal(1,"overlap was not exercised");changed=0;
  for(j=0;j<24000;j=j+1)for(k=0;k<4;k=k+1)if(image[j][k*16+:16]!=0)begin
   changed=changed+1;if(image[j][k*16+:16]!==16'h001f)$fatal(1,"overlap final color");
  end
  if(changed==0)$fatal(1,"empty overlap frame");
 end else if(flushes!=3)$fatal(1,"target flush count");
 $display("PASS state mode=%0d cycles=%0d draws=%0d quads=%0d alpha=%0d overlap=%0d",mode,cycle,starts,accepted,alpha_checks,overlap);$finish;
end
endmodule
'''


def main():
    ap = argparse.ArgumentParser(__doc__)
    ap.add_argument('--export', required=True, type=Path)
    ap.add_argument('--output', required=True, type=Path)
    ap.add_argument('--native', action='store_true')
    ap.add_argument('--production-cold', action='store_true')
    ap.add_argument('--production-top', type=Path, default=ROOT/'systems/cpu-v3-tang-nano-20k/examples/cpu_v3_system/self_test.v')
    args = ap.parse_args()
    out = args.output.resolve(); out.mkdir(parents=True, exist_ok=False)
    spec = importlib.util.spec_from_file_location('state_fixture', Path(__file__).with_name('vliw_clear.py'))
    fixture = importlib.util.module_from_spec(spec); spec.loader.exec_module(fixture)
    source = args.export.resolve(); identity = hashlib.sha256(source.read_bytes()).hexdigest()
    sys.path.insert(0, str(ROOT/'ip/gpu-v2/examples/postquad_ii4'))
    from wrapper import wrapper
    _, ports = wrapper(re.sub(r'\b(input|output)(?=\[)', r'\1 ', source.read_text()))
    cold_top = args.production_top.read_text() if args.production_cold else None
    if args.production_cold:
        assert args.native, 'The production binding uses native DSP/BSRAM'
        reset_expression=re.search(r'__GPU__ u_gpu\s*\(.*?\.reset\(([^)]+)\)',cold_top,re.S)[1]
        cold_gate=re.search(r'reg \[1:0\] cold_ready_sync=0;.*?^end',cold_top,re.S|re.M)[0]
        leaf=ROOT/'systems/cpu-v3-tang-nano-20k/src/hardware/gpu/gpu.v'
        staged=out/'dut.v';staged.write_bytes(source.read_bytes()+b'\n'+leaf.read_bytes())
        source=staged;identity=hashlib.sha256(source.read_bytes()).hexdigest()
    declarations = []
    for p in ports['ports']:
        declarations.append(('reg ' if p['direction']=='input' else 'wire ')
            + (f'[{p["bits"]-1}:0] ' if p['bits']>1 else '') + p['name']
            + ('=0;' if p['direction']=='input' else ';'))
    summaries = []
    bases = [0x120000, 0x180000, 0x1e0000, 0x240000]
    for mode in (0, 1):
        case = out/str(mode); case.mkdir()
        memory, put, records, _ = fixture.assets(case)
        record, size = records[3]
        original = bytes(memory[record:record+128+size])
        instance = bytes(memory[0x4280:0x4300])
        colors = [0xffffff80, 0x80ff4080, 0x40ffffff] if mode==0 else [0xc00000ff, 0x8000ff00, 0x40ff0000]
        for i, color in enumerate(colors):
            uniform = bytearray(instance);struct.pack_into('<II', uniform, 88, color, 0)
            put(0x4600+i*128, uniform)
            data = bytearray(original)
            q = struct.unpack_from('<Q', data, 0)[0]
            struct.pack_into('<Q', data, 0, (q & ~(63<<16)) | ((20+i)<<16))
            q = struct.unpack_from('<Q', data, 8)[0]
            rgb = mode==1 or i==1
            struct.pack_into('<Q', data, 8, (q & ~16) | (int(rgb)<<4))
            if rgb:
                for v in range(3):struct.pack_into('<I', data, 128+v*12+8, 0xe07030 if mode==0 else 0xffffff)
            put(0xa000+i*0x500, data)
        put(0x2000, struct.pack('<H', 0xf800)*512)
        put(0x4100, struct.pack('<8I', texture_word(0x2000,4,True,0),0,7|(1<<5),0,0,0,0,0))
        put(0x5600,target_params(*bases[:2]));put(0x5620,target_params(*bases[2:]))
        cb=CommandBuffer()
        cb.emit('DMA',272,0x4000,64);cb.emit('DMA',280,0x4100,32)
        for i in range(3):cb.emit('DMA',320+i*16,0x4600+i*128,128)
        cb.emit('DMA',288,0x5600,32);cb.emit('WAIT',a=3);cb.emit('FLUSH');cb.emit('SET_TARGET',288)
        cb.emit('SET_GLOBAL',272);cb.emit('SET_RENDER',280)
        cb.emit('DMA_MESHLET',0,0xa000,size);cb.emit('WAIT',a=1)
        for i in range(3):
            if mode==0 and i:
                cb.emit('DMA',288,0x5620 if i==1 else 0x5600,32);cb.emit('WAIT',a=3);cb.emit('SET_TARGET',288)
            if i<2:cb.emit('DMA_MESHLET',(i+1)%2,0xa000+(i+1)*0x500,size)
            cb.emit('DRAW_MESHLET',i%2);cb.emit('WAIT',a=1)
            if mode==0:cb.emit('WAIT',a=2);cb.emit('FLUSH')
        if mode==1:cb.emit('WAIT',a=2);cb.emit('FLUSH')
        cb.emit('END');program=cb.encode();put(0x1000,program+bytes((-len(program))%128))
        (case/'memory.hex').write_text(''.join(f'{int.from_bytes(memory[i:i+8],"little"):016x}\n' for i in range(0,len(memory),8)))
        (case/'colors.hex').write_text(''.join(f'{c:08x}\n' for c in colors))
        tb=TB.replace('@PORTS@','\n'.join(declarations)).replace('@GSR@',"GSR GSR(.GSRI(1'b1));" if args.native else '').replace('@BYTES@',str(len(program)))
        for key, value in zip(('AC','AD','BC','BD'),bases):tb=tb.replace('@'+key+'@',str(value))
        from device_bus import bench as device_bench
        tb=device_bench(tb)
        if args.production_cold:
            from device_bus import system_leaf
            tb=system_leaf(tb,ports['ports'],reset_expression)
            tb=tb.replace('ce=cycle%7!=2;', 'ce=1;')
            tb=tb.replace('always #5 clk=~clk;', 'reg sdram_clock_ready=0,sdram_init_done=0;\n'+cold_gate+'\nalways #5 clk=~clk;')
            tb=tb.replace('reset=1;output_ready=1;', 'reset=0;output_ready=1;')
            tb=tb.replace('tick;tick;reset=0;submit_commands(command_base,command_bytes);',
                'repeat(3)tick;sdram_clock_ready=1;repeat(4)tick;sdram_init_done=1;while(!cold_ready)tick;tick;'
                'if(busy!==0)$fatal(1,"Production cold start did not initialize GPU");submit_commands(command_base,command_bytes);')
            tb=tb.replace('while(busy)tick;', 'while(busy)tick;sdram_clock_ready=0;sdram_init_done=0;repeat(4)tick;'
                'if(!cold_ready||busy!==0)$fatal(1,"Cold gate reasserted after readiness loss");')
        (case/'tb.v').write_text(tb)
        commands=[('compile',[os.environ['IVERILOG_EXE'],'-g2012','-s','tb','-o',case/'sim.vvp',source,case/'tb.v']),
                  ('run',[os.environ['VVP_EXE'],case/'sim.vvp',f'+MODE={mode}'])]
        if args.native:commands[0][1].append(Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v')
        for label,command in commands:
            assert hashlib.sha256(source.read_bytes()).hexdigest()==identity
            with (case/(label+'.log')).open('wb') as log:
                result=subprocess.run(list(map(str,command)),cwd=case,stdout=log,stderr=subprocess.STDOUT,timeout=60)
            log_text=(case/(label+'.log')).read_text(errors='replace')
            assert result.returncode==0,log_text[-2000:]
        summaries += [line for line in log_text.splitlines() if line.startswith('PASS ')]
    receipt=dict(status='pass',export_sha256=identity,cases=summaries,production_cold=args.production_cold,
        production_top_sha256=hashlib.sha256(args.production_top.read_bytes()).hexdigest() if args.production_cold else None)
    (out/'receipt.json').write_text(json.dumps(receipt,indent=2));print(json.dumps(receipt))


if __name__=='__main__':main()
