"""Fixed-pose diagnostic for a real-hole wide material carrier, not production.

Only level2's existing3x3 ordinary-floor neighborhood is merged. The central
hole has no triangles; it is not represented by RGB pixels. Native floor tint
and G232 use the production photo-derived base material. This deliberately
changes the raised iris into an inlaid material surface and is not Web equivalent.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import struct


def load_assets():
    path=Path(__file__).with_name('assets.py')
    spec=importlib.util.spec_from_file_location('carrier_assets',path)
    module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    return module


def expand(value):
    r,g,b=value>>11,(value>>5)&63,value&31
    return [(r<<3)|(r>>2),(g<<2)|(g>>4),(b<<3)|(b>>2)]


def prepare(output,opening):
    a=load_assets();a.pack(output)
    from gpu_texture_layout import level_offset
    data=json.loads((output/'assets.json').read_text())
    level=data['levels'][1];w=level['width']
    goal=next(i for i,c in enumerate(level['cells']) if c[0]==6);gx,gz=goal%w,goal//w
    assert (gx,gz)==(13,1)
    for dz in [-1,0,1]:
        for dx in [-1,0,1]:
            if dx or dz:assert level['cells'][(gz+dz)*w+gx+dx][0]==1
    phase=(gx*3+gz*5)%4
    binary=(output/'goal_carrier.bin').read_bytes()[phase*32768:(phase+1)*32768]
    assert len(binary)==32768
    pixels=[struct.unpack_from('<H',binary,level_offset(7,x,y))[0] for y in range(128) for x in range(128)]
    (output/'carrier_texture.bin').write_bytes(binary)
    from PIL import Image
    preview=Image.new('RGB',(128,128));preview.putdata([tuple(expand(value)) for value in pixels]);preview.save(output/'carrier-texture.png')
    n=round(opening*128)/128
    corners=[(-3,-3),(3,-3),(3,3),(-3,3)]
    inside=[(-n,-n),(n,-n),(n,n),(-n,n)]
    quads=[]
    for i in range(4):
        j=(i+1)%4;points=[corners[i],corners[j],inside[j],inside[i]]
        quads.append(([(x,0,z) for x,z in points],(0,511,0),(0,0,1,1)))
    mesh=a.W.mesh('carrier-probe',quads,cull_negative=True)
    for vertex in mesh['vertices']:
        xyz=[mesh['config'][axis]+(((vertex[0]>>(axis*10))&1023)<<9) for axis in range(3)]
        uv=[round((2.5+(xyz[axis]/65536+3)/6*123)*65535/128) for axis in [0,2]]
        vertex[2]=uv[0]|(uv[1]<<16)
    u,v,s,t=a.W.uv_rect(0);fraction=n/.995;crop=(1-fraction)/2
    hatch=a.W.mesh('carrier-hatch-probe',[([(-n,0,-n),(n,0,-n),(n,0,n),(-n,0,n)],(0,511,0),(u+crop*s,v+crop*t,s*fraction,t*fraction))],cull_negative=True)
    new_groups=[('CARRIER_PROBE',[mesh]),('CARRIER_HATCH_PROBE',[hatch])]
    meshes=data['meshes'];catalog=data['catalog'];entry=len(meshes)
    for name,group in new_groups:
        catalog.append(dict(name=name,first=entry,count=len(group)));meshes.extend(group);entry+=len(group)
    memory=bytearray((entry*4+127)//128*128)
    resource=next(r for r in data['resources'] if r['file']=='mesh.bin');address=resource['address']
    for index,mesh in enumerate(meshes):
        record=bytearray(a.G.native_record(mesh));q=struct.unpack_from('<Q',record)[0]
        struct.pack_into('<Q',record,0,(q&~(1<<31))|(int(mesh['cull_negative'])<<31))
        sector=(address+len(memory))//128;struct.pack_into('<HH',memory,index*4,sector,len(record)//128-1)
        mesh['record_sector']=sector;mesh['payload_sectors']=len(record)//128-1;memory.extend(record)
    (output/'mesh.bin').write_bytes(memory)
    constants=(output/'renderer_assets.rs').read_text()
    for key,value in [('GPU_RENDERER_MESH_COUNT',entry),('GPU_RENDERER_MESH_SECTORS',len(memory)//128)]:
        constants=re.sub(rf'(pub const {key}:u16=)\d+;',rf'\g<1>{value};',constants)
    for c in catalog[-2:]:constants+=f"pub const {c['name']}_FIRST:u16={c['first']};\npub const {c['name']}_COUNT:u16={c['count']};\n"
    (output/'renderer_assets.rs').write_text(constants)
    data['resources']=[r for r in data['resources'] if r['kind']!='goal_carrier']
    data['resources'].append(dict(kind='carrier-probe',file='carrier_texture.bin',address=0x760000))
    for resource in data['resources']:
        payload=(output/resource['file']).read_bytes();resource.update(bytes=len(payload),sha256=hashlib.sha256(payload).hexdigest())
    extents=sorted((r['address'],r['address']+r['bytes']) for r in data['resources']);assert all(x[1]<=y[0] for x,y in zip(extents,extents[1:]));assert extents[-1][1]<0x7df800
    data['triangles']=sum(len(m['triangles']) for m in meshes)
    data['vertices']=sum(len(m['vertices']) for m in meshes)
    data['carrier_probe']=dict(source_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),level=2,goal=[gx,gz],opening_requested=opening,opening_native=n,real_geometry_hole=True,outer_existing_floor=[3,3],plane_y=0,raised_frame_preserved=False,baked_floor_g=232,baked_floor_lod_q3=0,material='production photo-derived carrier phase; no extra resampling',continuous_animation_implemented=False)
    (output/'assets.json').write_text(json.dumps(data,indent=2)+'\n')


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);p.add_argument('--opening',type=float,required=True);args=p.parse_args();prepare(args.output,args.opening)
