"""Round3 immutable batches, using the existing Bloxorz geometry without rendering."""
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
BASE = R.MESH

def pack(output):
    R.TEXTURE = 0x4bf000
    R.pack(output)
    levels = B.levels()
    chosen = [levels[n] for n in ["fixture-switch", "fixture-split"]]
    groups = []
    for level in chosen:
        cells = [(i % level['width'], i // level['width']) for i,c in enumerate(level['cells']) if c[0] not in (0,5,6)]
        buckets = {}
        for x,z in cells: buckets.setdefault((x//3,z//3),[]).append((x,z))
        groups.append([B.meshlet(level, bucket[i:i+5], "floor") for bucket in buckets.values() for i in range(0,len(bucket),5)])
    bridge_cells = [(i % chosen[0]["width"], i // chosen[0]["width"]) for i,c in enumerate(chosen[0]["cells"]) if c[0] == 5]
    assert len(bridge_cells) == 1
    groups.append([B.meshlet(chosen[0], bridge_cells, "bridge")])
    groups.append([B.S.box("block", [.49]*3, [(1,1),(0,1),(2,1),(1,-1),(0,-1),(2,-1)], 7)])
    count = sum(map(len,groups))
    memory = bytearray((count*4+127)//128*128)
    names = ['FLOOR0','FLOOR1','BRIDGE','BLOCK']
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
    assert BASE+len(memory)<R.TEXTURE
    R.validate_asset_range(BASE,len(memory))
    (output/'mesh.bin').write_bytes(memory)
    constants=(output/'renderer_assets.rs').read_text()
    for key,value in dict(GPU_RENDERER_MESH_COUNT=count,GPU_RENDERER_MESH_SECTORS=len(memory)//128).items():
        constants=re.sub(rf'(pub const {key}:u16=)\d+;',rf'\g<1>{value};',constants)
    # Conservative shared package bounds, including all floors and unit block.
    for axis,lo,hi in [('X',-8*65536,8*65536),('Y',-65536,65536),('Z',-3*65536,3*65536)]:
        for label,value in [('MIN',lo),('MAX',hi)]:
            for part,n in [('LOW',value&65535),('HIGH',(value>>16)&65535)]:
                key=f'GPU_RENDERER_{label}_{axis}_{part}'
                constants=re.sub(rf'(pub const {key}:u16=)\d+;',rf'\g<1>{n};',constants)
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
        assert len(words)==200
        constants+=''.join(f'out[{i}u16]={v};\n' for i,v in enumerate(words))+'}\n'
    constants+='}\n'
    (output/'renderer_assets.rs').write_text(constants)
    manifest=json.loads((output/'assets.json').read_text())
    for r in manifest['resources']:
        data=(output/r['file']).read_bytes();r.update(bytes=len(data),sha256=hashlib.sha256(data).hexdigest())
    manifest.update(catalog=catalog,meshes=meshes,levels=chosen,
        triangles=sum(len(m["triangles"]) for m in meshes),vertices=sum(len(m["vertices"]) for m in meshes),
        bounds=[[-8,-1,-3],[8,1,3]])
    for path in [Path(__file__).resolve(),SYSTEM/'examples/bloxorz/tools/batch-scene.py',SYSTEM/'examples/bloxorz/tools/static-scene.py',SYSTEM/'examples/bloxorz/assets/reference.txt',SYSTEM/'examples/bloxorz/rcc/bloxorz_core.rs',SYSTEM/'examples/bloxorz/geometry.rs',SYSTEM/'examples/gpu_demo/tools/assets.py']:
        manifest['sources'][str(path.relative_to(SYSTEM))]=hashlib.sha256(path.read_bytes()).hexdigest()
    (output/'assets.json').write_text(json.dumps(manifest,indent=2)+'\n')

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--output',type=Path,required=True)
    pack(parser.parse_args().output)
