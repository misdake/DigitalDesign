"""实际完整工厂的有界V96→最终帧缓冲独立回放。"""
from pathlib import Path
import argparse, copy, hashlib, importlib.util, json, os, re, shutil, subprocess, sys
sys.dont_write_bytecode=True
HERE=Path(__file__).resolve().parent;ROOT=HERE.parents[3]
SHA=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
TEX=16384;FB=192000;MAX_N=6

def execute(cmd,cwd,log,timeout=240):
    p=subprocess.run(list(map(str,cmd)),cwd=cwd,capture_output=True,timeout=timeout)
    Path(log).write_bytes(p.stdout+p.stderr)
    assert p.returncode==0,(p.stdout+p.stderr).decode(errors='replace')[-7000:]
    return p.stdout.decode(errors='replace')

def pattern(n,x,y):return ((x*3+y*7+n*5)%32<<11)|((y*5+x*11+n*13)%64<<5)|((x*13+y*3+n*17)%32)
def expand(w):return [(w>>11<<3)|(w>>13),(((w>>5)&63)<<2)|((w>>9)&3),((w&31)<<3)|((w>>2)&7)]
def sample(uv,lod,texture=None):
    d=min(lod//8,MAX_N);n=MAX_N-d;f=lod%8 if d<MAX_N else 0;p1=(511*f+4)//8;sums=[0]*3
    fine=[(v*(1<<max(1,n))//256)-128 for v in uv]
    for level,parent in [(n,511-p1),(max(0,n-1),p1)]:
        if not parent:continue
        xy=fine if level==n else [(v-128)//2 if n>1 else v for v in fine]
        frac=[v%256 for v in xy];base=[v//256 for v in xy]
        bot=parent*frac[1]//256;top=parent-bot;tr=top*frac[0]//256;br=bot*frac[0]//256
        for tap,w in enumerate([top-tr,tr,bot-br,br]):
            side=1<<level;x=(base[0]+tap%2)%side;y=(base[1]+tap//2)%side
            texel=pattern(level,x,y) if texture is None else texture[level][y*side+x]
            for c,v in enumerate(expand(texel)):sums[c]+=(w*v+256)//512
    return sums

def initial_memory(texture=None):
    b=bytearray(TEX+2*FB);b[TEX+FB:]=b'\xff'*FB;off=0
    for n in range(MAX_N+1):
        logical=1<<n
        for ty in range((logical+7)//8):
            for tx in range((logical+7)//8):
                for y in range(8):
                    for x in range(8):
                        xx,yy=(tx*8+x)%logical,(ty*8+y)%logical
                        texel=pattern(n,xx,yy) if texture is None else texture[n][yy*logical+xx]
                        b[off:off+2]=texel.to_bytes(2,'little');off+=2
    assert off<=TEX
    return b

def audit(run,packets,O,gold_exe,shading=None,texture=None):
    front=[s.split() for s in (run/'front.txt').read_text().splitlines()]
    ends={int(a[2]):(int(a[3],16),int(a[4],16)) for a in front if a[0]=='E'}
    qs=[];coverage=[set() for _ in packets];np=0;counts=dict(V_T_checked=True,attributes=0,depth_events=0,lod=0,empty=0)
    for a in front:
        typ=a[0]
        if typ=='P':
            assert int(a[2])==np and int(a[3],16)==packets[np]['descriptor'];np+=1;continue
        seq=int(a[2])
        if typ=='A':
            assert seq==len(qs);pid,slot,x,y=map(int,a[3:]);p=packets[pid];qi=y*200+x;mask,h=ends[seq]
            assert p['masks'].get(qi,0)==mask
            e=O.expected_quad(p,qi) if mask else None
            if mask:
                assert qi not in coverage[pid];coverage[pid].add(qi);assert h==e['raw']>>21
                h00=tuple(c+2*nx*x*2+2*ny*y*2 for nx,ny,c in p['edges'])
                math=O.M.quad(p['triangle'],h00,mask)
            else:math={'pixels':{},'passed':0};counts['empty']+=1
            qs.append(dict(packet=pid,qi=qi,mask=mask,expected=e,math=math,w=set(),n=set(),l=False,end=False))
            continue
        q=qs[seq];p=packets[q['packet']];e=q['expected']
        if typ=='E':assert not q['end'];q['end']=True
        elif typ=='W':
            lane,word,got=int(a[3]),int(a[4]),int(a[5],16);assert (lane,word) not in q['w'];q['w'].add((lane,word))
            if q['mask']>>lane&1:
                ix=1+(0,1,3,2).index(lane)*3+word;want=e['words'][ix];cm=e['compare'][ix]
                if word==0:want|=((e['passed']>>lane)&1)<<35
            else:
                n=p['triangle'].normal[2];attr=p['triangle'].rgb[2] if p['rgb'] else p['triangle'].uv[2]
                want=[(n[0]&4095)|((n[1]&4095)<<12),(n[2]&4095)|(attr[0]<<12),(attr[1]|(attr[2]<<8)) if p['rgb'] else attr[1]][word]
                cm=(1<<36)-1 if word!=2 else (1<<16)-1 if p['rgb'] else (1<<18)-1
            assert got&cm==want&cm,('I payload',seq,lane,word,hex(got),hex(want));counts['attributes']+=1
        elif typ=='N':
            lane,z,passed=map(int,a[3:]);assert lane not in q['n'];q['n'].add(lane)
            valid=bool(e and e['passed']>>lane&1);assert bool(passed)==valid
            if valid:
                x=q['qi']%200*2+(lane&1);y=q['qi']//200*2+(lane>>1);assert z==e['depth'][(x,y)]
            counts['depth_events']+=1
        elif typ=='L':
            assert not q['l'];q['l']=True;assert int(a[3])==(e['words'][0]>>19&255 if e else 0);counts['lod']+=1
        else:raise AssertionError(a)
    assert np==len(packets)
    for i,p in enumerate(packets):assert coverage[i]==set(p['masks'])
    for q in qs:assert q['end'] and len(q['w'])==12 and len(q['n'])==4 and q['l']
    # 主仓独立Lighting算术模型，仅输入真实N12与像素中心，不读DUT光照值。
    rows=[];pixels={};cache={}
    for seq,q in enumerate(qs):
        p=packets[q['packet']]
        for lane in range(4):
            v=q['math']['pixels'].get(lane)
            normal=v['normal'] if v else p['triangle'].normal[2]
            uv=v['uv'] if v else p['triangle'].uv[2];rgb=v['rgb'] if v else p['triangle'].rgb[2]
            x=q['qi']%200*2+(lane&1);y=q['qi']//200*2+(lane>>1)
            pixels[seq,lane]=dict(normal=normal,uv=uv,rgb=rgb,x=x,y=y,v=v)
            if q['math']['passed']>>lane&1 and not p.get('unlit'):rows.append(f'{seq*4+lane} '+ ' '.join(map(str,(*normal,x,y)))+'\n')
    (run/'lighting-input.txt').write_text(''.join(rows))
    command=[gold_exe,run/'lighting-input.txt',run/'lighting-gold.txt']
    if shading is not None:
        context=[*shading['light'],shading['ambient'],shading['directional'],
                 *shading['ray_scale'],shading['ray_k'],shading['shininess'],*shading['specular']]
        assert len(context)==12
        (run/'lighting-context.txt').write_text(' '.join(map(str,context))+'\n')
        command.append(run/'lighting-context.txt')
    execute(command,ROOT,run/'lighting.log')
    gs={int(a[0]):tuple(map(int,a[1:])) for l in (run/'lighting-gold.txt').read_text().splitlines() if (a:=l.split())}
    for (seq,lane),v in pixels.items():
        p=packets[qs[seq]['packet']];tex=[255]*3 if p['rgb'] else sample(v['uv'],qs[seq]['expected']['words'][0]>>19&255 if qs[seq]['expected'] else 0,texture)
        g,h=(256,0) if p.get('unlit') else gs.get(seq*4+lane,(0,0));tint=v['rgb'] if p['rgb'] else (255,255,255)
        rgb=[]
        spec=shading['specular'] if shading is not None else (0,0,0)
        for tintc,c,sc in zip(tint,tex,spec):
            s=tintc*c+128;base=(s+(s>>8))>>8;rgb.append(min(255,base*g//256+sc*h//256))
        v.update(tex=tex,final=rgb,g=g,h=h)
    back=[s.split() for s in (run/'back.txt').read_text().splitlines()]
    seen={k:set() for k in ('P','E','L','S','F','R')};commit_records=[]
    for a in back:
        typ=a[0]
        if typ=='C':commit_records.append(tuple([int(a[2]),int(a[3]),int(a[4],16)]));continue
        seq=int(a[2]);q=qs[seq]
        if typ in ('P','E','R'):
            assert seq not in seen[typ];seen[typ].add(seq)
            if typ=='P':assert int(a[3],16)&15==q['math']['passed']
            if typ=='E':assert not q['math']['passed']
            if typ=='R':assert q['math']['passed']
            continue
        lane=int(a[3]);v=pixels[seq,lane];assert q['math']['passed']>>lane&1,('empty/uncovered consumer',typ,seq,lane)
        assert (seq,lane) not in seen[typ];seen[typ].add((seq,lane))
        if typ=='L':assert tuple(map(int,a[4:]))==(v['g'],v['h']),('true normal Lighting',seq,lane,a[4:],v)
        else:
            want=v['tex'] if typ=='S' else v['final'];got=int(a[4],16)
            assert got==sum(c<<(8*i) for i,c in enumerate(want)),(typ,seq,lane,hex(got),want)
    nonempty={i for i,q in enumerate(qs) if q['math']['passed']};live={(i,l) for i in nonempty for l in range(4) if qs[i]['math']['passed']>>l&1}
    assert seen['P']==set(range(len(qs)));assert seen['E']==set(range(len(qs)))-nonempty;assert seen['R']==nonempty
    assert seen['L']=={(i,l) for i,l in live if not packets[qs[i]['packet']].get('unlit')}
    assert seen['F']==live
    assert seen['S']=={(i,l) for i,l in live if not packets[qs[i]['packet']]['rgb']}
    gold=initial_memory(texture);depth_rejected=0;accepted=0;want_commits=[]
    def rnd(v,d):q,r=divmod(v,d);return q+int(2*r>d or 2*r==d and q&1)
    for seq,q in enumerate(qs):
        if not q['math']['passed']:continue
        cm=0
        for lane in range(4):
            if not q['math']['passed']>>lane&1:continue
            v=pixels[seq,lane];x,y=v['x'],v['y'];off=((y//16)*25+x//16)*512+(y%16)*32+(x%16)*2
            z=v['v']['Z16'];old=int.from_bytes(gold[TEX+FB+off:TEX+FB+off+2],'little')
            if z>=old:depth_rejected+=1;continue
            cm|=1<<lane;accepted+=1;r,g,b=v['final'];w=(rnd(r*31,255)<<11)|(rnd(g*63,255)<<5)|rnd(b*31,255)
            gold[TEX+off:TEX+off+2]=w.to_bytes(2,'little');gold[TEX+FB+off:TEX+FB+off+2]=z.to_bytes(2,'little')
        # 完成事件携带输入mask；真正LESS通过数由逐像素FB独立金值核对。
        want_commits.append((q['qi']%200*2,q['qi']//200*2,q['math']['passed']))
    got=bytes(int(s,16) for s in (run/'memory.mem').read_text().splitlines())
    if got!=gold:
        ix=next(i for i,(a,b) in enumerate(zip(got,gold)) if a!=b);raise AssertionError(('whole framebuffer',ix,got[ix],gold[ix]))
    assert commit_records==want_commits,('ROP completion order',len(commit_records),len(want_commits))
    counts.update(quads=len(qs),packets=np,geometry_quads=sum(map(len,coverage)),live_quads=len(nonempty),live_pixels=len(live),ROP_pass=accepted,ROP_depth_reject=depth_rejected,light=len(seen['L']),sample=len(seen['S']),final=len(seen['F']),memory_bytes=len(got),lod_values=sorted({q['expected']['words'][0]>>19&255 for q in qs if q['expected']}))
    if shading is not None:
        counts['specular_nonzero']=sum(pixels[k]['h']>0 for k in live)
        counts['specular_peak']=max((pixels[k]['h'] for k in live),default=0)
    (run/'gold.mem').write_text(''.join(f'{v:02x}\n' for v in gold))
    return counts

def main():
    ap=argparse.ArgumentParser(__doc__);ap.add_argument('--tag',required=True);ap.add_argument('--name',choices=['bunny_patch_1','box_x_minus'],required=True)
    ap.add_argument('--mode',choices=['portable','native','both'],default='both');ap.add_argument('--stress',action='store_true');ap.add_argument('--rgb-repeat',action='store_true');ap.add_argument('--max-cycles',type=int,default=1200000)
    a=ap.parse_args();assert re.fullmatch('[A-Za-z0-9_.-]+',a.tag)
    out=ROOT/'target/gpu-vliw-integration-20261018/stage2'/a.tag;out.mkdir(parents=True,exist_ok=False)
    env=os.environ;env['CARGO_TARGET_DIR']=str(ROOT/'target/gpu-vliw-integration-20261018/cargo-validation')
    cargo=Path.home()/'.cargo/bin/cargo.exe'
    execute([cargo,'build','--release','-p','gpu-v2','--example','vliw_integration'],ROOT,out/'gold-build.log')
    exe=Path(env['CARGO_TARGET_DIR'])/'release/examples/vliw_integration.exe'
    ref=HERE/'reference';spec=importlib.util.spec_from_file_location('stage2_independent_oracle',ref/'scene/oracle.py');O=importlib.util.module_from_spec(spec);spec.loader.exec_module(O)
    ts=importlib.util.spec_from_file_location('current_transport',HERE/'transport.py');T=importlib.util.module_from_spec(ts);ts.loader.exec_module(T);O=T.adapt_oracle(O)
    fixtures=json.loads((ref/'uv-inputs.json').read_text());original=next(x for x in fixtures['scenes'][0]['meshlets'] if x['name']==a.name)
    instances=[copy.deepcopy(original)]
    if a.rgb_repeat:instances.append(copy.deepcopy(original));instances[-1]['color_mode']=1
    packets=[];metas=[]
    for i,v in enumerate(instances):
        v.update(slot=i%2,cfg_tex_w_log2=6,cfg_tex_h_log2=6,cfg_max_mip=6);meta,ps=O.prepare(v,out/f'task{i}');packets+=ps;metas.append(meta)
    (out/'input-contract.json').write_text(json.dumps(dict(instances=instances,note='真实V96/refs/normal不变；I texlog与实际64x64纹理一致重绑定为6；可选第二批仅将同attr32解释为RGB以检查跨T模式与LESS深度拒绝。'),ensure_ascii=False,indent=2))
    # 阶段3的未使用SDRAM/DMA工厂由另一个线程开发，不是本工厂的依赖。
    unrelated={'vliw_sdram.rs','vliw_sdram.v','vliw_memory.rs','vliw_memory.v','vliw_dma.rs','vliw_dma.v','mod.rs'}
    sources={str(p.relative_to(ROOT)).replace('\\','/'):SHA(p) for p in (ROOT/'ip/gpu-v2/src').rglob('*') if p.suffix in ('.rs','.v','.vh') and p.name not in unrelated}
    for p in list(ref.rglob('*'))+[Path(__file__),HERE/'transport.py',HERE/'stage2_tb.v',HERE/'main.rs']:
        if p.is_file() and p.suffix in ('.json','.py','.v','.rs'):sources[str(p.relative_to(ROOT)).replace('\\','/')]=SHA(p)
    (out/'source-manifest.json').write_text(json.dumps(sources,indent=2));reports=[]
    for rel,h in sources.items():
        dst=out/'source'/rel;dst.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(ROOT/rel,dst);assert SHA(dst)==h
    for mode in (['portable','native'] if a.mode=='both' else [a.mode]):
        run=out/mode;run.mkdir()
        for i in range(len(instances)):shutil.copytree(out/f'task{i}',run/f'task{i}')
        execute([cargo,'run','--release','-p','gpu-v2','--bin','gpu-v2-vliw-export','--',run/'dut.v','full']+(['portable'] if mode=='portable' else []),ROOT,run/'export.log')
        for rel,h in sources.items():assert SHA(ROOT/rel)==h,('source changed during export',rel)
        b=initial_memory();(run/'initial.mem').write_text(''.join(f'{v:02x}\n' for v in b))
        tb=(HERE/'stage2_tb.v').read_text();fields=dict(STRESS=int(a.stress),MAXC=a.max_cycles,BYTES=len(b),MAXTIME=a.max_cycles*7+100000,TASKS='\n'.join(f'load_and_start({i},{m["vertices"]},{m["triangles"]},{m["packets"]},{i%2});' for i,m in enumerate(metas)))
        for k,v in fields.items():tb=tb.replace('@'+k+'@',str(v))
        assert not re.search('@[A-Z]+@',tb);(run/'tb.v').write_text(tb)
        cmd=[env['IVERILOG_EXE'],'-g2012','-s','tb','-o',run/'sim.vvp']
        if mode=='native':cmd +=[Path(env['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v']
        execute(cmd+[run/'dut.v',run/'tb.v'],run,run/'compile.log',120)
        log=execute([env['VVP_EXE'],run/'sim.vvp'],run,run/'run.log',600);assert 'PASS FULL' in log
        counts=audit(run,packets,O,exe)
        report=dict(status='pass',mode=mode,stress=a.stress,rgb_repeat=a.rgb_repeat,name=a.name,counts=counts,log=log,source_sha256=SHA(run/'dut.v'),source_manifest_sha256=SHA(out/'source-manifest.json'),front_sha256=SHA(run/'front.txt'),back_sha256=SHA(run/'back.txt'),memory_sha256=SHA(run/'memory.mem'),max_cycles=a.max_cycles)
        (run/'receipt.json').write_text(json.dumps(report,ensure_ascii=False,indent=2));reports.append(report);print(json.dumps(report,ensure_ascii=False),flush=True)
    if len(reports)==2:
        for key in ('counts','front_sha256','back_sha256','memory_sha256'):assert reports[0][key]==reports[1][key],('P/N mismatch',key)
    (out/'receipt.json').write_text(json.dumps(dict(status='pass',runs=reports,boundary='真实V/T/R/I、24容量共享池、Lighting/采样/Final/ROP与完整384000B帧缓冲；有限有序128B内存服务，无物理板证据。'),ensure_ascii=False,indent=2))

if __name__=='__main__':main()
