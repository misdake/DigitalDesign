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
M = module("blox_markers", Path(__file__).with_name("markers.py"))
H = module("blox_hud_font", Path(__file__).with_name("hud_font.py"))
C = module("blox_goal_carrier", Path(__file__).with_name("goal_carrier.py"))
F = module("blox_ground_batches", Path(__file__).with_name("ground_batches.py"))

def compact_level(words):
    assert len(words)==8+words[6]*8 and words[6]<=150 and words[5]<512
    result=words[:7]+[2]
    for i in range(words[6]):
        kind,bit,keep,xor,x1,z1,x2,z2=words[8+i*8:16+i*8]
        assert 0<=kind<=7 and (bit|xor|(~keep&4095))<512
        first,second=kind,0
        if kind in (3,4):first|=keep<<3;second=xor|((bit.bit_length()-1)<<12)
        elif kind==5:
            assert bit and not bit&(bit-1)
            second=(bit.bit_length()-1)<<12
        elif kind==7 and x1!=65535:
            assert all(0<=v<32 for v in [x1,z1,x2,z2])
            first|=x1<<3|z1<<8;second=x2|z2<<5|32768
        result.extend([first,second])
    return result

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
    hud_font=Path(__file__).resolve().parents[1]/'assets/hud-font.txt'
    base_font_provenance=hud_font.with_name('hud-font-provenance.json')
    manifest['hud_font']=json.loads(base_font_provenance.read_text())
    ui=bytearray(65536)
    for line in hud_font.read_text().splitlines():
        if not line or line.startswith('#'):continue
        fields=line.split();glyph=int(fields[0]);rows=[int(v,16) for v in fields[1:]]
        assert len(rows)==16 and 0<=glyph<255
        for y,bits in enumerate(rows):
            for x in range(8):struct.pack_into('<H',ui,glyph*256+(y*8+(x^((y&1)*2)))*2,65535 if bits&(128>>x) else 0)
    assert all(any(ui[g*256:(g+1)*256]) for g in set(b'LEVEL DRAW MS')-{32})
    (output/'atlas.bin').write_bytes(ui)
    H.preview(ui).save(output/'hud-font-atlas-1x1.png')
    marker_data,marker_image,marker_groups=M.prepare(SYSTEM,W)
    (output/'markers.bin').write_bytes(marker_data);marker_image.save(output/'markers-atlas.png')
    R.validate_asset_range(0x740000,len(marker_data))
    manifest['resources'].append(dict(kind='markers',file='markers.bin',address=0x740000,bytes=len(marker_data)))
    manifest['marker_atlas']=dict(size=M.SIZE,slots=60,rectangles=[M.slot_rect(i) for i in range(60)],fragile_inner=44,gutter='2 except constant-color patches1',full_mip=False,uv='inner texel centres, periodic base layer only')
    levels = B.levels()
    chosen = [levels[n] for n in ["b1", "b2"]]
    groups = []
    groups.append([W.block()])
    groups.append(W.well())
    groups.append([W.floor(dict(width=1,depth=1),[(0,0)],'tile0',0)])
    groups.append([W.floor(dict(width=1,depth=1),[(0,0)],'tile1',1)])
    pad = W.plain("pad", W.box_quads((0,0,0),(1,1,1)))
    groups.append([pad])
    groups.append([dict(name='shadow-triangle',group='shadow-triangle',vertices=[[0,511<<10,0xffffff],[128,511<<10,0xffffff],[128<<20,511<<10,0xffffff]],triangles=[[0,1,2]],config=[0,0,0,9|256],color_mode=1,cull_negative=False)])
    # Ordinary UV quad; same native format as the fixed RTT demo.
    screen = dict(name="screen", group="screen", vertices=[], triangles=[[0,1,2],[0,2,3]],
                  config=[-65536,-65536,0,8], color_mode=0, cull_negative=False)
    for (x,y),uv in zip([(0,0),(512,0),(512,512),(0,512)],
                        [0xffffffff,0xffff0000,0,0x0000ffff]):
        screen['vertices'].append([x|(y<<10),511<<20,uv])
    groups.append([screen])
    groups.append([W.unit_floor_face(face,variant) for variant in range(4) for face in range(5)])
    groups += [[W.unit_tile(v) for v in range(4)],[W.unit_tile(0,True)]]+marker_groups
    groups += C.prepare(W,SYSTEM,output)
    manifest['resources'].append(dict(kind='goal_carrier',file='goal_carrier.bin',address=0x760000))
    count = sum(map(len,groups))
    memory = bytearray((count*4+127)//128*128)
    names = ['BLOCK','WELL','TILE0','TILE1','PAD','SHADOW','SCREEN','SHARED_FLOOR','BRIDGE_TILE','HINT_TILE','MARKED_SOFT','MARKED_HEAVY','MARKED_FRAGILE','MARKED_SPLIT','GOAL_CARRIER','GOAL_HATCH']
    assert len(names)==len(groups)
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
    assert BASE+len(memory)<=0x4f0000
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
    constants += 'pub const BLOX_BACKGROUND_SECTOR:u16=45056;\npub const BLOX_MARKER_SECTOR:u16=59392;\n'
    for c in catalog:
        constants+=f"pub const {c['name']}_FIRST:u16={c['first']};\n"+f"pub const {c['name']}_COUNT:u16={c['count']};\n"
    constants += 'use crate::dsl_rt::*;\n'
    original={}
    for line in (SYSTEM/'examples/bloxorz/assets/reference.txt').read_text().splitlines():
        if line.startswith('level '):
            fields=line.split();original[fields[1]]=list(map(int,fields[3:]))
    all_levels=[levels[f'b{i}'] for i in range(1,34)]
    archive=[0x424c,1,33,16]+[0]*(33*16)
    archive_meta=[]
    for n,level in enumerate(all_levels):
        packed=compact_level(original[level['name']]);faces=W.visible_floor_faces(level)
        record=[len(archive),len(packed),len(archive)+len(packed),len(faces)]
        cam=W.camera(level['width'],level['depth'])
        far,bounds=W.conservative_far(level)
        assert 0<far<199
        for v in cam['eye']+[cam['focal'],far]:
            bits=round(v*65536);record.extend([bits&65535,(bits>>16)&65535])
        batch_faces=F.schedule(level,faces,C.applicable(level))
        record.extend([int(C.applicable(level)),len(archive)+len(packed)+len(faces)]);assert len(record)==16
        archive[4+n*16:4+(n+1)*16]=record
        archive.extend(packed);archive.extend(faces);archive.extend([len(batch_faces)]+batch_faces)
        archive_meta.append(dict(level=n+1,level_offset=record[0],faces_offset=record[2],faces=len(faces),far=far,animation_bounds=bounds))
    palette_word=len(archive)
    for color in [0x38bdf8,0xfb923c,0x4ade80,0xa78bfa,0xfacc15,0xf472b6,0x22d3ee,0xf87171]:
        for rgb in W.palette(color):archive.extend([rgb[0]|(rgb[1]<<8),rgb[2]])
    constants+=f'pub const BLOX_PALETTE_WORD:u16={palette_word};\n'
    ground_template_word=len(archive);archive.extend(F.templates(W,marker_groups[-1]))
    constants+=f'pub const BLOX_GROUND_TEMPLATE_WORD:u16={ground_template_word};\n'
    assert len(archive)<65536
    binary=struct.pack('<'+'H'*len(archive),*archive);binary+=bytes((-len(binary))%128)
    assert 0x5b2000+len(binary)<=0x600000
    (output/'blox_levels.bin').write_bytes(binary)
    manifest['resources'].append(dict(kind='blox_levels',file='blox_levels.bin',address=0x5b2000,bytes=len(binary)))
    constants += 'pub const BLOX_LEVEL_COUNT:u16=33;\npub const BLOX_LEVEL_DATA_SECTOR:u16=46656;\n'
    constants += r"""
// Read-only app assets. Restore DSEG3 before renderer calls; no pointer escapes.
fn blox_asset_word(word:u16)->u16 {
    let sector=BLOX_LEVEL_DATA_SECTOR+(word>>6);
    let saved=read_dseg_page(3);dcache_wait();
    unsafe {write_dseg_page(3,sector>>8);}
    let value=unsafe {Ptr::from_addr(0xc000|((sector&255)<<6)|(word&63)).read(0)};
    dcache_wait();unsafe {write_dseg_page(3,saved);}
    value
}
pub fn blox_palette_word(channel:u16,shade:u16,part:u16)->u16 {blox_asset_word(BLOX_PALETTE_WORD+(channel&7)*8+shade*2+part)}
pub fn blox_level_meta(which:u16,field:u16)->u16 {blox_asset_word(4+which*16+field)}
pub fn blox_fixture_level(which:u16,mut out:Array<u16>){
    if which>=BLOX_LEVEL_COUNT {halt(0xb10);}
    let start=blox_level_meta(which,0);let count=blox_level_meta(which,1);
    let mut i:u16=0;while i<count {out[i]=blox_asset_word(start+i);i+=1;}
}
pub fn blox_ground_faces_base(which:u16)->u16 {blox_level_meta(which,15)}
pub fn blox_ground_face(base:u16,index:u16)->u16 {blox_asset_word(base+index)}
pub fn blox_ground_template(which:u16,mut out:Array<u16>){
    let start=BLOX_GROUND_TEMPLATE_WORD+which*20;
    let saved=read_dseg_page(3);dcache_wait();
    let mut page:u16=65535;let mut i:u16=0;
    while i<20 {
        let word=start+i;let sector=BLOX_LEVEL_DATA_SECTOR+(word>>6);
        if sector>>8!=page {dcache_wait();page=sector>>8;unsafe{write_dseg_page(3,page);}}
        out[i]=unsafe{Ptr::from_addr(0xc000|((sector&255)<<6)|(word&63)).read(0)};
        i+=1;
    }
    dcache_wait();unsafe{write_dseg_page(3,saved);}
}
pub fn blox_scene_face_count(which:u16)->u16 {blox_level_meta(which,3)}
pub fn blox_scene_face(which:u16,index:u16)->u16 {blox_asset_word(blox_level_meta(which,2)+index)}
"""
    manifest['stage_archive']=archive_meta
    manifest['goal_carrier']=dict(apertures=48,step=.02,plane_y=0,texture_phases=4,fallback_levels=[i+1 for i,l in enumerate(all_levels) if not C.applicable(l)],baked_floor_g=232,baked_floor_lod_q3=27,continuous_state_animation=True)
    (output/'renderer_assets.rs').write_text(constants)
    for r in manifest['resources']:
        data=(output/r['file']).read_bytes();r.update(bytes=len(data),sha256=hashlib.sha256(data).hexdigest())
        assert r['address']+len(data)<=0x600000 or 0x680000<=r['address']<r['address']+len(data)<=0x7df800
    extents=sorted((r['address'],r['address']+r['bytes'],r['file']) for r in manifest['resources'])
    assert all(a[1] <= b[0] for a,b in zip(extents,extents[1:]))
    manifest.update(profile='production',surface='column-major-16x16-pad16',
        borrowed=dict(color=0x600000,depth=0x480000,plane_capacity=262144),catalog=catalog,meshes=meshes,levels=chosen,
        triangles=sum(len(m["triangles"]) for m in meshes),vertices=sum(len(m["vertices"]) for m in meshes),
        bounds=[[-16,-1,-6],[16,1,6]], cameras=[W.camera(l["width"],l["depth"]) for l in chosen], pixel_reference=dict(aspect="5:3", atlas_sha256=hashlib.sha256((SYSTEM/"examples/bloxorz/assets/atlas-low.png").read_bytes()).hexdigest(), background_srgb="#2c2620"))
    for path in [Path(__file__).resolve(),Path(__file__).with_name("web_scene.py"),Path(__file__).with_name("markers.py"),Path(__file__).with_name("hud_font.py"),Path(__file__).with_name("goal_carrier.py"),Path(__file__).with_name("ground_batches.py"),hud_font,base_font_provenance,background,SYSTEM/'examples/bloxorz/assets/atlas-low.png',SYSTEM/'tools/gpu_texture_layout.py',SYSTEM/'examples/bloxorz/tools/batch-scene.py',SYSTEM/'examples/bloxorz/tools/static-scene.py',SYSTEM/'examples/bloxorz/assets/reference.txt',SYSTEM/'examples/bloxorz/rcc/bloxorz_core.rs',SYSTEM/'examples/bloxorz/geometry.rs',SYSTEM/'examples/gpu_demo/tools/assets.py']:
        manifest['sources'][str(path.relative_to(SYSTEM))]=hashlib.sha256(path.read_bytes()).hexdigest()
    (output/'assets.json').write_text(json.dumps(manifest,indent=2)+'\n')

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--output',type=Path);parser.add_argument('--compact-reference',type=Path);parser.add_argument('--profile',choices=['column16'],default='column16')
    args=parser.parse_args()
    if args.compact_reference:
        rows=[]
        for line in (SYSTEM/'examples/bloxorz/assets/reference.txt').read_text().splitlines():
            if line.startswith('level '):
                fields=line.split();line=' '.join(fields[:3]+list(map(str,compact_level(list(map(int,fields[3:]))))))
            rows.append(line)
        args.compact_reference.parent.mkdir(parents=True,exist_ok=True)
        args.compact_reference.write_text('\n'.join(rows)+'\n')
    else:
        assert args.output is not None
        pack(args.output)
