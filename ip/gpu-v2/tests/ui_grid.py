"""Bounded UI_GRID replay through the actual GPU, shared MC and SDRAM pin model."""
from pathlib import Path
import argparse,hashlib,importlib.util,json,os,re,shutil,struct,subprocess,sys
ROOT=Path(__file__).resolve().parents[3]
import sys
sys.path.insert(0,str(ROOT/'hardware/vendor/gowin/tests'))
from stream_contract import load_contract, stage_peer
sys.dont_write_bytecode=True
sys.path.insert(0,str(ROOT/'ip/gpu-v2/examples/vliw_integration'))
from replay_command import CommandBuffer,target_params
from device_bus import TASKS
sys.path.insert(0,str(ROOT/'ip/gpu-v2/examples/postquad_ii4'))
from wrapper import wrapper

def glyph(c,x,y):
    a=(x+y+c)%5!=0 and (x!=7 or c%2==0)
    return (((c*3+x+y)&31)<<11)|(((c*7+x*2)&31)<<6)|(((c+y*3)&31)<<1)|a

def assets(out,case,repeat=1,ranges=None):
    seed={}
    def put(a,data):
        assert a%4==0 and len(data)%4==0
        seed.update((a+i,int.from_bytes(data[i:i+4],'little')) for i in range(0,len(data),4))
    cells=bytearray([255]*750)
    if case=='sparse':
        for p,c in [(0,0),(3,1),(48,2),(49,3),(50,4),(200,1),(748,2),(749,254)]:cells[p]=c
    if case=='dense':cells[:100]=bytes(i%5 for i in range(100))
    if case=='numeric':cells[:3]=bytes([0,1,2])
    calls=ranges if ranges is not None else [(0,750)]*repeat
    visible=[any(first<=i<end for first,end in calls) for i in range(750)]
    put(0x50000,cells+bytes(18))
    for c in [0,1,2,3,4,254]:
        data=[]
        for y in range(16):
            for group in range(2):
                for lane in range(4):data.append(glyph(c,group*4+(lane^ (2 if y&1 else 0)),y))
        put(0x60000+c*256,struct.pack('<128H',*data))
    want=[];background=[]
    for i in range(96000):
        tile,local=divmod(i,256);ty,tx=divmod(tile,25);y=ty*16+local//16;x=tx*16+local%16
        bg=((x*13+y*37)^0x5aa5)&65535
        background.append(bg)
        cell=(y//16)*50+x//8
        char=cells[cell] if visible[cell] else 255
        p=glyph(char,x%8,y%16) if char!=255 else 0
        color=(((p>>11)&31)<<11)|(((p>>6)&31)<<6)|(((p>>10)&1)<<5)|((p>>1)&31) if p&1 else bg
        want.append(color)
    put(0x100000,struct.pack('<96000H',*background))
    put(0x140000,struct.pack('<96000H',*([0xa55a]*96000)))
    put(0x51000,target_params(0x100000,0x140000))
    put(0x51020,struct.pack('<Q',0x50000//128|((0x60000//65536)<<16))+bytes(24))
    cb=CommandBuffer();cb.emit('WAIT',a=3);cb.emit('FLUSH');cb.emit('DMA',272,0x51000,32);cb.emit('WAIT',a=1);cb.emit('SET_TARGET',272);cb.emit('FLUSH')
    cb.emit('DMA',276,0x51020,32);cb.emit('WAIT',a=1)
    # Deliberately cross a command page after UI, checking prefetch restart.
    for _ in range(22):cb.emit('NOP')
    for first,end in calls:
        if ranges is None:cb.emit('UI_GRID',276)
        else:cb.emit('UI_GRID',276,a=first,b=end)
    cb.emit('FLUSH');cb.emit('END');commands=cb.encode()
    put(0x40000,commands+bytes((-len(commands))%128))
    (out/'seed.txt').write_text(''.join(f'{a:x} {v:08x}\n' for a,v in sorted(seed.items())))
    (out/'expected.hex').write_text(''.join(f'{want[i]|want[i+1]<<16:08x}\n' for i in range(0,len(want),2)))
    chars=sum(c!=255 and visible[i] for i,c in enumerate(cells))
    tiles=sum(any(cells[i]!=255 for i in range(max(first,2*t),min(end,2*t+2)))
        for first,end in calls for t in range(first//2,(end+1)//2))
    ui_reads=sum((end-1)//128-first//128+1+2*sum(cells[i]!=255 for i in range(first,end)) for first,end in calls)
    return len(commands),chars,tiles,ui_reads

def rcc_assets(out,path):
    """Replay CPU-produced bytes, retaining a patterned target instead of CLEAR.

    gpu_rcc_ui.rs exports the actual descriptors, grids and command words. Only
    its two initial CLEARs are omitted there; no UI/DMA/fence is reconstructed.
    """
    record=json.loads(path.read_text())
    assets(out,'empty')
    seed={}
    for line in (out/'seed.txt').read_text().splitlines():
        address,value=(int(v,16) for v in line.split())
        for old,new,size in [(0x100000,record['color_base'],192000),
                             (0x140000,record['depth_base'],192000),
                             (0x60000,record['atlas_base'],65536)]:
            if old<=address<old+size:seed[new+address-old]=value
    objects=struct.pack(f'<{len(record["objects"])}H',*record['objects'])
    commands=struct.pack(f'<{len(record["commands"])}I',*record['commands'])
    commands+=bytes((-len(commands))%128)
    for base,data in [(record['object_base'],objects),(record['command_base'],commands)]:
        assert base%4==0 and len(data)%4==0
        seed.update((base+i,int.from_bytes(data[i:i+4],'little')) for i in range(0,len(data),4))
    want=[int(line,16) for line in (out/'expected.hex').read_text().splitlines()]
    chars=tiles=ui_reads=0
    for call in record['calls']:
        first,end=call['first'],call['end']
        start=call['grid']-record['object_base'];cells=objects[start:start+750]
        selected=[i for i in range(first,end) if cells[i]!=255]
        chars+=len(selected);tiles+=len({i//2 for i in selected})
        ui_reads+=(end-1)//128-first//128+1+2*len(selected)
        for cell in selected:
            cy,cx=divmod(cell,50)
            for y in range(16):
                for x in range(8):
                    p=glyph(cells[cell],x,y)
                    if not p&1:continue
                    px,py=cx*8+x,cy*16+y
                    index=((py//16)*25+px//16)*256+(py%16)*16+px%16
                    color=(((p>>11)&31)<<11)|(((p>>6)&31)<<6)|(((p>>10)&1)<<5)|((p>>1)&31)
                    lane=index%2;word=index//2
                    want[word]=(want[word]&~(65535<<(16*lane)))|(color<<(16*lane))
    (out/'seed.txt').write_text(''.join(f'{a:x} {v:08x}\n' for a,v in sorted(seed.items())))
    (out/'expected.hex').write_text(''.join(f'{v:08x}\n' for v in want))
    return len(record['commands'])*4,chars,tiles,ui_reads,record

def main():
    p=argparse.ArgumentParser(__doc__);p.add_argument('--export',type=Path,required=True);p.add_argument('--peer',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--case',choices=['sparse','empty','dense','numeric'],default='sparse');p.add_argument('--modes',nargs='+',type=int,choices=[0,1,2,3],default=[0,1]);p.add_argument('--repeat',type=int,choices=[1,2],default=1)
    p.add_argument('--ranges',nargs='+',help='Consecutive independent UI calls, each first:end (exclusive end)')
    p.add_argument('--rcc-records',type=Path,help='Exact compiled-CPU records from GPU_RCC_UI_RANGE_FIXTURE')
    a=p.parse_args()
    ranges=[tuple(map(int,r.split(':'))) for r in a.ranges] if a.ranges else None
    if ranges is not None and (a.repeat!=1 or any(len(r)!=2 or not 0<=r[0]<r[1]<=750 for r in ranges)):
        p.error('ranges require 0 <= first < end <= 750 and repeat=1')
    if a.rcc_records and (ranges is not None or a.repeat!=1):p.error('RCC records supply their own calls')
    out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    data=a.export.read_bytes();(out/'dut.v').write_bytes(data)
    _,info=wrapper(re.sub(r'\b(input|output)(?=\[)',r'\1 ',data.decode()))
    decl=[]
    for v in info['ports']:
        driven=v['direction']=='input' and not v['name'].startswith(('ro_','fb_r_','fb_w_'))
        decl.append(('reg ' if driven else 'wire ')+(f'[{v["bits"]-1}:0] ' if v['bits']>1 else '')+v['name']+('=0;' if driven else ';'))
    record=None
    if a.rcc_records:length,chars,tiles,ui_reads,record=rcc_assets(out,a.rcc_records)
    else:length,chars,tiles,ui_reads=assets(out,a.case,a.repeat,ranges)
    calls=len(record['calls']) if record else len(ranges) if ranges else a.repeat
    local_dmas=sum(c>>28==1 for c in record['commands']) if record else 2
    color_base=record['color_base'] if record else 0x100000
    s=importlib.util.spec_from_file_location('continuation',Path(__file__).with_name('command_continuation.py'));m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
    tb=m.TB[:m.TB.index('integer cycle=0')].replace('@PORTS@','\n'.join(decl)).replace('gpu_v2_vliw_command dut(.*);',TASKS+'\ngpu_v2_vliw_command dut(.*);')
    tb+=r'''
integer cycle=0,mode=0,fd,rc,a,v,j,ui_cycles=0,wr=0,rd=0,ro=0,ui_ro=0,overlap=0;
integer range_starts=0,local_dmas=0;
reg[31:0]want[0:47999];
integer pause_left=0;reg paused_once=0;
always @(negedge clk)if(!reset)begin
 if(mode==3)begin
  if(wr!=0&&!paused_once)begin pause_left=80;paused_once=1;end
  ce=pause_left==0;if(pause_left!=0)pause_left=pause_left-1;
 end else ce=(mode==0||cycle%7!=3);
 if(mode==2&&rd!=0)abort=1;
end
always @(posedge clk)begin
 cycle=cycle+1;
 if(cycle>100000)$fatal(1,"UI watchdog cp%0d ui%0d scan%0d valid%b loaded%b",dut.processor.state,dut.ui.state,dut.ui.scan,dut.ui.valid,dut.ui.loaded);
 if(!reset)begin
  if(dut.ui_start)range_starts=range_starts+1;
  if(ce&&dut.processor.work_valid&&dut.processor.work_ready&&!dut.processor.work_mesh)local_dmas=local_dmas+1;
  if(@SINGLE_BIND@&&range_starts>0&&range_starts<@CALLS@&&ce&&dut.processor.state==2&&
     (dut.processor.opcode==1||dut.processor.opcode==4||dut.processor.opcode==7))
   $fatal(1,"Repeated DMA/WAIT/FLUSH inside unchanged UI batch");
  if(dut.ui_active)ui_cycles=ui_cycles+1;
  if(fb_r_request_valid&&fb_r_request_ready&&
     (fb_r_address<@COLOR_HALF@||fb_r_address>=@COLOR_HALF@+96000))$fatal(1,"UI read outside color plane");
  if(fb_w_request_valid&&fb_w_request_ready&&
     (fb_w_address<@COLOR_HALF@||fb_w_address>=@COLOR_HALF@+96000))$fatal(1,"UI write outside color plane");
  if(fb_w_request_valid&&fb_w_request_ready)wr=wr+1;
  if(fb_r_request_valid&&fb_r_request_ready)rd=rd+1;
  if(ro_request_valid&&ro_request_ready)ro=ro+1;
  if(dut.ui_active&&ro_request_valid&&ro_request_ready)ui_ro=ui_ro+1;
  if(dut.ui_active&&dut.ui.overlay&&dut.dma_we)overlap=overlap+1;
 end
end
initial begin
 rc=$value$plusargs("MODE=%d",mode);reset=1;ce=1;output_ready=1;
 fd=$fopen("seed.txt","r");
 while(!$feof(fd))begin rc=$fscanf(fd,"%h %h\n",a,v);if(rc==2)begin pin.mem[native_word(a)]=v;pin.marked[native_word(a)]=1;end end
 $fclose(fd);$readmemh("expected.hex",want);
 repeat(4)@(negedge clk);reset=0;
 submit_commands(32'h40000,@BYTES@);
 if(mode==2)begin
  while(!abort||dut.ui_active||!dut.dma_idle||dut.memory_busy)@(negedge clk);
  if(!busy||!fault||wr!=0)$fatal(1,"UI abort ownership");
  $display("PASS UI_ABORT cycles=%0d read_requests=%0d",cycle,rd);$finish;
 end
 while(busy)@(negedge clk);
 if(mode==3&&!paused_once)$fatal(1,"UI write pause was not exercised");
 if(range_starts!=@CALLS@||local_dmas!=@LOCAL_DMAS@)$fatal(1,"UI batch starts%0d local DMA%0d",range_starts,local_dmas);
 for(j=0;j<48000;j=j+1)begin
  if(pin.mem[native_word(32'h100000+j*4)]!==want[j])$fatal(1,"UI color word%0d got%h want%h",j,pin.mem[native_word(32'h100000+j*4)],want[j]);
  if(pin.mem[native_word(32'h140000+j*4)]!==32'ha55aa55a)$fatal(1,"UI changed depth");
 end
 if(wr!=@SECTORS@||rd!=@SECTORS@||ui_ro!=@UI_READS@||device_read_data[1])$fatal(1,"UI conservation rd%0d wr%0d ui_ro%0d",rd,wr,ui_ro);
 $display("PASS UI mode=%0d cycles=%0d ui=%0d tiles=@TILES@ chars=@CHARS@ ro=%0d rd=%0d wr=%0d overlap=%0d ranges=%0d local_dmas=%0d",mode,cycle,ui_cycles,ro,rd,wr,overlap,range_starts,local_dmas);
 $finish;
end
endmodule
'''
    for k,v in dict(BYTES=length,SECTORS=tiles*4,TILES=tiles,CHARS=chars,UI_READS=ui_reads,
        CALLS=calls,LOCAL_DMAS=local_dmas,COLOR_HALF=color_base//2,
        SINGLE_BIND=int(bool(record and record.get('single_binding')))).items():tb=tb.replace('@'+k+'@',str(v))
    if record:
        for old,key in [('40000','command_base'),('100000','color_base'),('140000','depth_base')]:
            tb=tb.replace("32'h"+old,"32'h"+format(record[key],'x'))
    (out/'tb.v').write_text(tb)
    load_contract(a.peer)
    files=[]
    for name in ['bridge.v','combination.v','controller.v','cpu_v3_memory_arbiter.v','pins.v','shared_port.v','ports.vh']:
        shutil.copyfile(a.peer/name,out/name)
        if name.endswith('.v'):files.append(out/name)
    commands=[('compile',[os.environ['IVERILOG_EXE'],'-g2012','-D__ICARUS__','-I',out,'-s','tb','-o',out/'sim.vvp',out/'dut.v',*files,out/'tb.v',Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v'])]
    commands += [(f'run-{mode}',[os.environ['VVP_EXE'],out/'sim.vvp',f'+MODE={mode}']) for mode in a.modes]
    passes=[]
    for name,cmd in commands:
        assert hashlib.sha256((out/'dut.v').read_bytes()).digest()==hashlib.sha256(data).digest()
        with (out/f'{name}.log').open('w') as log:r=subprocess.run(list(map(str,cmd)),cwd=out,stdout=log,stderr=subprocess.STDOUT,timeout=180)
        text=(out/f'{name}.log').read_text(errors='replace')
        if r.returncode:raise RuntimeError(text[-2000:])
        passes += [l for l in text.splitlines() if l.startswith('PASS ')]
    receipt=dict(export_sha256=hashlib.sha256(data).hexdigest(),case=a.case,repeat=a.repeat,ranges=ranges,passes=passes,
        rcc_records_sha256=hashlib.sha256(a.rcc_records.read_bytes()).hexdigest() if a.rcc_records else None,
        single_binding=bool(record and record.get('single_binding')),expected_ranges=calls,expected_local_dmas=local_dmas,
        peer_sha256={f.name:hashlib.sha256(f.read_bytes()).hexdigest() for f in files},
        scope='Actual native GPU + shared MC/pin model; standalone CL2, approximately49.5/99MHz; no CPU/Display or board qualification')
    (out/'receipt.json').write_text(json.dumps(receipt,indent=2))
    print('\n'.join(passes));print('receipt:',out/'receipt.json')
if __name__=='__main__':main()
