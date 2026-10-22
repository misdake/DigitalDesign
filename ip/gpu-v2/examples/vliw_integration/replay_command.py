"""Execute a complete prewritten DRAM command buffer through the real memory peer.

Uses replay_export's scenes and independent numerical/pixel audit. The observer
loads expected records when the GPU starts a meshlet; it never drives GPU work.
"""
from pathlib import Path
import copy
import hashlib
import json
import re
import struct
import sys

import replay_export as replay
from device_bus import TASKS

HERE = Path(__file__).resolve().parent
COMMAND_BASE = 0x40000


def align(value, size=128):
    return (value+size-1)//size*size


class CommandBuffer:
    OPS = dict(NOP=0, DMA=1, DMA_MESHLET=2, DRAW_MESHLET=3, WAIT=4,
               SET_GLOBAL=5, SET_RENDER=6, FLUSH=7, CLEAR=8, TEX_INVALIDATE=9, SET_TARGET=10, END=15)

    def __init__(self):
        self.records = []

    def emit(self, name, slot=0, a=0, b=0, c=0):
        op = self.OPS[name]
        if name == 'DMA':
            assert b in (32,64,128) and a % b == 0 and slot % 4 == 0
            word = (op<<28) | ((a//32)<<10) | ((slot//4)<<3) | ((32,64,128).index(b)<<1)
        elif name == 'DMA_MESHLET':
            assert a % 128 == 0 and b in range(128,1025,128) and slot in (0,1)
            word = (op<<28) | ((a//128)<<12) | ((b//128-1)<<1) | slot
        elif name == 'DRAW_MESHLET':
            word = (op<<28) | slot
        elif name == 'CLEAR':
            assert slot in (0,1) and 0 <= a <= 65535
            word = (op<<28) | (slot<<16) | a
        elif name in ('SET_GLOBAL','SET_RENDER','SET_TARGET'):
            assert slot % 4 == 0
            word = (op<<28) | (slot//4)
        else:
            word = (op<<28) | (a if name == 'WAIT' else 0)
        self.records.append(dict(name=name, slot=slot, args=[a,b,c], word=word))

    def encode(self):
        return b''.join(struct.pack('<I', r['word']) for r in self.records)

    def text(self):
        return '\n'.join(f'{4*i:04x}: {r["word"]:08x} {r["name"]:<16} slot={r["slot"]:3} '
                         + ' '.join(f'{v:#x}' for v in r['args']) for i,r in enumerate(self.records))+'\n'


def target_params(color, depth):
    assert color % 65536 == 0 and depth % 65536 == 0
    assert 0 <= color <= 0x800000-192000 and 0 <= depth <= 0x800000-192000
    assert color+192000 <= depth or depth+192000 <= color
    return struct.pack('<Q', (color//512) | ((depth//512)<<14)) + bytes(24)


def texture_word(base, size_log2, full_mip, filter_code):
    """Current RGB565 descriptor; legal ranges and accessible bytes are host contracts."""
    return (base // 128) | (size_log2 << 16) | (int(full_mip) << 20) | (filter_code << 21)


def frame_assets(out, metas, configured_bench):
    values = json.loads((out/'input.json').read_text())['instances']
    seed = {int(a,16):int(v,16) for a,v in (line.split() for line in (out/'seed.txt').read_text().splitlines())}
    seed = {a:v for a,v in seed.items() if 0x1000<=a<0x5000 or 0x100000<=a<0x100000+400*240*2
            or 0x140000<=a<0x140000+400*240*2}
    def put(address,data):
        assert address%4==0 and len(data)%4==0
        for i in range(0,len(data),4):seed[address+i]=int.from_bytes(data[i:i+4],'little')
    def native(name):
        match=re.search(r'\b'+name+r'=([^;,]+)',configured_bench);assert match,name
        text=re.sub(r"\d+'h([0-9a-fA-F]+)",lambda m:str(int(m[1],16)),match[1])
        assert re.fullmatch(r'[0-9xabcdefABCDEF()<>|+* -]+',text),(name,text)
        return eval(text,{'__builtins__':{}})
    glob=[native('cfg_far_z'),(native('light_x')&65535)|((native('light_y')&65535)<<16),
          (native('light_z')&65535)|((native('ray_x')&65535)<<16),
          (native('ray_y')&65535)|((native('ray_k')&65535)<<16),native('ambient')|(native('directional')<<16),
          native('context_mode')|(native('context_code')<<2)|(native('context_epoch')<<16)]
    put(0x50000,struct.pack('<16I',*glob,*([0]*10)))
    put(0x50140,target_params(0x100000,0x140000))
    address=0x240000;records=[];states=[];previous=None;groups=[];payload_bytes=0
    for i,instance in enumerate(values):
        image={int(a):int(v,16) for a,v in (line.split() for line in (out/f'task{i}/dma.txt').read_text().splitlines())}
        page=i%2;rows=[a-page*128 for a in image if page*128<=a<(page+1)*128]
        data=b''.join(struct.pack('<Q',image.get(page*128+j,0)) for j in range(max(rows)+1))
        padded=align(len(data));metadata=b''.join(struct.pack('<Q',image[256+page*4+k]) for k in range(4))
        put(address,metadata+bytes(96)+data+bytes(padded-len(data)))
        records.append((address,padded));payload_bytes+=len(data);address+=128+padded
        rgba=instance.get('mesh_uniform_color',[255,255,255,255])
        assert len(rgba)==4 and all(0<=v<=255 for v in rgba)
        uniform=list(instance['uniform'][:22])+[int.from_bytes(bytes(rgba),'little'),0]+[0]*8
        binding=(bool(instance.get('unlit',False)),tuple(uniform))
        if binding!=previous:
            gi=len(groups);ia=0x53000+gi*256;ra=ia+128
            words=list(binding[1]);assert 22<=len(words)<=32
            words += [0]*(32-len(words))
            put(ia,struct.pack('<32I',*(v&0xffffffff for v in words)))
            draw = native('draw_config')
            render=[texture_word(0x1000, (draw>>20)&15, bool(draw&(1<<24)), draw>>25),native('specular'),
                    native('depth_func')|(native('depth_write')<<3)|(native('blend')<<4)|(int(binding[0])<<5),
                    native('cfg_lod_bias')&255]
            put(ra,struct.pack('<8I',*render,0,0,0,0))
            groups.append(dict(binding=binding,instance=ia,render=ra,texture=render[0]&0x1fffff))
        states.append(len(groups)-1);previous=binding
    cb=CommandBuffer();cb.emit('WAIT',a=2);cb.emit('FLUSH')
    cb.emit('DMA',272,0x50140,32);cb.emit('WAIT',a=1);cb.emit('SET_TARGET',272)
    cb.emit('CLEAR',0,0);cb.emit('CLEAR',1,65535)
    cb.emit('DMA',272,0x50000,64);cb.emit('WAIT',a=1);cb.emit('SET_GLOBAL',272)
    old_group=None
    def apply_group(gi):
        nonlocal old_group
        g=groups[gi];old=groups[old_group] if old_group is not None else None
        if old is None or old['binding'][1]!=g['binding'][1]:cb.emit('DMA',320,g['instance'],128)
        material=old is None or old['binding'][0]!=g['binding'][0]
        if material:cb.emit('DMA',280,g['render'],32)
        cb.emit('WAIT',a=3 if material else 1)
        if material:cb.emit('SET_RENDER',280)
        # Frame assets may have been rewritten at retained physical addresses.
        # Ordinary resource switches retain immutable lines under physical tags.
        if old is None:cb.emit('TEX_INVALIDATE')
        old_group=gi
    apply_group(states[0]);cb.emit('DMA_MESHLET',0,*records[0]);cb.emit('WAIT',a=1)
    for i in range(len(records)):
        if i+1<len(records):cb.emit('DMA_MESHLET',(i+1)%2,*records[i+1])
        if FRAME_STRESS and i<2:
            while len(cb.records)%32!=31:cb.emit('NOP')
        cb.emit('DRAW_MESHLET',i%2)
        if i+1<len(records):
            cb.emit('WAIT',a=1)
            if states[i+1]!=old_group:apply_group(states[i+1])
    cb.emit('WAIT',a=2);cb.emit('FLUSH');cb.emit('END')
    commands=cb.encode();put(COMMAND_BASE,commands+bytes((-len(commands))%128))
    (out/'seed.txt').write_text(''.join(f'{a:08x} {v:08x}\n' for a,v in sorted(seed.items())))
    (out/'commandbuffer.bin').write_bytes(commands+bytes((-len(commands))%128));(out/'commandbuffer.txt').write_text(cb.text())
    memory=dict(command_base=COMMAND_BASE,command_valid_bytes=len(commands),command_allocation_bytes=align(len(commands)),
                geometry_address_end=address,mesh_records_bytes=address-0x240000,payload_bytes=payload_bytes,
                inline_metadata_bytes=32*len(values),record_padding_bytes=96*len(values),global_bytes=64,
                instance_bytes=128*len(groups),render_bytes=32*len(groups),clear_parameter_bytes=64,
                clear_bytes=2*400*240*2)
    (out/'commandbuffer.json').write_text(json.dumps(dict(base=COMMAND_BASE,bytes=len(commands),commands=cb.records,
          draws=list(range(len(values))),meshlets=len(values),memory=memory),indent=2))
    return len(commands),len(values)

FRAME_STRESS=False

def command_bench(text, bunny, metas, out):
    byte_count, draw_count = frame_assets(out, metas, text)
    text = text.replace('gpu_v2_vliw_sdram dut(.*);', 'gpu_v2_vliw_command dut(.*);')
    text = text.replace(bunny.TASKS, '')
    # Retain activity and wall classification; remove host descriptor/DMA code.
    old = bunny.MC_MONITOR.replace('@NJ@', str(len(metas)))
    counters = old[old.index(' if(ro_request_valid&&ro_request_ready)'):]
    counters = counters.replace(' if(frame_begin>0)', ' if(frame_begin>0)')
    declarations = '''
reg[2:0]device_index=0;reg[3:0]device_channel=0;reg device_read_enable=0,device_write_enable=0;reg[15:0]device_write_data=0;
wire[15:0]device_read_data;wire busy;
reg[31:0]command_base=32'h40000,command_bytes=@CB_BYTES@;

wire[31:0]retired_commands=dut.processor.diagnostic_pc/4+1;
integer dma_overlap_cycles=0,front_wait_cycles=0,all_wait_cycles=0;
integer queued_draws=0,issued_meshlets=0,completed_draws=0,mesh_dma_overlap=0;
integer draw_overlap=0,draw_mailbox_wait=0;
integer ro_requests=0,fb_reads=0,fb_writes=0,ro_beats=0,fb_r_beats=0,fb_w_beats=0;
integer frame_begin=0,render_begin=0,render_end=0,frame_end=0,mesh_start=0,mesh_front_end=0;
integer vw=0,vwait=0,tw=0,twait=0,prep=0,ib=0,rb=0,nodes=0,walk=0,leaf=0,row=0,rprep=0,rcredit=0;
integer pixel_launch=0,covered_launch=0,empty_scan=0,numeric_reject=0,host_start_pending=0;
integer wall[0:7],wc;reg mesh_reported=1,force_pause=0;
integer raw_seed_addr,seed_handle,seed_rc;reg[31:0]seed_value,dump_value;
wire dma_we=dut.dma_we,dma_ready=dut.dma_ready;
wire start_ready=dut.start_ready;
always @(posedge clk)if(!reset)begin
 if(dut.work_busy&&dut.input_active)dma_overlap_cycles=dma_overlap_cycles+1;
 if(dut.dma_we&&dut.input_active&&dut.dma_addr<256)mesh_dma_overlap=mesh_dma_overlap+1;
 if(ce&&dut.processor.state==2&&dut.processor.opcode==3&&!meshlet_ready)front_wait_cycles=front_wait_cycles+1;
 if(ce&&dut.processor.state==7&&dut.processor.wait_mask[1]&&!dut.compute_idle)all_wait_cycles=all_wait_cycles+1;
 if(dut.start_fire)begin
  queued_draws=queued_draws+1;issued_meshlets=issued_meshlets+1;
  if(!frontend_idle)draw_overlap=draw_overlap+1;
 end
 if(ce&&meshlet_done)completed_draws=completed_draws+1;
'''.replace('@CB_BYTES@', str(byte_count))
    text = text.replace(old, declarations+counters+'reg[3:0]old_cp_state=0;\nalways @(posedge clk)begin\n #1;\n if(!reset&&dut.processor.state==2&&old_cp_state!=2)\n  $display("CPCOMMIT cycle=%0d offset=%0d opcode=%0d",cycle,dut.processor.diagnostic_pc,dut.processor.opcode);\n old_cp_state=dut.processor.state;\nend\n')
    observer = '''
 if(!reset&&ce&&task_id>=0&&(saw_done||meshlet_done)&&!mesh_reported)begin
  if(vi!=nv*5||ti!=nt||published_vertices!=nv)$fatal(1,"automatic meshlet computation counts");
  mesh_reported=1;mesh_front_end=cycle;render_end=cycle;
  $display("MESH task=%0d cycles=%0d end=%0d V=%0d T=%0d P=%0d exported=%0d",task_id,cycle-mesh_start,cycle,vi/5,ti,np,observed_packets[task_id]);
 end
 if(!reset&&start_taken)begin
  if(!mesh_reported)$fatal(1,"new page started before previous frontend retired");
  task_id=task_id+1;vi=0;ti=0;pi=0;saw_done=0;mesh_reported=0;mesh_start=cycle;
  if(task_id==0)render_begin=cycle;
  case(task_id)
@CASES@
   default:$fatal(1,"too many automatic meshlets");
  endcase
  expect_task(task_id,np);
  $display("START task=%0d cycle=%0d",task_id,cycle);
 end
'''
    cases = []
    for i, m in enumerate(metas):
        cases.append(f'   {i}:begin nv={m["vertices"]};nt={m["triangles"]};np={m["packets"]};\n'
            f'    $readmemh("task{i}/vertices.hex",vertices,0,nv*5-1);\n'
            f'    $readmemh("task{i}/admissions.hex",admissions,0,nt-1);\n'
            f'    if(np>0)begin $readmemh("task{i}/descriptors.hex",descriptors,0,np-1);\n'
            f'     $readmemh("task{i}/words.hex",words,0,np*16-1);end end')
    observer = observer.replace('@CASES@', '\n'.join(cases))
    needle = 'start_taken=ce&&!reset&&start&&start_ready;'
    assert text.count(needle) == 1
    text = text.replace(needle, 'start_taken=dut.start_fire;\n'+observer)
    a = text.index('initial begin\n frame_begin=0;')
    b = text.index('initial begin #', a)
    dump = text[text.index(' ff=$fopen("memory.mem","w");', a):text.index(' if(job_completions!=', a)]
    reports = text[text.index(' $display("PASS BUNNY', a):text.index(' $fclose(fd);$fclose(bd);$finish;', a)]
    reports = reports.replace('dma_overlap,host_start_pending', 'dma_overlap_cycles,host_start_pending')
    initial = '''initial begin
 for(j=0;j<8;j=j+1)wall[j]=0;
 fd=$fopen("front.txt","w");bd=$fopen("back.txt","w");
 seed_handle=$fopen("seed.txt","r");if(!seed_handle)$fatal(1,"seed missing");
 while(!$feof(seed_handle))begin seed_rc=$fscanf(seed_handle,"%h %h",raw_seed_addr,seed_value);
  if(seed_rc==2)pin.seed_word(native_word(raw_seed_addr),seed_value);
 end $fclose(seed_handle);
 tick();tick();reset=0;frame_begin=cycle;
 submit_commands(command_base,command_bytes);
 while(busy)tick();frame_end=cycle;
 if(device_read_data[1]||fault||issued_meshlets!=@NJ@||completed_draws!=@ND@||!mesh_reported)
  $fatal(1,"command frame result error%0d offset%0d meshlets%0d draws%0d",device_read_data[1],dut.processor.diagnostic_pc,issued_meshlets,completed_draws);
 inputs_done=1;
@DUMP@
@REPORTS@
 $display("COMMAND commands=%0d draws=%0d meshlets=%0d DMAoverlap=%0d meshDMAwords=%0d waitFront=%0d waitAll=%0d",retired_commands,queued_draws,issued_meshlets,dma_overlap_cycles,mesh_dma_overlap,front_wait_cycles,all_wait_cycles);
 $display("COMMAND_OVERLAP draws=%0d mailboxWait=%0d",draw_overlap,draw_mailbox_wait);
 repeat(5)begin tick();if(busy)$fatal(1,"command busy reasserted");end

 $fclose(fd);$fclose(bd);$finish;
end
'''
    for key, value in dict(NJ=len(metas), ND=draw_count, DUMP=dump, REPORTS=reports).items():
        initial = initial.replace('@'+key+'@', str(value))
    return (text[:a]+initial+text[b:]).replace("gpu_v2_vliw_command dut(.*);", TASKS+"\ngpu_v2_vliw_command dut(.*);")


def main():
    writer_hash = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    global FRAME_STRESS
    stress = '--stress' in sys.argv
    FRAME_STRESS=stress
    if stress:
        sys.argv.remove('--stress')
    small_materials='--small-materials' in sys.argv
    if small_materials:sys.argv.remove('--small-materials')
    original_load = replay.load
    original_run = replay.run_checked_simulation
    state = {}

    def load(name, path):
        module = original_load(name, path)
        if name == 'export_replay_transport':
            original_adapt = module.adapt_oracle
            module.adapt_oracle = lambda oracle: original_adapt(oracle, mesh_uniform_color=True)
        if name == 'export_replay_bunny':
            state['bunny'] = module
            original_bench = module.bench

            def bench(s, v, metas, max_cycles):
                state['metas'] = copy.deepcopy(metas)
                return original_bench(s, v, metas, max_cycles)
            module.bench = bench
        if name == 'mixed_material_scene' and small_materials:
            original_instances=module.instances
            def limited_instances(*args):
                values,info,shading=original_instances(*args)
                room=[v for v in values if v['group']=='box']
                rgb=[v for v in values if v['group']=='vertex_color']
                bunny=[v for v in values if v['group']=='bunny'][:2]
                values=[room[0],*rgb,*bunny]
                for i,v in enumerate(values):v.update(slot=i%2,generation=i%16)
                info.update(scene='one_textured_face_RGB_triangle_two_lit_bunny_meshlets',
                            submitted_triangles=sum(len(v['triangles']) for v in values),
                            submitted_vertices=sum(len(v['vertices']) for v in values),meshlets=len(values),
                            subset=True,note='Bounded material-transition fixture; not the full rabbit/room.')
                return values,info,shading
            module.instances=limited_instances
        return module

    def run(execute, command, vvp, out, source, export_sha, timeout):
        path = out/'tb.v'
        bench = command_bench(path.read_text(), state['bunny'], state['metas'], out)
        if stress:
            bench = bench.replace('localparam STRESS=0,', 'localparam STRESS=1,')
        path.write_text(bench)
        return original_run(execute, command, vvp, out, source, export_sha, timeout)

    replay.load = load
    replay.run_checked_simulation = run
    replay.main()
    assert hashlib.sha256(Path(__file__).read_bytes()).hexdigest() == writer_hash
    out = Path(sys.argv[sys.argv.index('--output')+1]).resolve()
    path = out/'receipt.json'
    receipt = json.loads(path.read_text())
    log = (out/'run.log').read_text()
    profile = {}
    for prefix in ('COMMAND ', 'COMMAND_OVERLAP '):
        line = next(line for line in log.splitlines() if line.startswith(prefix))
        profile[prefix.strip()] = {k: int(v) for k, v in re.findall(r'(\w+)=(\d+)', line)}
    trace = [{k: int(v) for k, v in re.findall(r'(\w+)=(\d+)', line)}
             for line in log.splitlines() if line.startswith('CPCOMMIT ')]
    manifest = json.loads((out/'commandbuffer.json').read_text())
    assert len(trace) == len(manifest['commands'])
    assert [t['offset'] for t in trace] == list(range(0, manifest['bytes'], 4))
    assert [t['opcode'] for t in trace] == [CommandBuffer.OPS[c['name']] for c in manifest['commands']]
    receipt['info']['memory'] = manifest['memory']
    receipt.update(execution='autonomous DRAM command buffer', command_writer_sha256=writer_hash,
                   commandbuffer_sha256=hashlib.sha256((out/'commandbuffer.bin').read_bytes()).hexdigest(),
                   command_profile=profile, command_trace=trace, stress=stress, small_materials=small_materials)
    path.write_text(json.dumps(receipt, indent=2))


if __name__ == '__main__':
    main()
