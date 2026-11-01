"""Bounded raw memcpy through the production GPU, native MC and SDRAM pins."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / 'hardware/vendor/gowin/tests'))
from stream_contract import load_contract, stage_peer
sys.path.insert(0, str(ROOT / 'ip/gpu-v2/examples/vliw_integration'))
from replay_command import CommandBuffer, memcpy_params
from device_bus import TASKS
sys.path.insert(0, str(ROOT / 'ip/gpu-v2/examples/postquad_ii4'))
from wrapper import wrapper
import command_continuation


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--export', type=Path, required=True)
    parser.add_argument('--peer', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--bytes', type=int, default=1536)
    parser.add_argument('--modes', type=int, nargs='+', default=[0, 1, 2, 3, 4, 5])
    parser.add_argument('--trace', action='store_true', help='Record every command/TDP/router edge')
    parser.add_argument('--no-monitors', action='store_true')
    args = parser.parse_args()
    assert 512 <= args.bytes <= 0x100000 and args.bytes % 512 == 0
    assert all(0 <= mode <= 5 for mode in args.modes)
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    data = args.export.read_bytes()
    (out / 'dut.v').write_bytes(data)
    _, info = wrapper(re.sub(r'\b(input|output)(?=\[)', r'\1 ', data.decode()))
    declarations = []
    for port in info['ports']:
        driven = port['direction'] == 'input' and not port['name'].startswith(('ro_', 'fb_r_', 'fb_w_'))
        declarations.append(('reg ' if driven else 'wire ')
            + (f'[{port["bits"]-1}:0] ' if port['bits'] > 1 else '')
            + port['name'] + ('=0;' if driven else ';'))
    seed = {}
    def put(address, blob):
        assert len(blob) % 4 == address % 4 == 0
        seed.update((address + i, int.from_bytes(blob[i:i+4], 'little')) for i in range(0, len(blob), 4))
    # Deliberately exercise all bits, including alpha-like low bits and lane
    # swizzles; source and each destination have independent guard words.
    source = bytes(((i * 73) ^ (i >> 3) ^ (i >> 9) ^ 0xa5) & 255 for i in range(args.bytes))
    jobs = [(0x200000, 0x400000, args.bytes), (0x400000, 0x7ffe00, 512), (0x7ffe00, 0x600000, 512)]
    for base, size in [(0x200000, args.bytes), (0x400000, args.bytes), (0x7ffe00, 512), (0x600000, 512)]:
        put(base-128, bytes([0x69])*128)
        put(base, bytes([0x3c])*size)
        if base+size < 0x800000:
            put(base+size, bytes([0x96])*128)
    put(0x200000, source)
    put(0x51000, b''.join(memcpy_params(*job) for job in jobs) + bytes(32))
    cb = CommandBuffer()
    cb.emit('WAIT', a=3)
    cb.emit('FLUSH')
    cb.emit('DMA', 272, 0x51000, 128)
    cb.emit('WAIT', a=1)
    for _ in range(22):
        cb.emit('NOP')
    cb.emit('MEMCPY', 272)
    for _ in range(4):
        cb.emit('NOP')
    cb.emit('MEMCPY', 276)
    cb.emit('MEMCPY', 280)
    cb.emit('END')
    commands = cb.encode()
    put(0x40000, commands + bytes(-len(commands) % 128))
    expected = seed.copy()
    for src, dst, size in jobs:
        for off in range(0, size, 4):
            expected[dst+off] = expected[src+off]
    (out / 'seed.txt').write_text(''.join(f'{a:x} {v:08x}\n' for a, v in sorted(seed.items())))
    (out / 'expected.txt').write_text(''.join(f'{a:x} {v:08x}\n' for a, v in sorted(expected.items())))
    tb = command_continuation.TB.split('integer cycle=0')[0]
    tb = tb.replace('@PORTS@', '\n'.join(declarations)).replace('gpu_v2_vliw_command dut(.*);', TASKS+'\ngpu_v2_vliw_command dut(.*);')
    tb += r'''
integer cycle=0,mode=0,fd,rc,a,v,rd=0,wr=0,beats=0,terms=0,active_cycles=0;
integer first=0,last=0,pause_left=0;reg paused=0;
always @(negedge clk)if(!reset)begin
 ce=(mode!=1||cycle%7!=3);
 if(mode==4&&!paused&&rd!=0)begin pause_left=80;paused=1;end
 if(mode==5&&!paused&&wr!=0)begin pause_left=80;paused=1;end
 if(pause_left!=0)begin ce=0;pause_left=pause_left-1;end
 if(mode==2&&rd!=0)abort=1;
 if(mode==3&&wr!=0)abort=1;
end
always @(posedge clk)begin
 cycle=cycle+1;
 if(cycle>1000000)$fatal(1,"MEMCPY watchdog cp%0d rd%0d wr%0d",dut.processor.state,rd,wr);
 if(!reset)begin
  if(dut.copy_active)active_cycles=active_cycles+1;
  if(fb_r_request_valid&&fb_r_request_ready)begin
   if(fb_r_count!=3||fb_r_address!==((rd<@FIRST_SECTORS@?32'h200000+rd*128:rd<@FIRST_SECTORS@+4?32'h400000+(rd-@FIRST_SECTORS@)*128:32'h7ffe00+(rd-@FIRST_SECTORS@-4)*128)/2))$fatal(1,"MEMCPY read address rd%0d addr%h",rd,fb_r_address);
   rd=rd+1;if(first==0)first=cycle;
  end
  if(fb_w_request_valid&&fb_w_request_ready)begin
   if(fb_w_count!=3||fb_w_address!==((wr<@FIRST_SECTORS@?32'h400000+wr*128:wr<@FIRST_SECTORS@+4?32'h7ffe00+(wr-@FIRST_SECTORS@)*128:32'h600000+(wr-@FIRST_SECTORS@-4)*128)/2))$fatal(1,"MEMCPY write address wr%0d addr%h",wr,fb_w_address);
   wr=wr+1;
  end
  if(fb_w_data_ready)beats=beats+1;
  if(fb_w_response_valid&&fb_w_response_last)begin terms=terms+1;last=cycle;end
  if(ro_response_error||fb_r_response_error||fb_w_response_error)$fatal(1,"MEMCPY MC error");
 end
end
initial begin
 rc=$value$plusargs("MODE=%d",mode);reset=1;ce=1;output_ready=1;
 fd=$fopen("seed.txt","r");
 while(!$feof(fd))begin rc=$fscanf(fd,"%h %h\n",a,v);if(rc==2)begin pin.mem[native_word(a)]=v;pin.marked[native_word(a)]=1;end end
 $fclose(fd);
 repeat(4)@(negedge clk);reset=0;
 submit_commands(32'h40000,@BYTES@);
 if(mode==2||mode==3)begin
  while(!abort||dut.copy_active||dut.memory_busy||!dut.dma_idle)@(negedge clk);
  if(!busy||!fault||wr!=(mode==2?0:4)||terms!=wr||beats!=wr*16)$fatal(1,"MEMCPY abort drain rd%0d wr%0d beats%0d terms%0d",rd,wr,beats,terms);
  $display("PASS MEMCPY_ABORT mode=%0d cycles=%0d rd=%0d wr=%0d",mode,cycle,rd,wr);$finish;
 end
 while(busy)@(negedge clk);
 if((mode==4||mode==5)&&!paused)$fatal(1,"MEMCPY pause not exercised");
 fd=$fopen("expected.txt","r");
 while(!$feof(fd))begin rc=$fscanf(fd,"%h %h\n",a,v);
  if(rc==2&&pin.mem[native_word(a)]!==v)$fatal(1,"MEMCPY address%h got%h want%h",a,pin.mem[native_word(a)],v);
 end
 $fclose(fd);
 if(rd!=@SECTORS@||wr!=rd||terms!=wr||beats!=wr*16||device_read_data[1])$fatal(1,"MEMCPY conservation rd%0d wr%0d terms%0d",rd,wr,terms);
 $display("PASS MEMCPY mode=%0d cycles=%0d active=%0d span=%0d bytes=@COPIED@ rd=%0d wr=%0d",mode,cycle,active_cycles,last-first+1,rd,wr);$finish;
end
endmodule
'''
    copied = sum(job[2] for job in jobs)
    tb = tb.replace('@BYTES@', str(len(commands))).replace('@SECTORS@', str(copied//128)).replace('@COPIED@', str(copied)).replace('@FIRST_SECTORS@', str(args.bytes//128))
    if args.trace:
        from maintenance_profiles import trace_tb
        tb = trace_tb(tb)
    (out / 'tb.v').write_text(tb)
    contract = load_contract(args.peer)
    files = stage_peer(args.peer, out)
    run = [('compile', [os.environ['IVERILOG_EXE'], '-g2012', *(['-DSYNTHESIS'] if args.no_monitors else []), '-D__ICARUS__', '-I', out, '-s', 'tb', '-o', out/'sim.vvp', out/'dut.v', *files, out/'tb.v', Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v'])]
    run += [(f'run-{mode}', [os.environ['VVP_EXE'], out/'sim.vvp', f'+MODE={mode}']) for mode in args.modes]
    passes = []
    for name, command in run:
        assert hashlib.sha256((out/'dut.v').read_bytes()).digest() == hashlib.sha256(data).digest()
        with (out/f'{name}.log').open('w') as log:
            result = subprocess.run(list(map(str, command)), cwd=out, stdout=log, stderr=subprocess.STDOUT, timeout=300)
        text = (out/f'{name}.log').read_text(errors='replace')
        if result.returncode:
            raise RuntimeError(text[-2400:])
        passes += [line for line in text.splitlines() if line.startswith('PASS ')]
    (out/'receipt.json').write_text(json.dumps(dict(profile=contract['profile'], export_sha256=hashlib.sha256(data).hexdigest(), bytes=copied, jobs=jobs, passes=passes), indent=2))
    print('\n'.join(passes))


if __name__ == '__main__':
    main()
