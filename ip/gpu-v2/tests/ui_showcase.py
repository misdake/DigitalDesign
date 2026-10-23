"""Two real-MC UI frames: fixed ASCII, colored pixel icons and split CJK glyphs.

The font zip is external input; no font binary is vendored. CJK occupies two
8x16 cells. Source glyph pixels also drive an independent screen-space oracle.
"""
from pathlib import Path
from zipfile import ZipFile
import argparse,hashlib,importlib.util,json,os,re,shutil,struct,subprocess,sys
from PIL import Image,ImageDraw,ImageFont
from fontTools.ttLib import TTFont
ROOT=Path(__file__).resolve().parents[3]
sys.dont_write_bytecode=True
sys.path.insert(0,str(ROOT/'ip/gpu-v2/examples/vliw_integration'))
from replay_command import CommandBuffer,target_params
from device_bus import TASKS
sys.path.insert(0,str(ROOT/'ip/gpu-v2/examples/postquad_ii4'))
from wrapper import wrapper

def rgb565(p):return ((p>>11)<<11)|(((p>>6)&31)<<6)|(((p>>10)&1)<<5)|((p>>1)&31)
def rgba5551(rgb):return ((rgb[0]>>3)<<11)|((rgb[1]>>3)<<6)|((rgb[2]>>3)<<1)|1
def offset(x,y):return ((y//16)*25+x//16)*256+(y%16)*16+x%16

def native_ascii(path):
    """Read the Windows Fixedsys raster strike directly, without FreeType mapping.

    Its native 8x15 pixels occupy the first 15 rows of an 8x16 cell. The FON
    resource is an external test asset, never copied into repository sources.
    """
    data=path.read_bytes();ne=struct.unpack_from('<I',data,0x3c)[0]
    p=ne+struct.unpack_from('<H',data,ne+0x24)[0]
    shift=struct.unpack_from('<H',data,p)[0];p+=2
    while True:
        kind,count=struct.unpack_from('<HH',data,p)
        if kind==0:raise ValueError('No native 8x15 raster font')
        p+=8
        for _ in range(count):
            start,size=struct.unpack_from('<HH',data,p);p+=12
            if kind!=0x8008:continue
            f=data[start<<shift:(start+size)<<shift]
            if struct.unpack_from('<H',f)[0]!=0x200 or struct.unpack_from('<HH',f,86)!=(8,15):continue
            masks={}
            for code in range(32,127):
                width,base=struct.unpack_from('<HH',f,118+(code-f[95])*4)
                assert width==8
                masks[chr(code)]=[bool(f[base+y]&(128>>x)) if y<15 else False for y in range(16) for x in range(8)]
            return masks,hashlib.sha256(data).hexdigest()

def native_cjk(font,char):
    """Sample the original 100-font-unit bitmap cells, not a scaled TTF render."""
    g=font['glyf'][font.getBestCmap()[ord(char)]]
    assert not g.isComposite() and set(g.flags)=={1}
    points=list(g.coordinates)
    assert all(x%100==0 and y%100==0 and 0<=x<=1600 and -200<=y<=1400 for x,y in points)
    edges=[];first=0
    for end in g.endPtsOfContours:
        contour=points[first:end+1];first=end+1
        for a,b in zip(contour,contour[1:]+contour[:1]):
            assert a[0]==b[0] or a[1]==b[1],'Expected bitmap outline'
            edges.append((a,b))
    mask=[]
    for row in range(16):
        for col in range(16):
            x=col*100+50;y=1400-row*100-50;winding=0
            for (ax,ay),(bx,by) in edges:
                if ax==bx and ax>x:
                    if ay<=y<by:winding+=1
                    if by<=y<ay:winding-=1
            mask.append(winding!=0)
    return mask

def assets(out,font_zip):
    with ZipFile(font_zip) as z:
        name=next(n for n in z.namelist() if n.endswith('16px.ttf'))
        font_data=z.read(name);(out/'wqy16.ttf').write_bytes(font_data)
    cjk=TTFont(out/'wqy16.ttf')
    ascii_masks,ascii_hash=native_ascii(Path(os.environ['WINDIR'])/'Fonts/vgafix.fon')
    glyphs=[];lookup={};labels=[];grids=[];cjk_chars=set()
    white=(232,239,248);cyan=(72,216,240);yellow=(255,210,72);muted=(152,180,208)
    def add(image,label):
        pixels=list(image.convert('RGBA').getdata())
        assert all(p[3] in (0,255) for p in pixels),'Source pixels must already have binary alpha'
        words=tuple(rgba5551(p) if p[3]==255 else 0 for p in pixels)
        if not any(words):return 255
        if words not in lookup:
            assert len(glyphs)<255,'Atlas exhausted'
            lookup[words]=len(glyphs);glyphs.append(words);labels.append(label)
        return lookup[words]
    for frame in range(2):
        grid=bytearray([255]*750)
        def text(x,y,value,color):
            for c in value:
                wide=ord(c)>127;im=Image.new('RGBA',(16 if wide else 8,16))
                if c!=' ':
                    if wide:cjk_chars.add(c)
                    mask=native_cjk(cjk,c) if wide else ascii_masks[c]
                    im.putdata([(*color,255) if on else (0,0,0,0) for on in mask])
                for half in range(2 if wide else 1):
                    assert 0<=x<50 and 0<=y<15
                    grid[y*50+x]=add(im.crop((half*8,0,half*8+8,16)),c+f':{half}')
                    x+=1
        text(2,1,f'GPU UI / FRAME {frame+1}',white)
        text(2+2*frame,3+frame,'中文界面测试' if frame==0 else '第二帧更新成功',cyan)
        text(2,6,'TEXT + ICONS + CHINESE',white)
        text(2,8,'STATUS: READY' if frame==0 else 'STATUS: RUNNING',yellow)
        text(2,10,'NATIVE BITMAP / NO SCALE',muted)
        text(2,13,'8x16 RGBA5551 / ALPHA 0|1',muted)
        for i in range(8):
            im=Image.new('RGBA',(8,16));d=ImageDraw.Draw(im);color=(*[(72,220,128),(255,194,64),(88,168,255),(240,112,128)][i%4],255)
            if i==0:d.polygon([(1,3),(1,12),(6,8)],fill=color)
            elif i==1:d.rectangle((1,4,2,12),fill=color);d.rectangle((5,4,6,12),fill=color)
            elif i==2:d.rectangle((0,5,6,11),outline=color);d.rectangle((7,7,7,9),fill=color);d.rectangle((2,7,2+frame*2,9),fill=color)
            elif i==3:d.ellipse((0,4,7,11),outline=color);d.point((3,7),fill=color)
            elif i==4:d.line([(0,9),(2,12),(7,4)],fill=color,width=2)
            elif i==5:d.line((0,4,7,11),fill=color,width=2);d.line((0,11,7,4),fill=color,width=2)
            elif i==6:
                for j in range(4):d.rectangle((j*2,12-j*2-frame,j*2,13),fill=color)
            else:d.polygon([(3,2),(7,9),(5,13),(1,13),(0,9)],outline=color)
            grid[8*50+30+i*2]=add(im,f'icon{i}/frame{frame}')
        grids.append(grid)
    # Independent monochrome rasterizer at the native EM scale checks the
    # outline sampler. This reference is not used to produce atlas pixels.
    reference_font=ImageFont.truetype(str(out/'wqy16.ttf'),18)
    for char in cjk_chars:
        reference=Image.new('1',(16,16));draw=ImageDraw.Draw(reference);draw.fontmode='1'
        draw.text((0,0),char,font=reference_font,fill=1)
        assert native_cjk(cjk,char)==[bool(p) for p in reference.getdata()]
    atlas=bytearray(65536)
    for i,words in enumerate(glyphs):
        native=[words[y*8+(x^((y&1)*2))] for y in range(16) for x in range(8)]
        atlas[i*256:(i+1)*256]=struct.pack('<128H',*native)
    seed={};expected=[];commands=[];counts=[]
    def put(a,data):
        assert a%4==0 and len(data)%4==0
        seed.update((a+i,int.from_bytes(data[i:i+4],'little')) for i in range(0,len(data),4))
    put(0x60000,atlas)
    for frame,grid in enumerate(grids):
        color=0x100000+frame*0x80000;depth=color+0x40000
        bg=[0]*96000;want=[0]*96000
        for y in range(240):
            for x in range(400):
                rgb=(12+frame*4,20,32+frame*4)
                if 16<=y<48:rgb=(20,36,56)
                if y>=208:rgb=(16,28,44)
                if x%8==0 or y%16==0:rgb=tuple(min(c+2,255) for c in rgb)
                old=rgb565(rgba5551(rgb));idx=offset(x,y);bg[idx]=old
                c=grid[(y//16)*50+x//8];p=glyphs[c][(y%16)*8+x%8] if c!=255 else 0
                want[idx]=rgb565(p) if p&1 else old
        expected.append(want)
        put(color,struct.pack('<96000H',*bg));put(depth,struct.pack('<96000H',*([0xa55a]*96000)))
        grid_base=0x50000+frame*1024;desc=0x51000+frame*64
        put(grid_base,grid+bytes([255]*18));put(desc,target_params(color,depth));put(desc+32,struct.pack('<Q',(grid_base//128)|(6<<16))+bytes(24))
        cb=CommandBuffer();cb.emit('WAIT',a=3);cb.emit('FLUSH');cb.emit('DMA',272,desc,32);cb.emit('WAIT',a=1);cb.emit('SET_TARGET',272);cb.emit('FLUSH')
        cb.emit('DMA',276,desc+32,32);cb.emit('WAIT',a=1);cb.emit('UI_GRID',276);cb.emit('FLUSH');cb.emit('END')
        code=cb.encode();put(0x40000+frame*256,code+bytes((-len(code))%128));commands.append(len(code))
        counts.append(dict(chars=sum(c!=255 for c in grid),tiles=sum(grid[i:i+2]!=b'\xff\xff' for i in range(0,750,2))))
        (out/f'expected{frame}.hex').write_text(''.join(f'{want[i]|want[i+1]<<16:08x}\n' for i in range(0,96000,2)))
        (out/f'commands{frame}.txt').write_text(cb.text())
    (out/'seed.txt').write_text(''.join(f'{a:x} {v:08x}\n' for a,v in sorted(seed.items())))
    (out/'atlas-labels.json').write_text(json.dumps(labels,ensure_ascii=False,indent=2),encoding='utf8')
    (out/'font-production.json').write_text(json.dumps(dict(cjk_grid_units=100,cjk_baseline_units=1400,cjk_size=[16,16],cjk_reference_glyphs=len(cjk_chars),cjk_reference_pixel_differences=0,ascii_size=[8,15],ascii_cell=[8,16],ascii_sha256=ascii_hash,antialias=False,resize=False),indent=2))
    return commands,counts,hashlib.sha256(font_data).hexdigest()

def main():
    p=argparse.ArgumentParser(__doc__);p.add_argument('--export',required=True,type=Path);p.add_argument('--peer',required=True,type=Path);p.add_argument('--font-zip',required=True,type=Path);p.add_argument('--output',required=True,type=Path);a=p.parse_args()
    out=a.output.resolve();out.mkdir(parents=True,exist_ok=False);source=a.export.read_bytes();(out/'dut.v').write_bytes(source)
    sizes,counts,font_hash=assets(out,a.font_zip)
    _,ports=wrapper(re.sub(r'\b(input|output)(?=\[)',r'\1 ',source.decode()));decl=[]
    for v in ports['ports']:
        driven=v['direction']=='input' and not v['name'].startswith(('ro_','fb_r_','fb_w_'))
        decl.append(('reg ' if driven else 'wire ')+(f'[{v["bits"]-1}:0] ' if v['bits']>1 else '')+v['name']+('=0;' if driven else ';'))
    spec=importlib.util.spec_from_file_location('peer_test',Path(__file__).with_name('command_continuation.py'));m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
    tb=m.TB[:m.TB.index('integer cycle=0')].replace('@PORTS@','\n'.join(decl)).replace('gpu_v2_vliw_command dut(.*);',TASKS+'\ngpu_v2_vliw_command dut(.*);')
    tb+=r'''
integer cycle=0,fd,rc,a,v,j,k,frame,begin_cycle,ui_cycles=0,wr=0,rd=0,ro=0;
reg[31:0]want0[0:47999],want1[0:47999];
always @(negedge clk)if(!reset)ce=cycle%11!=4;
always @(posedge clk)begin
 cycle=cycle+1;if(cycle>160000)$fatal(1,"two-frame UI watchdog");
 if(dut.ui_active)ui_cycles=ui_cycles+1;
 if(fb_w_request_valid&&fb_w_request_ready)wr=wr+1;
 if(fb_r_request_valid&&fb_r_request_ready)rd=rd+1;
 if(ro_request_valid&&ro_request_ready)ro=ro+1;
end
initial begin
 reset=1;ce=1;output_ready=1;
 fd=$fopen("seed.txt","r");while(!$feof(fd))begin rc=$fscanf(fd,"%h %h\n",a,v);if(rc==2)begin pin.mem[native_word(a)]=v;pin.marked[native_word(a)]=1;end end $fclose(fd);
 $readmemh("expected0.hex",want0);$readmemh("expected1.hex",want1);
 repeat(4)@(negedge clk);reset=0;
 for(frame=0;frame<2;frame=frame+1)begin
  begin_cycle=cycle;ui_cycles=0;wr=0;rd=0;ro=0;
  submit_commands(32'h40000+frame*256,@BYTES@);
  while(busy)@(negedge clk);
  for(k=0;k<=frame;k=k+1)begin
   for(j=0;j<48000;j=j+1)begin
    if(pin.mem[native_word(32'h100000+k*32'h80000+j*4)]!==(k==0?want0[j]:want1[j]))$fatal(1,"UI frame%0d retained%0d color word%0d",frame,k,j);
    if(pin.mem[native_word(32'h140000+k*32'h80000+j*4)]!==32'ha55aa55a)$fatal(1,"UI depth changed");
   end
  end
  if(device_read_data[1])$fatal(1,"UI transport error");
  fd=$fopen(frame==0?"actual0.hex":"actual1.hex","w");
  for(j=0;j<48000;j=j+1)$fdisplay(fd,"%08x",pin.mem[native_word(32'h100000+frame*32'h80000+j*4)]);$fclose(fd);
  $display("PASS UI_FRAME frame=%0d total=%0d ui=%0d ro=%0d rd=%0d wr=%0d",frame,cycle-begin_cycle,ui_cycles,ro,rd,wr);
 end
 $finish;
end
endmodule
'''.replace('@BYTES@',str(sizes[0]));assert sizes[0]==sizes[1]
    (out/'tb.v').write_text(tb);files=[]
    for name in ['bridge.v','combination.v','controller.v','cpu_v3_memory_arbiter.v','pins.v','shared_port.v','ports.vh']:
        shutil.copyfile(a.peer/name,out/name)
        if name.endswith('.v'):files.append(out/name)
    cmds=[('compile',[os.environ['IVERILOG_EXE'],'-g2012','-D__ICARUS__','-I',out,'-s','tb','-o',out/'sim.vvp',out/'dut.v',*files,out/'tb.v',Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v']),('run',[os.environ['VVP_EXE'],out/'sim.vvp'])]
    for name,cmd in cmds:
        assert hashlib.sha256((out/'dut.v').read_bytes()).digest()==hashlib.sha256(source).digest()
        with (out/f'{name}.log').open('w') as log:r=subprocess.run(list(map(str,cmd)),cwd=out,stdout=log,stderr=subprocess.STDOUT,timeout=240)
        log=(out/f'{name}.log').read_text(errors='replace')
        if r.returncode:raise RuntimeError(log[-2000:])
    passes=[s for s in log.splitlines() if s.startswith('PASS ')]
    for frame in range(2):
        actual=[int(s,16) for s in (out/f'actual{frame}.hex').read_text().split()];im=Image.new('RGB',(400,240))
        for y in range(240):
            for x in range(400):
                i=offset(x,y);v=(actual[i//2]>>((i&1)*16))&65535
                im.putpixel((x,y),(((v>>11)&31)*255//31,((v>>5)&63)*255//63,(v&31)*255//31))
        im.save(out/f'frame{frame+1}.png');im.resize((800,480),Image.Resampling.NEAREST).save(out/f'frame{frame+1}-2x.png')
    receipt=dict(status='pass',passes=passes,counts=counts,export_sha256=hashlib.sha256(source).hexdigest(),font_sha256=font_hash,peer_sha256={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in files},scope='Two sequential real GPU+MC command buffers, no reset between frames; independent whole-plane reference; standalone CL2, no CPU/Display/board claim')
    (out/'receipt.json').write_text(json.dumps(receipt,indent=2));print(json.dumps(receipt))
if __name__=='__main__':main()
