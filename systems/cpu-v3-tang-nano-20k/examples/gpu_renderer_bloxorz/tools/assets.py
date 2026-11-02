"""Bloxorz column-layout scene and fullscreen RTT mesh, using the existing Bloxorz geometry without rendering."""
import argparse
import hashlib
import importlib.util
import json
import re
from pathlib import Path
import struct
import sys
sys.dont_write_bytecode = True
SYSTEM = Path(__file__).resolve().parents[3]
def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result
B = module("blox_batch", SYSTEM / "examples/bloxorz/tools/batch-scene.py")
R = module("minimal_renderer", SYSTEM / "examples/gpu_renderer/tools/assets.py")
G = module("native_assets", SYSTEM / "examples/gpu_demo/tools/assets.py")
W = module("blox_web_scene", Path(__file__).with_name("web_scene.py"))

def pack(output):
    R.pack(output)
    manifest=json.loads((output/'assets.json').read_text())
    BASE=next(r['address'] for r in manifest['resources'] if r['kind']=='mesh')
    texture=0x680000
    for resource in manifest['resources']:
        if resource['kind']=='texture':
            resource['address']=texture
            (output/resource['file']).write_bytes(W.texture(SYSTEM))
            resource['bytes']=(output/resource['file']).stat().st_size
            R.validate_asset_range(texture,resource['bytes'])
            assert texture+resource['bytes'] < 0x7df800
    background = Path(__file__).resolve().parents[1] / 'assets/background-rgb565-column16.bin'
    data = background.read_bytes()
    assert len(data) == 204800
    assert hashlib.sha256(data).hexdigest() == '5f4be20a8bf7708d54ff2bde923210964c147eeb53c43c0b2e6ded71f9dc8047'
    (output/'background.bin').write_bytes(data)
    manifest['resources'].append(dict(kind='background',file='background.bin',address=0x580000,bytes=len(data)))
    levels = B.levels()
    chosen = [levels[n] for n in ["b1", "b2"]]
    # The two-word undo timeline represents this exact unsplit level subset.
    for level in chosen:
        assert level['width'] <= 16 and level['depth'] <= 8 and level['mask'] < 4
        assert all(cell[0] in (0, 1, 3, 4, 5, 6) and cell[3] < 4 for cell in level['cells'])
    groups = [W.floors(level, f"floor{n}") for n, level in enumerate(chosen)]
    for channel in [1, 2]:
        cells = [(i % chosen[1]['width'], i // chosen[1]['width'])
                 for i, cell in enumerate(chosen[1]['cells'])
                 if cell[0] == 5 and cell[1] == channel]
        groups.append([W.floor(chosen[1], cells, f"bridge{channel}")])
    groups.append([W.block()])
    groups.append(W.well())
    groups.append([W.floor(dict(width=1,depth=1),[(0,0)],'tile0',0)])
    groups.append([W.floor(dict(width=1,depth=1),[(0,0)],'tile1',1)])
    groups.append([W.soft_glyph()])
    groups.append([W.heavy_base()])
    groups.append([W.heavy_glyph()])
    import copy
    for bridge in groups[2:4]:
        hint=copy.deepcopy(bridge[0]);hint['color_mode']=1
        for vertex in hint['vertices']:vertex[2]=0xffffff
        groups.append([hint])
    pad = B.S.box("pad", [.5] * 3, [(1,1),(0,1),(2,1),(1,-1),(0,-1),(2,-1)], 7)
    for vertex in pad['vertices']:
        vertex[2] = 0xffffff
    groups.append([pad])
    groups.append([dict(name='shadow-triangle',group='shadow-triangle',vertices=[[0,511<<10,0xffffff],[128,511<<10,0xffffff],[128<<20,511<<10,0xffffff]],triangles=[[0,1,2]],config=[0,0,0,9|256],color_mode=1,cull_negative=False)])
    # Ordinary UV quad; same native format as the fixed RTT demo.
    screen = dict(name="screen", group="screen", vertices=[], triangles=[[0,1,2],[0,2,3]],
                  config=[-65536,-65536,0,8], color_mode=0, cull_negative=False)
    for (x,y),uv in zip([(0,0),(512,0),(512,512),(0,512)],
                        [0xffffffff,0xffff0000,0,0x0000ffff]):
        screen['vertices'].append([x|(y<<10),511<<20,uv])
    groups.append([screen])
    count = sum(map(len,groups))
    memory = bytearray((count*4+127)//128*128)
    names = ['FLOOR0','FLOOR1','BRIDGE0','BRIDGE1','BLOCK','WELL','TILE0','TILE1','SOFT_GLYPH','HEAVY_BASE','HEAVY_GLYPH','HINT0','HINT1','PAD','SHADOW','SCREEN']
    catalog = []
    meshes = []
    entry = 0
    for name,group in zip(names,groups):
        catalog.append(dict(name=name,first=entry,count=len(group)))
        for mesh in group:
            record = bytearray(G.native_record(mesh))
            # Preserve the source geometry's two-sided contract.
            q = struct.unpack_from('<Q',record)[0]
            struct.pack_into('<Q',record,0,(q & ~(1<<31)) | (int(mesh['cull_negative'])<<31))
            sector=(BASE+len(memory))//128
            struct.pack_into('<HH',memory,entry*4,sector,len(record)//128-1)
            mesh['record_sector']=sector
            mesh['payload_sectors']=len(record)//128-1
            # Decode the packed native image independently of the old prepare model.
            q0,q1,q2,q3=struct.unpack_from('<4Q',record)
            assert q0&127==len(mesh['vertices']) and (q0>>8)&127==len(mesh['triangles'])
            assert (q0>>16)&63==20 and (q0>>31)&1==int(mesh['cull_negative'])
            assert q1==mesh['color_mode']<<4 and q3>>32==mesh['config'][3]
            assert q2 == (mesh['config'][0] & 0xffffffff) | ((mesh['config'][1] & 0xffffffff) << 32)
            assert q3 & 0xffffffff == mesh['config'][2] & 0xffffffff
            assert record[32:128] == bytes(96)
            for i,v in enumerate(mesh['vertices']):
                assert list(struct.unpack_from('<3I',record,128+12*i))==v
            offset=(q0>>24)&127
            for i,tri in enumerate(mesh['triangles']):
                packed=struct.unpack_from('<Q',record,128+(offset+i//3)*8)[0]>>(18*(i%3))
                assert [(packed>>(6*k))&63 for k in range(3)]==tri
            memory.extend(record);meshes.append(mesh);entry+=1
    assert BASE+len(memory)<texture
    R.validate_asset_range(BASE,len(memory))
    (output/'mesh.bin').write_bytes(memory)
    constants=(output/'renderer_assets.rs').read_text()
    for key,value in dict(GPU_RENDERER_MESH_COUNT=count,GPU_RENDERER_MESH_SECTORS=len(memory)//128,GPU_RENDERER_TEXTURE=texture//128,GPU_RENDERER_TEXTURE_SECTORS=(output/'texture.bin').stat().st_size//128,GPU_RENDERER_TEXTURE_LOG2=9,GPU_RENDERER_TEXTURE_MIP=1).items():
        constants=re.sub(rf'(pub const {key}:u16=)\d+;',rf'\g<1>{value};',constants)
    # Conservative shared package bounds, including all floors and unit block.
    for axis,lo,hi in [('X',-16*65536,16*65536),('Y',-65536,65536),('Z',-6*65536,6*65536)]:
        for label,value in [('MIN',lo),('MAX',hi)]:
            for part,n in [('LOW',value&65535),('HIGH',(value>>16)&65535)]:
                key=f'GPU_RENDERER_{label}_{axis}_{part}'
                constants=re.sub(rf'(pub const {key}:u16=)\d+;',rf'\g<1>{n};',constants)
    constants += 'pub const BLOX_BACKGROUND_SECTOR:u16=45056;\n'
    for n, color in enumerate([0x38bdf8,0xfb923c]):
        for shade, rgb in zip(['DIM','BASE','GLOW','INK'],W.palette(color)):
            constants += f'pub const BLOX_CHANNEL{n}_{shade}_RG:u16={rgb[0] | (rgb[1]<<8)};\n'
            constants += f'pub const BLOX_CHANNEL{n}_{shade}_B:u16={rgb[2]};\n'
    for c in catalog:
        constants+=f"pub const {c['name']}_FIRST:u16={c['first']};\n"+f"pub const {c['name']}_COUNT:u16={c['count']};\n"
    constants += 'use crate::dsl_rt::*;\n'
    constants += 'pub fn blox_fixture_level(which:u16, mut out:Array<u16>) {\n'
    original={}
    for line in (SYSTEM/'examples/bloxorz/assets/reference.txt').read_text().splitlines():
        if line.startswith('level '):
            parts=line.split();original[parts[1]]=list(map(int,parts[3:]))
    for n,level in enumerate(chosen):
        constants+=('if' if n==0 else 'else if')+f' which == {n} {{\n'
        words=original[level['name']]
        assert len(words)<=728
        constants+=''.join(f'out[{i}u16]={v};\n' for i,v in enumerate(words))+'}\n'
    constants+='}\n'
    for n, level in enumerate(chosen):
        camera = W.camera(level['width'], level['depth'])
        for label, value in list(zip(['EYE_X','EYE_Y','EYE_Z'], camera['eye'])) + [('FOCAL', camera['focal'])]:
            bits = round(value * 65536)
            constants += f'pub const BLOX_CAMERA{n}_{label}_LOW:u16={bits & 65535};\n'
            constants += f'pub const BLOX_CAMERA{n}_{label}_HIGH:u16={(bits >> 16) & 65535};\n'
    (output/'renderer_assets.rs').write_text(constants)
    for r in manifest['resources']:
        data=(output/r['file']).read_bytes();r.update(bytes=len(data),sha256=hashlib.sha256(data).hexdigest())
        assert r['address']+len(data)<=0x600000 or r['address']>=0x680000
    extents=sorted((r['address'],r['address']+r['bytes'],r['file']) for r in manifest['resources'])
    assert all(a[1] <= b[0] for a,b in zip(extents,extents[1:]))
    manifest.update(profile='production',surface='column-major-16x16-pad16',
        borrowed=dict(color=0x600000,depth=0x640000,plane_capacity=262144),catalog=catalog,meshes=meshes,levels=chosen,
        triangles=sum(len(m["triangles"]) for m in meshes),vertices=sum(len(m["vertices"]) for m in meshes),
        bounds=[[-16,-1,-6],[16,1,6]], cameras=[W.camera(l["width"],l["depth"]) for l in chosen], pixel_reference=dict(aspect="5:3", atlas_sha256=hashlib.sha256((SYSTEM/"examples/bloxorz/assets/atlas-low.png").read_bytes()).hexdigest(), background_srgb="#2c2620"))
    for path in [Path(__file__).resolve(),Path(__file__).with_name("web_scene.py"),background,SYSTEM/'examples/bloxorz/assets/atlas-low.png',SYSTEM/'tools/gpu_texture_layout.py',SYSTEM/'examples/bloxorz/tools/batch-scene.py',SYSTEM/'examples/bloxorz/tools/static-scene.py',SYSTEM/'examples/bloxorz/assets/reference.txt',SYSTEM/'examples/bloxorz/rcc/bloxorz_core.rs',SYSTEM/'examples/bloxorz/geometry.rs',SYSTEM/'examples/gpu_demo/tools/assets.py']:
        manifest['sources'][str(path.relative_to(SYSTEM))]=hashlib.sha256(path.read_bytes()).hexdigest()
    (output/'assets.json').write_text(json.dumps(manifest,indent=2)+'\n')

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--output',type=Path,required=True);parser.add_argument('--profile',choices=['column16'],default='column16')
    pack(parser.parse_args().output)
