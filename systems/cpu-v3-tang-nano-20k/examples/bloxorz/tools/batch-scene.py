"""Example-only static level batching; native encoding is owned by the GPU oracle."""
from pathlib import Path
from collections import defaultdict
import importlib.util
import json
import struct
import sys
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("blox_static", Path(__file__).with_name("static-scene.py"))
S = importlib.util.module_from_spec(spec)
spec.loader.exec_module(S)
O = S.O
BASE = 0x4B0000
STEP = 512


def levels():
    result = {}
    for line in (Path(__file__).parents[1] / "assets/reference.txt").read_text().splitlines():
        if line.startswith("level "):
            words = line.split()
            data = list(map(int, words[3:]))
            result[words[1]] = dict(name=words[1], width=data[0], depth=data[1], start=data[2:5], mask=data[5],
                                  cells=[data[8+i*8:16+i*8] for i in range(data[6])])
    return result


def group_key(cell, x, z):
    kind = cell[0]
    if kind == 1:
        return "ground"
    if kind == 5:
        return f"bridge-{cell[1]}"
    return f"{['void','ground','fragile','soft','heavy','bridge','goal-cover','splitter'][kind]}-{x}-{z}"


def enabled(cell, mask):
    return cell[0] != 6 and (cell[0] != 5 or bool(mask & cell[1]))


def canonical(level, cells):
    vertices, triangles = [], []
    colors = [(110,130,155), (48,63,82), (75,94,119)]
    for x,z in cells:
        center = [(2*x-(level['width']-1))*65536, 0, (2*z-(level['depth']-1))*65536]
        for face,(axis,sign) in enumerate([(1,1),(0,1),(2,1)]):
            first = len(vertices)
            for q in [(-1,-1),(1,-1),(1,1),(-1,1)]:
                signs=[0,0,0]; signs[axis]=sign
                signs[(axis+1)%3],signs[(axis+2)%3]=q
                xyz=tuple(center[i]+signs[i]*[65536,13056,65536][i] for i in range(3))
                normal=[0,0,0];normal[axis]=511
                vertices.append((xyz,O.pack(normal,10),O.pack(colors[face],8)))
            triangles += [[first,first+1,first+2],[first,first+2,first+3]]
    return vertices,triangles


