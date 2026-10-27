"""Three tiny CP draws switch A(full mip)/B(base only)/A without reset.

Exercises the actual descriptor loader, LOD, Sampling, cache invalidate and ROP.
The second case diagnoses an invalidate without the post-draw WAIT(all).
"""
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
from replay_command import CommandBuffer, texture_word

TB = r'''
`timescale 1ns/1ps
module tb;
@PORTS@
reg[31:0]command_base=0,command_bytes=0;
@GSR@
always #5 clk=~clk;
gpu_v2_vliw_command dut(.*);
reg[63:0]memory[0:8191],image[0:65535];
reg active=0,ra=0,wa=0;
integer cycle=0,mode=0,ba=0,bi=0,bn=0,rb=0,ri=0,wb=0,wi=0,j,k;
@TRACE@
integer starts=0,invalidates=0,flushes=0,changed=0,commits=0,texture_reads=0;
reg[15:0]expected;
function integer image_index;
 input integer halfword;
 begin
  if(halfword>=22'h80000&&halfword<22'h98000)image_index=halfword/4-131072;
  else if(halfword>=22'ha0000&&halfword<22'hb8000)image_index=32768+halfword/4-163840;
  else begin $fatal(1,"FB address");image_index=0;end
 end
endfunction
always @(negedge clk)begin
 ce=cycle%7!=2;ro_request_ready=!active&&cycle%5!=1;
 ro_response_valid=active&&cycle%3!=1;ro_response_last=bi==bn-1;ro_response_error=0;
 ro_data=memory[(ba>>2)+bi];
 fb_r_request_ready=!ra&&!wa;fb_r_response_valid=ra&&cycle%3!=2;
 fb_r_response_last=ri==15;fb_r_response_error=0;fb_r_data=ra?image[rb+ri]:0;
 fb_w_request_ready=!wa&&!ra;fb_w_data_ready=wa&&cycle%4!=2;
 fb_w_response_valid=wa&&wi==15&&fb_w_data_ready;
 fb_w_response_last=1;fb_w_response_error=0;
end
always @(posedge clk)begin
 cycle=cycle+1;if(cycle>30000)$fatal(1,"CP texture watchdog");
 if(!reset)begin
  if(ro_request_valid&&ro_request_ready)begin
   active=1;ba=ro_address;bi=0;bn=ro_count==3?16:ro_count==1?8:4;
   if((ro_address>=22'h1000&&ro_address<22'h1200)||(ro_address>=22'h1400&&ro_address<22'h1440))texture_reads=texture_reads+1;
  end else if(ro_response_valid)begin if(ro_response_last)active=0;else bi=bi+1;end
  if(fb_r_request_valid&&fb_r_request_ready)begin ra=1;rb=image_index(fb_r_address);ri=0;end
  else if(fb_r_response_valid)begin if(ri==15)ra=0;else ri=ri+1;end
  if(fb_w_request_valid&&fb_w_request_ready)begin wa=1;wb=image_index(fb_w_address);wi=0;end
  else if(wa&&fb_w_data_ready)begin image[wb+wi]=fb_w_data;wi=wi+1;if(wi==16)wa=0;end
  if(dut.start_fire)begin
   if(dut.texture_base!==(starts==1?16'h50:16'h40)||
      dut.texture_n!==(starts==1?4'd3:4'd4)||dut.texture_mip!==(starts!=1)||dut.texture_filter!==starts[1:0])
    $fatal(1,"installed descriptor mismatch");
   if(dut.core.cfg_tex_w_log2!==dut.core.cfg_tex_h_log2||
      dut.core.cfg_tex_w_log2!=={1'b0,dut.texture_n}||
      dut.core.cfg_max_mip!==(dut.texture_mip?{1'b0,dut.texture_n}:5'd0))
    $fatal(1,"LOD/sampler descriptor diverged");
   starts=starts+1;
  end
  if(dut.texture_invalidate)invalidates=invalidates+1;
  if(commit_valid)commits=commits+1;
  if(ce&&dut.flush_request&&dut.flush_complete&&starts!=0)begin
   flushes=flushes+1;expected=flushes==2?16'h07e0:16'hf800;changed=0;
   for(j=0;j<24000;j=j+1)for(k=0;k<4;k=k+1)begin
    if(image[j][k*16+:16]!=0)begin
     changed=changed+1;
     if(image[j][k*16+:16]!==expected)$fatal(1,"wrong material at flush%0d got%h",flushes,image[j][k*16+:16]);
    end
   end
   if(changed==0)$fatal(1,"no rendered pixels");
  end
 end
end
task tick;begin @(negedge clk);#1;end endtask
initial begin
 j=$value$plusargs("MODE=%d",mode);
 $readmemh("memory.hex",memory);
 for(j=0;j<65536;j=j+1)image[j]=j>=32768?64'hffffffffffffffff:64'd0;
 reset=1;output_ready=1;command_base=mode?32'h1800:32'h1000;command_bytes=mode?@BAD_BYTES@:@BYTES@;
 tick;tick;reset=0;submit_commands(command_base,command_bytes);
 while(busy)tick;
 if(active||ra||wa)$fatal(1,"result before memory drain");
 if(mode)begin if(device_read_data[1]!=4||invalidates!=1)$fatal(1,"missing WAIT guard");end
 else if(device_read_data[1]||starts!=3||invalidates!=@INVALIDATES@||flushes!=3||commits!=3||texture_reads!=@TEXTURE_READS@)
  $fatal(1,"conservation error=%0d draws=%0d inv=%0d flush=%0d commits=%0d texture_reads=%0d",device_read_data[1],starts,invalidates,flushes,commits,texture_reads);
 $display("PASS CP texture mode=%0d cycles=%0d draws=%0d invalidates=%0d flushes=%0d texture_reads=%0d error=%0d",mode,cycle,starts,invalidates,flushes,texture_reads,device_read_data[1]);
 $finish;
end
endmodule
'''


