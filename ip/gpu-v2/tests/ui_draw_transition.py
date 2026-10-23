"""Bounded DRAW/UI/DRAW transition through native GPU, shared MC and SDRAM pins."""
import argparse
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / 'ip/gpu-v2/examples/vliw_integration'))
from replay_command import CommandBuffer, target_params
from device_bus import TASKS
sys.path.insert(0, str(ROOT / 'ip/gpu-v2/examples/postquad_ii4'))
from wrapper import wrapper


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def assets(out, layout):
    fixture = load('transition_assets', Path(__file__).with_name('vliw_clear.py'))
    ui = load('transition_glyph', Path(__file__).with_name('ui_grid.py'))
    memory, _, records, _ = fixture.assets(out)
    seed = {a: int.from_bytes(memory[a:a+4], 'little') for a in range(0, len(memory), 4)}

    def put(address, data):
        assert address % 4 == 0 and len(data) % 4 == 0
        seed.update((address+i, int.from_bytes(data[i:i+4], 'little')) for i in range(0, len(data), 4))

    here = ROOT / 'ip/gpu-v2/examples/vliw_integration'
    oracle = fixture.load('transition_oracle', here / 'reference/scene/oracle.py')
    oracle = fixture.load('transition_transport', here / 'transport.py').adapt_oracle(oracle)
    instance = copy.deepcopy(json.loads((here / 'reference/uv-inputs.json').read_text())['scenes'][0]['meshlets'][0])
    instance['vertices'] = [copy.deepcopy(instance['vertices'][i]) for i in instance['triangles'][0]]
    instance.update(slot=1, triangles=[[0, 1, 2]], color_mode=1, cull_negative=1)
    _, packets = oracle.prepare(instance, out / 'coverage-trial')
    if not packets:
        instance['triangles'] = [[0, 2, 1]]
    _, packets = oracle.prepare(instance, out / 'coverage')
    assert len(packets) == 1
    depth_pixels = {}
    for quad in packets[0]['masks']:
        depth_pixels.update(oracle.expected_quad(packets[0], quad)['depth'])
    assert 0 < len(depth_pixels) <= 8

    record, size = records[3]
    for index, rgb in enumerate((0x0000ff, 0xff0000)):
        data = bytearray(memory[record:record+128+size])
        for vertex in range(3):
            struct.pack_into('<I', data, 128+vertex*12+8, rgb)
        put(0xa000+index*0x500, data)
    render = bytearray(memory[0x4100:0x4120])
    # The second draw deliberately covers the same point at the same depth.
    # ALWAYS+write makes its blue result observable instead of failing LESS.
    struct.pack_into('<I', render, 8, 7 | (1 << 3) | (1 << 5))
    put(0x4100, render)

    # Two maximal disjoint layouts, including the final 128-byte grid sector.
    grid, atlas = (0x7ffd00, 0x7e0000) if layout == 'high-grid' else (0x7efd00, 0x7f0000)
    assert grid+768 <= 0x800000 and atlas+65536 <= 0x800000
    assert grid+768 <= atlas or atlas+65536 <= grid
    cells = bytearray([255]*750)
    cells[0], cells[749] = 0, 254
    put(grid, cells+bytes(18))
    put(atlas, bytes(65536))
    for char in (0, 254):
        words = [ui.glyph(char, group*4+(lane ^ (2 if y & 1 else 0)), y)
                 for y in range(16) for group in range(2) for lane in range(4)]
        put(atlas+char*256, struct.pack('<128H', *words))

    def offset(x, y):
        return ((y//16)*25+x//16)*256+(y % 16)*16+x % 16

    background = [0]*96000
    for y in range(240):
        for x in range(400):
            background[offset(x, y)] = ((x*13+y*37) ^ 0x5aa5) & 65535
    before = list(background)
    depth = [65535]*96000
    for (x, y), value in depth_pixels.items():
        assert cells[(y//16)*50+x//8] == 255, 'UI must leave the retained 3D sample visible'
        before[offset(x, y)], depth[offset(x, y)] = 0xf800, value
    after = list(before)
    opaque = transparent = 0
    for y in range(240):
        for x in range(400):
            char = cells[(y//16)*50+x//8]
            if char == 255:
                continue
            pixel = ui.glyph(char, x % 8, y % 16)
            if pixel & 1:
                after[offset(x, y)] = (((pixel >> 11) & 31) << 11
                    | ((pixel >> 6) & 31) << 6 | ((pixel >> 10) & 1) << 5 | (pixel >> 1) & 31)
                opaque += 1
            else:
                transparent += 1
    final = list(after)
    for x, y in depth_pixels:
        final[offset(x, y)] = 0x001f
    put(0x100000, struct.pack('<96000H', *background))
    put(0x140000, struct.pack('<96000H', *([65535]*96000)))
    put(0x51000, target_params(0x100000, 0x140000))
    put(0x51020, struct.pack('<Q', grid//128 | ((atlas//65536) << 16))+bytes(24))
    cb = CommandBuffer()
    cb.emit('DMA', 288, 0x51000, 32)
    cb.emit('DMA', 272, 0x4000, 64)
    cb.emit('DMA', 280, 0x4100, 32)
    cb.emit('DMA', 320, 0x4280, 128)
    cb.emit('DMA', 292, 0x51020, 32)
    cb.emit('WAIT', a=3)
    cb.emit('FLUSH')
    cb.emit('SET_TARGET', 288)
    cb.emit('SET_GLOBAL', 272)
    cb.emit('SET_RENDER', 280)
    cb.emit('DMA_MESHLET', 0, 0xa000, size)
    cb.emit('WAIT', a=1)
    cb.emit('DRAW_MESHLET', 0)
    cb.emit('WAIT', a=3)
    cb.emit('FLUSH')
    while len(cb.records) < 31:
        cb.emit('NOP')
    cb.emit('UI_GRID', 292)
    cb.emit('DMA_MESHLET', 1, 0xa500, size)
    cb.emit('WAIT', a=1)
    cb.emit('DRAW_MESHLET', 1)
    cb.emit('WAIT', a=3)
    cb.emit('FLUSH')
    cb.emit('END')
    commands = cb.encode()
    put(0x40000, commands+bytes((-len(commands)) % 128))
    # Sentinels include unallocated neighbors, not only initialized resources.
    for address in (0xffffc, 0x12ee00, 0x13fffc, 0x16ee00, grid-4, atlas-4):
        if address not in seed:
            seed[address] = 0xcafebabe
    protected = {a: v for a, v in seed.items()
                 if not (0x100000 <= a < 0x12ee00 or 0x140000 <= a < 0x16ee00)}
    (out / 'seed.txt').write_text(''.join(f'{a:x} {v:08x}\n' for a, v in sorted(seed.items())))
    (out / 'protected.hex').write_text(''.join(f'{a:08x}{v:08x}\n' for a, v in sorted(protected.items())))
    for name, image in [('before', before), ('after', after), ('final', final), ('depth', depth)]:
        (out / (name+'.hex')).write_text(''.join(f'{image[i] | image[i+1] << 16:08x}\n' for i in range(0, 96000, 2)))
    (out / 'commandbuffer.txt').write_text(cb.text())
    return dict(command_bytes=len(commands), protected=len(protected), grid=grid, atlas=atlas,
                geometry_pixels=len(depth_pixels), opaque=opaque, transparent=transparent)


TB = r'''
integer cycle=0,mode=0,fd,rc,a,v,j,draws=0,ui_started=0,ui_checked=0;
integer requests=0,terminals=0,beats=0,ui_requests=0,ui_terminals=0,ui_beats=0;
integer last_terminal=0,abort_cycle=0,physical_writes=0,protected_checks=0;
integer dma_after_ui=0,ui_issued=0;
reg[31:0]before_image[0:47999],after_image[0:47999],final_image[0:47999],depth_image[0:47999];
reg[63:0]protected_words[0:@PROTECTED@-1];

function unknown_write_bytes;
 input[31:0]value;input[3:0]mask;
 begin
  // Fixed slices avoid Icarus $isunknown(dynamic part-select) false positives.
  unknown_write_bytes=(!mask[0]&&((^value[7:0])===1'bx))||
   (!mask[1]&&((^value[15:8])===1'bx))||(!mask[2]&&((^value[23:16])===1'bx))||
   (!mask[3]&&((^value[31:24])===1'bx));
 end
endfunction

// Called on actual unmasked SDRAM pin writes, before the model updates memory.
task physical_write;
 input[20:0]location;input[31:0]value;input[3:0]mask;
 integer address;
 begin
  address=4*((location&31)|(((location>>19)&3)<<5)|(((location>>5)&16383)<<7));
  if(!((address>=32'h100000&&address<32'h12ee00)||(address>=32'h140000&&address<32'h16ee00)))
   $fatal(1,"UI guard: physical write outside target %h",address);
  if(dut.ui_active&&address>=32'h140000)$fatal(1,"UI guard: physical depth write");
  if(unknown_write_bytes(value,mask))$fatal(1,"UI guard: unknown physical write");
  physical_writes=physical_writes+1;
 end
endtask

task check_frame;
 input integer phase_id;
 integer w,l;
 reg[31:0]expected_word,actual;
 begin
  for(w=0;w<48000;w=w+1)begin
   actual=pin.mem[native_word(32'h100000+w*4)];
   expected_word=phase_id==0?before_image[w]:phase_id==1?after_image[w]:final_image[w];
   if(phase_id==3)begin
    for(l=0;l<2;l=l+1)
     if(actual[l*16+:16]!==before_image[w][l*16+:16]&&actual[l*16+:16]!==after_image[w][l*16+:16])
      $fatal(1,"UI guard: abort pixel corruption word=%0d",w);
   end else if(actual!==expected_word)
    $fatal(1,"UI transition phase=%0d word=%0d actual=%h expected=%h",phase_id,w,actual,expected_word);
   if(pin.mem[native_word(32'h140000+w*4)]!==depth_image[w])$fatal(1,"UI transition depth word=%0d",w);
  end
  for(w=0;w<@PROTECTED@;w=w+1)begin
   if(pin.mem[native_word(protected_words[w][63:32])]!==protected_words[w][31:0])
    $fatal(1,"UI guard: modified CB/asset/neighbor %h",protected_words[w][63:32]);
   protected_checks=protected_checks+1;
  end
 end
endtask

always@(negedge clk)if(!reset)begin
 ce=cycle%7!=3;
 if(!abort&&((mode==1&&ui_beats>0&&ui_terminals<ui_requests)||
             (mode==2&&ui_requests==8&&ui_beats>=113&&ui_terminals<8)))begin
  abort=1;abort_cycle=cycle;
 end
end
always@(posedge clk)begin
 cycle=cycle+1;
 if(cycle>50000)$fatal(1,"UI transition watchdog mode=%0d cp=%0d ui=%0d req=%0d term=%0d",mode,dut.processor.state,dut.ui.state,ui_requests,ui_terminals);
 if(!reset)begin
  if(ro_response_error||fb_r_response_error||fb_w_response_error)$fatal(1,"UI transition MC error");
  if(dut.start_fire)begin
   if(draws==1&&(!ui_checked||dma_after_ui==0))$fatal(1,"DRAW resumed before UI/reload");
   draws=draws+1;
  end
  if(ce&&dut.ui_start)begin
   if(draws!=1)$fatal(1,"UI before first DRAW");
   check_frame(0);ui_started=ui_started+1;
  end
  if(dut.ui_active&&dut.ui.cmd_valid&&dut.ui.cmd_ready&&dut.ui.cmd_write)ui_issued=ui_issued+1;
  if(!abort&&dut.ui_done&&!dut.ui_active&&ui_started&&!ui_checked)begin
   if(ui_terminals!=8||ui_requests!=8)$fatal(1,"UI done before final write terminal");
   check_frame(1);ui_checked=1;
  end
  if(ce&&ui_checked&&dut.work_valid&&dut.work_ready&&dut.work_mesh)dma_after_ui=dma_after_ui+1;
  if(fb_w_request_valid&&fb_w_request_ready)begin
   if(fb_w_count!=3)$fatal(1,"Unexpected write length");
   if(!((fb_w_address>=32'h80000&&fb_w_address+64<=32'h97700)||
        (fb_w_address>=32'ha0000&&fb_w_address+64<=32'hb7700)))
    $fatal(1,"UI guard: request outside target %h",fb_w_address);
   requests=requests+1;
   if(dut.ui_active)begin
    if(fb_w_address>=32'ha0000)$fatal(1,"UI guard: requested depth write");
    ui_requests=ui_requests+1;
   end
  end
  if(fb_w_data_ready)begin beats=beats+1;if(dut.ui_active)ui_beats=ui_beats+1;end
  if(fb_w_response_valid&&fb_w_response_last)begin
   terminals=terminals+1;last_terminal=cycle;
   if(dut.ui_active)ui_terminals=ui_terminals+1;
  end
  if(dut.ui_active&&fb_r_request_valid&&fb_r_request_ready&&fb_r_address>=32'ha0000)
   $fatal(1,"UI guard: requested depth read");
 end
end
initial begin
 if(unknown_write_bytes(32'h4270426d,4'h0)!==0||
    unknown_write_bytes(32'hxx70426d,4'h0)!==1||
    unknown_write_bytes(32'hxx70426d,4'h8)!==0||
    unknown_write_bytes(32'hxxxxxxxx,4'hf)!==0)$fatal(1,"Write guard self-test");
 rc=$value$plusargs("MODE=%d",mode);reset=1;ce=1;output_ready=1;
 fd=$fopen("seed.txt","r");if(!fd)$fatal(1,"seed");
 while(!$feof(fd))begin rc=$fscanf(fd,"%h %h\n",a,v);if(rc==2)pin.seed_word(native_word(a),v);end
 $fclose(fd);
 $readmemh("before.hex",before_image);$readmemh("after.hex",after_image);
 $readmemh("final.hex",final_image);$readmemh("depth.hex",depth_image);$readmemh("protected.hex",protected_words);
 repeat(4)@(negedge clk);reset=0;submit_commands(32'h40000,@BYTES@);
 if(mode!=0)begin
  while(!abort||dut.ui_active||!dut.dma_idle||dut.memory_busy)@(negedge clk);
  if(!busy||!fault||device_read_data[0]!==1'b1||draws!=1||ui_checked!=0||ui_started!=1)
   $fatal(1,"Abort incorrectly reported normal completion");
  if(ui_requests<1||ui_terminals!=ui_requests||requests!=terminals||beats!=requests*16)
   $fatal(1,"Abort failed terminal/data conservation");
  if(mode==2&&ui_requests!=8)$fatal(1,"Final-burst abort coverage");
  check_frame(mode==2?1:3);
  j=requests;repeat(12)@(negedge clk);
  if(requests!=j||!busy||!fault||dut.memory_busy)$fatal(1,"Work escaped abort drain");
 end else begin
  while(busy)@(negedge clk);
  if(fault||device_read_data[1]||draws!=2||ui_started!=1||!ui_checked||dma_after_ui!=1||
     requests!=terminals||beats!=requests*16||dut.memory_busy||!dut.dma_idle)
   $fatal(1,"Normal transition conservation");
  check_frame(2);
  if(cycle<=last_terminal)$fatal(1,"END preceded final framebuffer terminal");
 end
 if(physical_writes==0||protected_checks==0||ui_issued==0)$fatal(1,"Missing monitor coverage");
 $display("PASS UI_DRAW mode=%0d cycles=%0d draws=%0d UIwrites=%0d/%0d physical=%0d guards=%0d abort=%0d terminal=%0d",mode,cycle,draws,ui_terminals,ui_requests,physical_writes,protected_checks,abort_cycle,last_terminal);
 $finish;
end
endmodule
'''


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--export', type=Path, required=True)
    parser.add_argument('--peer', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--layout', choices=['high-grid', 'high-atlas'], default='high-atlas')
    parser.add_argument('--modes', type=int, nargs='+', choices=[0, 1, 2], default=[0, 1, 2])
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    original = args.export.read_bytes()
    identity = hashlib.sha256(original).hexdigest()
    (out / 'dut.v').write_bytes(original)
    meta = assets(out, args.layout)
    _, info = wrapper(re.sub(r'\b(input|output)(?=\[)', r'\1 ', original.decode()))
    declarations = []
    for port in info['ports']:
        driven = port['direction'] == 'input' and not port['name'].startswith(('ro_', 'fb_r_', 'fb_w_'))
        declarations.append(('reg ' if driven else 'wire ')
            + (f'[{port["bits"]-1}:0] ' if port['bits'] > 1 else '') + port['name']
            + ('=0;' if driven else ';'))
    continuation = load('transition_mc', Path(__file__).with_name('command_continuation.py'))
    tb = continuation.TB[:continuation.TB.index('integer cycle=0')]
    tb = tb.replace('@PORTS@', '\n'.join(declarations)).replace('gpu_v2_vliw_command dut(.*);', TASKS+'\ngpu_v2_vliw_command dut(.*);')
    tb += TB.replace('@BYTES@', str(meta['command_bytes'])).replace('@PROTECTED@', str(meta['protected']))
    (out / 'tb.v').write_text(tb)
    files, peers = [], {}
    for name in ['bridge.v', 'combination.v', 'controller.v', 'cpu_v3_memory_arbiter.v', 'pins.v', 'shared_port.v', 'ports.vh']:
        source = args.peer / name
        data = source.read_bytes()
        peers[name] = hashlib.sha256(data).hexdigest()
        if name == 'pins.v':
            text = data.decode()
            anchor = 'location = {burst_bank, rows[burst_bank], col};'
            assert text.count(anchor) == 1
            text = text.replace(anchor, anchor+'\n                if(dqm!=15)tb.physical_write(location,dq,dqm);')
            (out / name).write_text(text)
        else:
            shutil.copyfile(source, out / name)
        if name.endswith('.v'):
            files.append(out / name)
    commands = [('compile', [os.environ['IVERILOG_EXE'], '-g2012', '-D__ICARUS__', '-I', out, '-s', 'tb', '-o', out / 'sim.vvp', out / 'dut.v', *files, out / 'tb.v', Path(os.environ['GOWIN_HOME']) / 'IDE/simlib/gw2a/prim_sim.v'])]
    commands += [(f'run-{mode}', [os.environ['VVP_EXE'], out / 'sim.vvp', f'+MODE={mode}']) for mode in args.modes]
    passes = []
    for name, command in commands:
        assert hashlib.sha256((out / 'dut.v').read_bytes()).hexdigest() == identity
        with (out / (name+'.log')).open('w') as log:
            result = subprocess.run(list(map(str, command)), cwd=out, stdout=log, stderr=subprocess.STDOUT, timeout=180)
        text = (out / (name+'.log')).read_text(errors='replace')
        if result.returncode:
            raise RuntimeError(text[-2500:])
        passes += [line for line in text.splitlines() if line.startswith('PASS ')]
    receipt = dict(status='pass', export_sha256=identity, assets=meta, passes=passes,
        peer_sha256=peers, test_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        instrumented_pin_sha256=hashlib.sha256((out / 'pins.v').read_bytes()).hexdigest(),
        scope='Native GPU+shared MC+pin model; bounded standalone CL2, not CPU/Display/CL3 board qualification')
    (out / 'receipt.json').write_text(json.dumps(receipt, indent=2))
    print(json.dumps(receipt))


if __name__ == '__main__':
    main()
