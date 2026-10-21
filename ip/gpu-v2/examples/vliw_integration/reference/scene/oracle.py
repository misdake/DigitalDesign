"""原始V96→冻结V/T数学→RF30几何→全新I整数gold；不读取被测RTL状态。"""
from pathlib import Path
from collections import Counter
import hashlib,importlib.util,json,sys
HERE=Path(__file__).resolve().parent;ZERO=HERE.parent;ROOT=HERE.parents[3]
def load(name,path):
    spec=importlib.util.spec_from_file_location(name,path);m=importlib.util.module_from_spec(spec);sys.modules[name]=m;spec.loader.exec_module(m);return m
F=load('zero_scene_front',ZERO/'reference.py')
M=load('zero_scene_math',ZERO/'math/model.py')
def pack(v,b):return sum((int(x)&((1<<b)-1))<<(b*i) for i,x in enumerate(v))
def signed(v,b):v=int(v)&((1<<b)-1);return v-(1<<b) if v>>(b-1) else v
def coverage(edges):
    masks={}
    for y in range(240):
        lo,hi=0,399
        for nx,ny,c in edges:
            a,b=2*nx,2*ny;tau=0 if a>0 or a==0 and b>0 else 1;need=tau-c-b*y
            if a>0:lo=max(lo,-((-need)//a))
            elif a<0:hi=min(hi,need//a)
            elif need>0:lo,hi=1,0;break
            if lo>hi:break
        for x in range(lo,hi+1):
            qi=y//2*200+x//2;masks[qi]=masks.get(qi,0)|(1<<((y&1)*2+(x&1)))
    return masks

def prepare(instance,out):
    out=Path(out);out.mkdir(parents=True,exist_ok=True)
    vertices=instance['vertices'];triangles=instance['triangles'];uniform=instance['uniform'];config=instance['config']
    s=int(instance.get('scale_s',0));rgb=instance.get('color_mode',1) not in (0,False,'uv','UV');cull=bool(instance.get('cull_negative',1));normal_mode=bool(config[3]&256)
    slot=int(instance.get('slot',0));base=slot*128;offset=(3*len(vertices)+1)//2
    seq=int(instance.get('seq_base',instance.get('sequence',0)))&127;generation=int(instance.get('generation',0))&15
    metadata=[len(vertices)|(len(triangles)<<8)|(20<<16)|(offset<<24)|(int(cull)<<31),s|(int(rgb)<<4)|(seq<<5)|(generation<<12),
              (config[0]&0xffffffff)|((config[1]&0xffffffff)<<32),(config[2]&0xffffffff)|((config[3]&0xffffffff)<<32)]
    image={320+i:(uniform[2*i]&0xffffffff)|((uniform[2*i+1]&0xffffffff)<<32) for i in range(16)}
    flat=[v for row in vertices for v in row]
    for i in range((len(flat)+1)//2):image[base+i]=(flat[2*i]&0xffffffff)|(((flat[2*i+1] if 2*i+1<len(flat) else 0)&0xffffffff)<<32)
    for group in range((len(triangles)+2)//3):
        image[base+offset+group]=sum(pack(triangles[k],6)<<(18*lane) for lane in range(3) if (k:=group*3+lane)<len(triangles))
    image.update({256+4*slot+i:v for i,v in enumerate(metadata)})
    (out/'dma.txt').write_text(''.join(f'{a} {v:016x}\n' for a,v in sorted(image.items())))
    values=F.oracle(uniform,config,vertices);(out/'vertices.hex').write_text(''.join(f'{v:09x}\n' for v in values))
    draw=dict(far_raw=int(instance.get('cfg_far_z',199*65536)),tex_log=(int(instance.get('cfg_tex_w_log2',8)),int(instance.get('cfg_tex_h_log2',8))),max_mip=int(instance.get('cfg_max_mip',8)),bias_q4=int(instance.get('cfg_lod_bias',0)))
    admissions=[];classes=[];packets=[];stats=Counter()
    for tid,refs in enumerate(triangles):
        desc=pack(refs,6)|(s<<18)|(int(rgb)<<23)|(int(normal_mode)<<24)|(((seq+tid)&127)<<25)|(generation<<32)
        admissions.append(desc)
        H=[tuple(signed(values[6*vi+j],32) for j in range(3)) for vi in refs]
        P=[tuple(v>>s for v in h) for h in H];a,b,c=P
        N=[(b[1]*c[2]-b[2]*c[1],b[2]*c[0]-b[0]*c[2],b[0]*c[1]-b[1]*c[0]),
           (c[1]*a[2]-c[2]*a[1],c[2]*a[0]-c[0]*a[2],c[0]*a[1]-c[1]*a[0]),
           (a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0])]
        delta=sum(x*y for x,y in zip(a,N[0]));cls=3 if all(h[2]<=0 for h in H) else 2 if not delta else 1 if delta<0 and cull else 0
        classes.append(cls);stats[['admitted','backface','degenerate','behind'][cls]]+=1
        words,error,_=F.triangle_oracle(values,refs,s,int(cull));assert not error
        if cls:continue
        assert words is not None and len(words)==19
        sigma=1 if delta>0 else -1;edges=[];exps=[]
        for nn in N:
            C=-399*nn[0]-239*nn[1]+512*nn[2]
            e=max(0,abs(nn[0]).bit_length()-17,abs(nn[1]).bit_length()-17,abs(C).bit_length()-27)
            q=tuple(sigma*((-1 if v<0 else 1)*(abs(v)>>e)) for v in (nn[0],nn[1],C));edges.append(q);exps.append(e)
        normals=[];attrs=[]
        for vi in refs:
            v0,v1=values[6*vi+3:6*vi+5];n=[signed(v0,18),signed(v0>>18,18),signed(v1,18)]
            if not normal_mode:n=[x*4 for x in n]
            normals.append(tuple(M.normal12_from18(x) for x in n));attrs.append(values[6*vi+5]&0xffffffff)
        t=M.Triangle(tuple(row[:2] for row in edges),tuple(exps),abs(delta),s,
                     tuple((v&65535,v>>16) for v in attrs),tuple(normals),tuple(tuple(v>>(8*j)&255 for j in range(3)) for v in attrs),**draw)
        t.validate();masks=coverage(edges)
        packets.append(dict(tid=tid,descriptor=desc|((int(delta<0))<<22),words=tuple(words),triangle=t,edges=tuple(edges),masks=masks,rgb=rgb))
    (out/'admissions.hex').write_text(''.join(f'{v:09x}\n' for v in admissions));(out/'classes.hex').write_text(''.join(f'{v:x}\n' for v in classes))
    (out/'descriptors.hex').write_text(''.join(f'{p["descriptor"]:09x}\n' for p in packets) or '0\n')
    (out/'words.hex').write_text(''.join(f'{v:014x}\n' for p in packets for v in p['words']) or '0\n')
    meta=dict(name=instance['name'],group=instance['group'],vertices=len(vertices),triangles=len(triangles),packets=len(packets),slot=slot,draw=draw,classes=dict(stats),R_quads=sum(len(p['masks']) for p in packets),coverage_pixels=sum(m.bit_count() for p in packets for m in p['masks'].values()),dma_words=len(image))
    (out/'oracle.json').write_text(json.dumps(meta,ensure_ascii=False,indent=2));return meta,packets

def expected_quad(packet,qi):
    t=packet['triangle'];mask=packet['masks'][qi];x=(qi%200)*2;y=(qi//200)*2
    h00=tuple(c+2*nx*x+2*ny*y for nx,ny,c in packet['edges'])
    raw=x|(y<<9)|(mask<<17)|sum((v&((1<<30)-1))<<(21+30*i) for i,v in enumerate(h00))
    q=M.quad(t,h00,mask);lod=0 if packet['rgb'] else q['lod']['LOD_q3']
    header=(x>>1)|((y>>1)<<8)|(q['passed']<<15)|(lod<<19)|(int(packet['rgb'])<<27)
    words=[header];compare=[(1<<36)-1];depth={};classes=Counter()
    for ordinal in range(4):
        lane=ordinal^(ordinal>>1)
        if lane not in q['pixels']:
            words += [0,0,0];compare += [0,0,0];continue
        p=q['pixels'][lane]
        if p['alignment']['zero']:classes['D0']+=1
        elif p['overflow']:classes['overflow']+=1
        elif not p['valid']:classes['far']+=1
        else:
            classes['pass']+=1;depth[(x+(lane&1),y+(lane>>1))]=p['Z16']
            if p['Z32']==0:classes['Z32_zero_pass']+=1
            if p['Z16']==0:classes['Z16_zero_pass']+=1
        n=p['normal'];a=p['rgb'] if packet['rgb'] else p['uv']
        w0=(n[0]&4095)|((n[1]&4095)<<12)
        w1=(n[2]&4095)|(a[0]<<12)
        w2=(a[1]|(a[2]<<8)|(p['Z16']<<16)) if packet['rgb'] else a[1]|(p['Z16']<<18)
        # 所有几何覆盖lane属性均核对；overflow/far/D0的Z payload不属于发布语义。
        zm=((1<<36)-1) if p['valid'] else (((1<<36)-1)^(((1<<16)-1)<<(16 if packet['rgb'] else 18)))
        words += [w0,w1,w2];compare += [(1<<36)-1,(1<<36)-1,zm]
    return dict(raw=raw,words=words,compare=compare,depth=depth,classes=classes,passed=q['passed'])