def meshlet(level, cells, name):
    vertices,triangles=canonical(level,cells)
    origins=[min(v[0][i] for v in vertices) for i in range(3)]
    assert [v%STEP for v in origins]==[0,256,0]
    encoded=[]
    for xyz,normal,color in vertices:
        delta=[(xyz[i]-origins[i])//STEP for i in range(3)]
        assert all((xyz[i]-origins[i])%STEP==0 and 0<=delta[i]<=1023 for i in range(3))
        assert tuple(origins[i]+delta[i]*STEP for i in range(3))==xyz
        encoded.append([O.pack(delta,10),normal,color])
    assert len(encoded)<=64 and len(triangles)<=96
    return dict(name=name,group=name,vertices=encoded,triangles=triangles,config=origins+[9|256],
                scale_s=5,color_mode=1,cull_negative=False,cfg_far_z=100*65536,
                cfg_tex_w_log2=0,cfg_tex_h_log2=0,cfg_max_mip=0,cells=cells)


def grouped(level):
    cells=defaultdict(list)
    for n,cell in enumerate(level['cells']):
        if cell[0]:
            x,z=n%level['width'],n//level['width']
            cells[group_key(cell,x,z)].append((x,z))
    groups=[]
    for name,coords in cells.items():
        buckets=defaultdict(list)
        for x,z in coords:buckets[(x//3,z//3)].append((x,z))
        meshes=[]
        for bucket in buckets.values():
            for i in range(0,len(bucket),5):meshes.append(meshlet(level,bucket[i:i+5],name))
        x,z=coords[0];cell=level['cells'][z*level['width']+x]
        groups.append(dict(name=name,meshes=meshes,enabled=enabled(cell,level['mask']),kind=cell[0],cells=coords))
    return groups


def validate(level):
    groups=grouped(level)
    seen={}; duplicates=0;cross_duplicates=0;covered=[];owner=0
    for g in groups:
        assert g['name']=='ground' or g['kind']!=1
        for m in g['meshes']:
            owner+=1
            covered += m['cells']
            expected,_=canonical(level,m['cells'])
            for record,(xyz,_,_) in zip(m['vertices'],expected):
                decoded=tuple(m['config'][i]+((record[0]>>(10*i))&1023)*STEP for i in range(3))
                assert decoded==xyz
                if xyz in seen:
                    assert seen[xyz][0]==decoded
                    duplicates+=1
                    cross_duplicates+=seen[xyz][1]!=owner
                seen[xyz]=(decoded,owner)
    assert sorted(covered)==sorted((i%level['width'],i//level['width']) for i,c in enumerate(level['cells']) if c[0])
    return dict(level=level['name'],width=level['width'],depth=level['depth'],groups=len(groups),
                meshlets=sum(len(g['meshes']) for g in groups),duplicate_endpoints=duplicates,cross_meshlet_endpoints=cross_duplicates,exact_grid=True)


def assets(out, level, mode):
    out.mkdir(parents=True,exist_ok=True);S.OUT=out
    validation=validate(level)
    if mode=='batch':
        groups=grouped(level)
        draws=[dict(group=i,kind=0,x=0,z=0) for i,g in enumerate(groups) if g['enabled']]
    else:
        floor=S.box('floor',[1,.2,1],[(1,1),(0,1),(2,1)],8)
        groups=[dict(name='local-floor',meshes=[floor])]
        draws=[dict(group=0,kind=2,x=i%level['width'],z=i//level['width']) for i,c in enumerate(level['cells']) if c[0] and enabled(c,level['mask'])]
    block=S.box('block',[.98]*3,[(1,1),(0,1),(2,1),(1,-1),(0,-1),(2,-1)],7)
    draws.append(dict(group=len(groups),kind=1,x=0,z=0))
    groups.append(dict(name='block',meshes=[block]))
    count=sum(len(g['meshes']) for g in groups)
    directory_bytes=(count*4+127)//128*128
    memory=bytearray(directory_bytes);entry=0;uniform=[0]*32
    for i in [0,5,10,12,16,20,21]:uniform[i]=65536
    uniform[11]=40*65536;uniform[22]=0xFFFFFFFF
    meshes=[]
    for g in groups:
        g['directory_sector']=(BASE+entry*4)//128
        g['directory_word']=(entry*4%128)//2
        g['count']=len(g['meshes']);indices=[]
        for mesh in g.pop('meshes'):
            index=len(meshes);indices.append(index);meshes.append(mesh)
            record,padded,_,_=S.image_record(mesh,out/f'asset-{index}',uniform)
            struct.pack_into('<HH',memory,entry*4,(BASE+len(memory))//128,padded//128)
            memory.extend(record);entry+=1
        g['meshlets']=indices
    texture_sector=(BASE+len(memory))//128
    memory.extend(b'\xff\xff'+bytes(126))
    assert len(memory)<=0x350000
    manifest=dict(level=level,mode=mode,groups=groups,draws=draws,meshes=meshes,asset_sectors=len(memory)//128,
                  texture_sector=texture_sector,validation=validation,native_draws=sum(groups[d['group']]['count'] for d in draws))
    words=[level['width'],level['depth'],*level['start'],len(draws),texture_sector,len(memory)//128]
    for d in draws:
        g=groups[d['group']]
        words += [g['directory_sector'],g['directory_word'],g['count'],d['group'],d['kind'],d['x'],d['z'],0]
    (out/'manifest.json').write_text(json.dumps(manifest,indent=2))
    (out/'params.bin').write_bytes(struct.pack('<'+'H'*len(words),*words))
    (out/'assets.bin').write_bytes(memory)
    (out/'assets.hex').write_text(f'@{BASE//8:x}\n'+''.join(f'{int.from_bytes(memory[i:i+8],"little"):016x}\n' for i in range(0,len(memory),8)))
    print(json.dumps(dict(level=level['name'],mode=mode,draw_calls=len(draws),native_draws=manifest['native_draws'],asset_bytes=len(memory))))


def checked_vertices(mesh, uniform):
    # Check before the oracle's ABI wraps/truncates; wrapped values alone cannot prove range.
    for packed, normal, _ in mesh['vertices']:
        xyz=[mesh['config'][k]+((packed>>(10*k))&1023)*(1<<(mesh['config'][3]&31)) for k in range(3)]
        n=[O.signed(normal>>(10*k),10)*128 for k in range(3)]
        for j in range(3):
            raw=(sum(xyz[k]*O.signed(uniform[4*j+k],32) for k in range(3))+65536*O.signed(uniform[4*j+3],32))>>16
            assert -(1<<31)<=raw<(1<<31) and abs(raw>>mesh['scale_s'])<=131071
            raw_normal=sum(n[k]*O.signed(uniform[12+3*j+k],32) for k in range(3))>>16
            assert -131072<=raw_normal<=131071
    return O.F.oracle(uniform,mesh['config'],mesh['vertices'])


def compare(out):
    def endpoints(folder):
        manifest=json.loads((folder/'manifest.json').read_text());points={}
        for draw,line in zip(manifest['draws'],(folder/'matrices.txt').read_text().splitlines()):
            words=[int(x,16) for x in line.split()]
            uniform=[words[1+2*j]|words[2+2*j]<<16 for j in range(21)]+[65536,0xffffffff]+[0]*9
            if draw['kind']==1:continue
            for mid in manifest['groups'][draw['group']]['meshlets']:
                mesh=manifest['meshes'][mid];values=checked_vertices(mesh,uniform)
                for vi,(packed,_,_) in enumerate(mesh['vertices']):
                    world=[mesh['config'][k]+((packed>>(10*k))&1023)*(1<<(mesh['config'][3]&31)) for k in range(3)]
                    if draw['kind']==2:
                        world[0]+=(2*draw['x']-(manifest['level']['width']-1))*65536
                        world[2]+=(2*draw['z']-(manifest['level']['depth']-1))*65536
                    h=[O.signed(values[vi*6+j],32)>>5 for j in range(3)]
                    assert h[2]>0
                    pixel=(199.5+256*h[0]/h[2],119.5+256*h[1]/h[2])
                    points.setdefault(tuple(world),[]).append(pixel)
        return points
    tiled=out.with_name(out.name.replace('-batch','-tiled'))
    a,b=endpoints(tiled),endpoints(out)
    assert a.keys()==b.keys()
    error=max(max(abs(pa[i]-pb[i]) for i in range(2)) for key in a for pa in a[key] for pb in b[key])
    assert error<0.1
    for pairs in b.values():assert all(p==pairs[0] for p in pairs), 'shared-instance endpoints differ'
    ca=[int(x,16) for x in (tiled/'gold1.hex').read_text().split()]
    cb=[int(x,16) for x in (out/'gold1.hex').read_text().split()]
    differences=[i for i,(x,y) in enumerate(zip(ca,cb)) if x!=y]
    def near_edge(image,p):
        x,y=p%400,p//400
        return any(image[yy*400+xx]!=image[p] for yy in range(max(0,y-1),min(240,y+2)) for xx in range(max(0,x-1),min(400,x+2)))
    assert all(near_edge(ca,p) or near_edge(cb,p) for p in differences), 'interior coverage/color disagreement'
    report=dict(shared_endpoints=len(a),max_projected_endpoint_delta=error,color_differences=len(differences),
                coverage_differences=sum(bool(x)!=bool(y) for x,y in zip(ca,cb)),differences_near_edges_only=True)
    (out/'batch-equivalence.json').write_text(json.dumps(report,indent=2));print(json.dumps(report))


def golden(out):
    S.OUT=out
    manifest=json.loads((out/'manifest.json').read_text())
    color,depth=[0]*96000,[65535]*96000
    lines=(out/'matrices.txt').read_text().splitlines()
    assert len(lines)==len(manifest['draws'])
    max_position=0
    for i,line in enumerate(lines):
        words=[int(x,16) for x in line.split()];group=words[0]
        assert group==manifest['draws'][i]['group']
        uniform=[words[1+2*j]|words[2+2*j]<<16 for j in range(21)]+[65536,0xffffffff]+[0]*9
        for mid in manifest['groups'][group]['meshlets']:
            mesh=manifest['meshes'][mid]
            values=checked_vertices(mesh,uniform)
            for vi in range(len(mesh['vertices'])):
                for axis in range(3):
                    v=O.signed(values[vi*6+axis],32)>>mesh['scale_s']
                    assert -131072<=v<=131071
                    max_position=max(max_position,abs(v))
            _,_,packets,_=S.image_record(mesh,out/f'draw-{i}-{mid}',uniform)
            for packet in packets:
                for qi,mask in packet['masks'].items():
                    x,y=qi%200*2,qi//200*2
                    h=tuple(c+2*nx*x+2*ny*y for nx,ny,c in packet['edges'])
                    quad=O.M.quad(packet['triangle'],h,mask)
                    for lane,pixel in quad['pixels'].items():
                        if not pixel['valid']:continue
                        p=(y+(lane>>1))*400+x+(lane&1)
                        if pixel['Z16']<depth[p]:
                            r,g,b=pixel['rgb'];color[p]=((r*31+127)//255<<11)|((g*63+127)//255<<5)|((b*31+127)//255)
                            depth[p]=pixel['Z16']
    for name,values in [('gold',color),('depth',depth)]:
        for frame in [1,2]:(out/f'{name}{frame}.hex').write_text(''.join(f'{v:04x}\n' for v in values))
    report=dict(max_shifted_position=max_position,covered_pixels=sum(x!=0 for x in color))
    (out/'reference-report.json').write_text(json.dumps(report,indent=2));print(json.dumps(report))


if __name__=='__main__':
    mode=sys.argv[1];out=Path(sys.argv[2]).resolve()
    all_levels=levels()
    if mode=='validate':
        out.mkdir(parents=True,exist_ok=True)
        report=[validate(level) for level in all_levels.values()]
        (out/'all-levels.json').write_text(json.dumps(report,indent=2));print(f'EXACT_GRID_PASS levels={len(report)}')
    elif mode=='assets':
        name=sys.argv[3]
        if name=='largest':name=max(all_levels,key=lambda n:(len(all_levels[n]['cells']),sum(c[0]!=0 for c in all_levels[n]['cells'])))
        assets(out,all_levels[name],sys.argv[4])
    elif mode=='range':
        maximum=0;normal_maximum=0;count=0
        for line in (out/'range-matrices.txt').read_text().splitlines():
            name,*parts=line.split();words=[int(w,16) for w in parts]
            uniform=[words[2*i]|words[2*i+1]<<16 for i in range(21)]+[65536,0xffffffff]+[0]*9
            for group in grouped(all_levels[name]):
                for mesh in group['meshes']:
                    values=checked_vertices(mesh,uniform)
                    for vi in range(len(mesh['vertices'])):
                        for axis in range(3):
                            value=O.signed(values[vi*6+axis],32)>>5
                            assert -131072<=value<=131071
                            maximum=max(maximum,abs(value))
                        n0,n1=values[vi*6+3:vi*6+5]
                        normal_maximum=max(normal_maximum,*(abs(O.signed(v,18)) for v in [n0,n0>>18,n1]))
                    count+=len(mesh['vertices'])
            validate(all_levels[name])
        report=dict(levels=len(all_levels),vertices=count,max_shifted_position=maximum,max_normal18=normal_maximum,exact_grid=True)
        (out/'range-report.json').write_text(json.dumps(report,indent=2));print(json.dumps(report))
    elif mode=='golden':golden(out)
    elif mode=='compare':compare(out)