def main():
    ap = argparse.ArgumentParser(__doc__)
    ap.add_argument('--export', type=Path, required=True)
    ap.add_argument('--output', type=Path, required=True)
    ap.add_argument('--native', action='store_true')
    ap.add_argument('--physical', action='store_true', help='Switch descriptors without invalidation; retained A must hit')
    ap.add_argument('--vary-bias', action='store_true', help='Change signed LOD bias only after a full drain')
    ap.add_argument('--no-monitors', action='store_true', help='Same datapath with simulation assertions omitted')
    ap.add_argument('--hardware-errors', action='store_true', help='Legacy pre-monitor diagnostic baseline')
    args = ap.parse_args()
    out = args.output.resolve(); out.mkdir(parents=True, exist_ok=False)
    spec = importlib.util.spec_from_file_location('clear_fixture', Path(__file__).with_name('vliw_clear.py'))
    fixture = importlib.util.module_from_spec(spec); spec.loader.exec_module(fixture)
    memory, put, records, _ = fixture.assets(out)
    record, size = records[3]
    q = struct.unpack_from('<Q', memory, record+8)[0]
    struct.pack_into('<Q', memory, record+8, q & ~16)  # same tiny triangle, UV route
    put(0x2000, struct.pack('<H', 0xf800)*512)
    put(0x2800, struct.pack('<H', 0x07e0)*64)
    for address, base, n, mip, filter_code in [(0x4100,0x2000,4,True,0),
                                              (0x4140,0x2800,3,False,1),
                                              (0x4180,0x2000,4,True,2)]:
        lod_bias = [0, 3, -2][filter_code] if args.vary_bias else 0
        put(address, struct.pack('<8I', texture_word(base,n,mip,filter_code),0,7|(1<<5),lod_bias & 255,0,0,0,0))
    def program(bad):
        cb = CommandBuffer()
        cb.emit('DMA',272,0x4000,64);cb.emit('DMA',320,0x4280,128)
        cb.emit('DMA',280,0x4100,32);cb.emit('WAIT',a=3)
        cb.emit('DMA',288,0x4460,32);cb.emit('WAIT',a=3);cb.emit('FLUSH');cb.emit('SET_TARGET',288)
        cb.emit('SET_GLOBAL',272);cb.emit('SET_RENDER',280);cb.emit('TEX_INVALIDATE')
        cb.emit('DMA_MESHLET',0,record,size);cb.emit('WAIT',a=1)
        for i in range(3):
            if i:
                cb.emit('DMA',280,0x4100+i*0x40,32);cb.emit('WAIT',a=3)
                cb.emit('SET_RENDER',280)
                if not args.physical:cb.emit('TEX_INVALIDATE')
            cb.emit('DRAW_MESHLET',0)
            if bad:cb.emit('TEX_INVALIDATE')
            cb.emit('WAIT',a=2);cb.emit('FLUSH')
        cb.emit('END')
        return cb.encode()
    normal, bad = program(False), program(True)
    put(0x1000, normal+bytes((-len(normal))%128));put(0x1800, bad+bytes((-len(bad))%128))
    (out/'memory.hex').write_text(''.join(f'{int.from_bytes(memory[i:i+8],"little"):016x}\n' for i in range(0,len(memory),8)))
    source = args.export.resolve(); identity = hashlib.sha256(source.read_bytes()).hexdigest()
    sys.path.insert(0, str(ROOT/'ip/gpu-v2/examples/postquad_ii4'))
    from wrapper import wrapper
    _, ports = wrapper(re.sub(r'\b(input|output)(?=\[)',r'\1 ',source.read_text()))
    declarations = []
    for p in ports['ports']:
        width = f'[{p["bits"]-1}:0]' if p['bits']>1 else ''
        declarations.append(('reg ' if p['direction']=='input' else 'wire ')+width+' '+p['name']+('=0;' if p['direction']=='input' else ';'))
    tb = TB.replace('@PORTS@','\n'.join(declarations)).replace('@BYTES@',str(len(normal))).replace('@BAD_BYTES@',str(len(bad)))
    # Filtered A crosses one additional periodic tile boundary. The retained
    # nearest A tile must hit, while that newly demanded tile still pays a refill.
    tb = tb.replace('@INVALIDATES@',str(1 if args.physical else 3)).replace('@TEXTURE_READS@',str(3 if args.physical else 4))
    tb = tb.replace('@GSR@',"GSR GSR(.GSRI(1'b1));" if args.native else '')
    if args.vary_bias:
        tb = tb.replace('  if(dut.texture_invalidate)', """
  if(ce&&dut.core.front.interpolator.cfg_apply)begin
   if(dut.core.front.interpolator.lc_bias!==dut.cfg_lod_bias ||
      dut.core.front.interpolator.lc_tex_u!=={1'b0,dut.texture_n} ||
      dut.core.front.interpolator.lc_tex_v!=={1'b0,dut.texture_n} ||
      dut.core.front.interpolator.lc_max!==(dut.texture_mip?{1'b0,dut.texture_n}:5'd0))
    $fatal(1,"prepared current descriptor/bias diverged at context publication");
  end
  if(dut.texture_invalidate)""")

    outputs = ','.join(p['name'] for p in ports['ports'] if p['direction']=='output')
    tb = tb.replace('@TRACE@', '''integer trace_file=0;
always @(negedge clk)begin #1;
 if(!reset&&mode==0)begin
  if(trace_file==0)trace_file=$fopen("valid.trace","w");
  $fdisplay(trace_file,"%h",{''' + outputs + '''});
 end
end''')
    from device_bus import bench as device_bench
    tb=device_bench(tb)
    (out/'tb.v').write_text(tb)
    command = [os.environ['IVERILOG_EXE'],'-g2012','-s','tb','-o',out/'sim.vvp',source,out/'tb.v']
    if args.native:command.append(Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v')
    if args.no_monitors:command.insert(1,'-DSYNTHESIS')
    results = []
    for label, cmd in [('compile',command),('valid',[os.environ['VVP_EXE'],out/'sim.vvp','+MODE=0']),
                       ('missing-wait',[os.environ['VVP_EXE'],out/'sim.vvp','+MODE=1'])]:
        if label=='missing-wait' and args.no_monitors:continue
        assert hashlib.sha256(source.read_bytes()).hexdigest()==identity
        with (out/(label+'.log')).open('wb') as log:
            r = subprocess.run(list(map(str,cmd)),cwd=out,stdout=log,stderr=subprocess.STDOUT,timeout=60)
        log_text=(out/(label+'.log')).read_text(errors='replace')
        if label=='missing-wait' and not args.hardware_errors:
            assert r.returncode!=0 and 'CP drain contract' in log_text,log_text[-1800:]
            results.append('PASS missing WAIT diagnosed by independent simulation assertion')
        else:
            assert r.returncode==0,log_text[-1800:]
            results += [line for line in log_text.splitlines() if line.startswith('PASS ')]
    receipt = dict(status='pass',native=args.native,physical=args.physical,vary_bias=args.vary_bias,monitors=not args.no_monitors,export_sha256=identity,cases=results,
                   output_trace_sha256=hashlib.sha256((out/'valid.trace').read_bytes()).hexdigest(),
                   test_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest())
    (out/'receipt.json').write_text(json.dumps(receipt,indent=2));print(json.dumps(receipt))


if __name__ == '__main__':main()
