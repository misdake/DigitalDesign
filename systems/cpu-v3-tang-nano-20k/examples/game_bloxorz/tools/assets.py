"""Bloxorz column-layout scene and fullscreen RTT mesh, using the existing Bloxorz geometry without rendering."""
import argparse
import copy
import hashlib
import importlib.util
import json
import os
import re
from pathlib import Path
import struct
import sys
sys.dont_write_bytecode = True
SYSTEM = Path(__file__).resolve().parents[3]
def visible_side_mesh(source, side):
    mesh = copy.deepcopy(source)
    vertices, triangles, kept = [], [], []
    for face in range(len(source['vertices']) // 4):
        normal = source['vertices'][face * 4][1]
        xyz = [(normal >> (axis * 10)) & 1023 for axis in range(3)]
        x, y, z = [value if value < 512 else value - 1024 for value in xyz]
        if y > 0 or z > 0 or (x > 0 if side == 0 else x < 0):
            first = len(vertices)
            vertices.extend(source['vertices'][face * 4:face * 4 + 4])
            triangles.extend([[first, first + 1, first + 2], [first, first + 2, first + 3]])
            kept.append(face)
    mesh.update(vertices=vertices, triangles=triangles, cull_negative=True,
                name=source['name'] + f'-visible-{side}', visibility_side=side)
    if 'marker_slots' in mesh:
        mesh['marker_slots']['top_face'] = kept.index(mesh['marker_slots']['top_face'])
    return mesh

def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result
B = module("blox_batch", SYSTEM / "applications/bloxorz/tools/batch-scene.py")
R = module("minimal_renderer", SYSTEM / "applications/rendering/tools/minimal_assets.py")
G = module("native_assets", SYSTEM / "examples/demo_box_rabbit/tools/assets.py")
W = module("blox_web_scene", Path(__file__).with_name("web_scene.py"))
M = module("blox_markers", Path(__file__).with_name("markers.py"))
import ui_font as H
C = module("blox_goal_carrier", Path(__file__).with_name("goal_carrier.py"))
F = module("blox_ground_batches", Path(__file__).with_name("ground_batches.py"))
import atlas_borders as A

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

def pack(output,main_log2=8,marker_log2=8):
    assert main_log2 in (6,7,8) and marker_log2 in (7,8)
    A.LOG2=main_log2;M.SIZE=1<<marker_log2
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
    manifest['hud_font']=json.loads(H.PROVENANCE.read_text())
    ui=H.atlas(H.load_rows())
    assert all(any(ui[g*256:(g+1)*256]) for g in set(b'LEVEL DRAW MS.0123456789-')-{32})
    (output/'atlas.bin').write_bytes(ui)
    H.preview(ui).save(output/'hud-font-atlas-1x1.png')
    marker_data,marker_image,marker_groups,marker_items=M.prepare(SYSTEM,W)
    (output/'markers.bin').write_bytes(marker_data);marker_image.save(output/'markers-atlas.png')
    R.validate_asset_range(0x740000,len(marker_data))
    manifest['resources'].append(dict(kind='markers',file='markers.bin',address=0x740000,bytes=len(marker_data)))
    manifest['marker_atlas']=dict(size=M.SIZE,slots=60,rectangles=[M.slot_rect(i) for i in range(60)],items=marker_items,fragile_inner=M.slot_rect(48)[2]-4,gutter='2 except active-heavy patches1',full_mip=False,uv='inner texel centres, periodic base layer only')
    from PIL import Image
    masks=Image.new('L',(M.SIZE,M.SIZE))
    for item in marker_items:
        x,y,edge,gutter=item['rect'];masks.paste(M.glyph_mask(item['pattern'],item['inner']),(x+gutter,y+gutter))
    masks.save(output/'marker-masks.png')
    levels = B.levels()
    chosen = [levels[n] for n in ["b1", "b2"]]
    groups = []
    groups.append([W.block()])
    groups.append(W.well())
    hatch=W.unit_tile(0);hatch['name']=hatch['group']='pad-hatch';hatch['cull_negative']=True
    groups.append([hatch])
    groups.append([W.unit_tile(1)])
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
    names = ['BLOCK','WELL','TILE0','TILE1','PAD','SHADOW','SCREEN','SHARED_FLOOR','BRIDGE_TILE','HINT_TILE','MARKED_SOFT','MARKED_HEAVY','MARKED_FRAGILE','MARKED_SPLIT','GOAL_CARRIER','GOAL_HATCH']
    assert len(names)==len(groups)
    for index, name in enumerate(names):
        if name in {'TILE1', 'BRIDGE_TILE', 'MARKED_SOFT'}:
            groups[index] = [visible_side_mesh(mesh, side) for side in (0, 1) for mesh in groups[index]]
    for level in levels.values():
        assert W.camera(level['width'], level['depth'])['eye'][2] > level['depth'] / 2
    count = sum(map(len,groups))
    memory = bytearray((count*4+127)//128*128)
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
    for key,value in dict(GPU_RENDERER_MESH_COUNT=count,GPU_RENDERER_MESH_SECTORS=len(memory)//128,GPU_RENDERER_TEXTURE=texture//128,GPU_RENDERER_TEXTURE_SECTORS=(output/'texture.bin').stat().st_size//128,GPU_RENDERER_TEXTURE_LOG2=main_log2,GPU_RENDERER_TEXTURE_MIP=0).items():
        constants=re.sub(rf'(pub const {key}:u16=)\d+;',rf'\g<1>{value};',constants)
    # Conservative shared package bounds, including all floors and unit block.
    for axis,lo,hi in [('X',-16*65536,16*65536),('Y',-65536,65536),('Z',-6*65536,6*65536)]:
        for label,value in [('MIN',lo),('MAX',hi)]:
            for part,n in [('LOW',value&65535),('HIGH',(value>>16)&65535)]:
                key=f'GPU_RENDERER_{label}_{axis}_{part}'
                constants=re.sub(rf'(pub const {key}:u16=)\d+;',rf'\g<1>{n};',constants)
    constants += 'pub const BLOX_BACKGROUND_SECTOR:u16=45056;\npub const BLOX_MARKER_SECTOR:u16=59392;\n'
    constants+=f'pub const BLOX_MARKER_LOG2:u16={marker_log2};\npub const BLOX_MARKER_SECTORS:u16={M.SIZE*M.SIZE//64};\n'
    for c in catalog:
        constants+=f"pub const {c['name']}_FIRST:u16={c['first']};\n"+f"pub const {c['name']}_COUNT:u16={c['count']};\n"
    constants += 'use crate::dsl_rt::*;\n'
    original={}
    for line in (SYSTEM/'applications/bloxorz/assets/reference.txt').read_text().splitlines():
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
    fragile_template_word=len(archive);archive.extend(F.fragile_templates(marker_groups[-2]))
    constants+=f'pub const BLOX_FRAGILE_TEMPLATE_WORD:u16={fragile_template_word};\n'
    assert len(archive)<65536
    binary=struct.pack('<'+'H'*len(archive),*archive);binary+=bytes((-len(binary))%128)
    assert 0x5b2000+len(binary)<=0x600000
    (output/'blox_levels.bin').write_bytes(binary)
    manifest['resources'].append(dict(kind='blox_levels',file='blox_levels.bin',address=0x5b2000,bytes=len(binary)))
    initial = int(os.environ.get('BLOXORZ_START_LEVEL', '1'))
    if not 1 <= initial <= 33:
        raise ValueError('BLOXORZ_START_LEVEL must be in 1..33')
    constants += f'pub const BLOXORZ_INITIAL_LEVEL:u16={initial-1};\n'
    manifest['initial_level'] = initial
    phase = os.environ.get('BLOXORZ_START_PHASE', 'entry')
    assert phase in ('entry', 'play'), 'BLOXORZ_START_PHASE must be entry or play'
    constants += f'pub const BLOXORZ_INITIAL_PLAY:u16={int(phase == "play")};\n'
    manifest['initial_phase'] = phase
    constants += 'pub const BLOX_LEVEL_COUNT:u16=33;\npub const BLOX_LEVEL_DATA_SECTOR:u16=46656;\n'
    # CPU-only tables share the archive's single generated source of truth.
    def cpu_words(name, words):
        return f'pub static {name}:Buf<u16,{len(words)}>=Buf::new([' + ','.join(map(str, words)) + ']);\n'
    constants += """
#[repr(C)]
#[derive(Copy, Clone)]
pub struct BloxLevelHeader {
    pub packed_first:u16,pub packed_words:u16,pub faces_first:u16,pub face_count:u16,
    pub eye:Vec3,pub focal:fix32,pub far:fix32,pub carrier:u16,pub ground_first:u16,
}
"""
    headers = []
    for n in range(33):
        row = archive[4 + n * 16:4 + (n + 1) * 16]
        def fixed(at):
            return f'fix32::from_words({row[at]},{row[at + 1]})'
        eye = f'Vec3{{x:{fixed(4)},y:{fixed(6)},z:{fixed(8)}}}'
        headers.append('BloxLevelHeader{' +
                       f'packed_first:{row[0]},packed_words:{row[1]},faces_first:{row[2]},face_count:{row[3]},' +
                       f'eye:{eye},focal:{fixed(10)},far:{fixed(12)},carrier:{row[14]},ground_first:{row[15]}' + '}')
    constants += 'pub static BLOX_LEVEL_HEADERS:Buf<BloxLevelHeader,33>=Buf::new([' + ','.join(headers) + ']);\n'

    constants += cpu_words('BLOX_PALETTE', archive[palette_word:ground_template_word])
    constants += cpu_words('BLOX_GROUND_TEMPLATES', archive[ground_template_word:fragile_template_word])
    constants += cpu_words('BLOX_FRAGILE_TEMPLATES', archive[fragile_template_word:])
    geometry_words = max(
        (archive_meta[n + 1]['level_offset'] if n + 1 < len(archive_meta) else palette_word)
        - meta['faces_offset'] for n, meta in enumerate(archive_meta))
    constants += f'pub const BLOX_LEVEL_GEOMETRY_WORDS:u16={geometry_words};\n'
    constants += f'pub const BLOX_ARCHIVE_WORDS:u16={len(archive)};\n'
    constants += r"""
// Immutable source in external SDRAM; destination stays below the DSEG3 window.
// The cache tags and outstanding accesses already carry physical addresses.
pub fn blox_asset_read(start:u16,mut out:Array<u16>,count:u16) {
    if start>BLOX_ARCHIVE_WORDS || count>BLOX_ARCHIVE_WORDS-start
        || out.as_ptr().addr()>=0xc000 || count>0xc000-out.as_ptr().addr() {halt(0xb14);}
    if count==0 {return;}
    let saved=read_dseg_page(3);
    let mut word=start;let mut copied:u16=0;
    while copied<count {
        let sector=BLOX_LEVEL_DATA_SECTOR+(word>>6);
        let offset=((sector&255)<<6)|(word&63);
        unsafe {write_dseg_page(3,sector>>8);}
        let source=Ptr::from_addr(0xc000|offset);
        let mut chunk=count-copied;
        if chunk>16384-offset {chunk=16384-offset;}
        let mut i:u16=0;
        while i<chunk {out[copied+i]=unsafe{source.read(i as i16)};i+=1;}
        copied+=chunk;word+=chunk;
    }
    unsafe {write_dseg_page(3,saved);}
}
pub fn blox_palette_word(channel:u16,shade:u16,part:u16)->u16 {
    BLOX_PALETTE[(channel&7)*8+shade*2+part]
}
pub fn blox_fixture_level(which:u16,mut out:Array<u16>){
    if which>=BLOX_LEVEL_COUNT {halt(0xb10);}
    let header=&BLOX_LEVEL_HEADERS[which];
    blox_asset_read(header.packed_first,out,header.packed_words);
}
pub fn blox_ground_template(which:u16,mut out:Array<u16>){
    let start=which*20;let mut i:u16=0;
    while i<20 {out[i]=BLOX_GROUND_TEMPLATES[start+i];i+=1;}
}
pub fn blox_fragile_template(variant:u16,face:u16,mut out:Array<u16>){
    let start=variant*100+face*20;let mut i:u16=0;
    while i<20 {out[i]=BLOX_FRAGILE_TEMPLATES[start+i];i+=1;}
}
"""
    manifest['stage_archive']=archive_meta
    manifest['goal_carrier']=dict(apertures=48,step=.02,plane_y=0,texture_phases=4,fallback_levels=[i+1 for i,l in enumerate(all_levels) if not C.applicable(l)],baked_floor_g=232,baked_floor_lod_q3=0,continuous_state_animation=True)
    constants += r"""
mod gpu_api;
use gpu_api::*;
pub const BLOX_MESH_BYTES:AssetBytes=AssetBytes{
    first_sector:GPU_RENDERER_MESH_FIRST,sector_count:GPU_RENDERER_MESH_SECTORS,
    byte_len_lo:(GPU_RENDERER_MESH_SECTORS&511)<<7,byte_len_hi:GPU_RENDERER_MESH_SECTORS>>9,
};
pub const BLOX_TEXTURE_BYTES:AssetBytes=AssetBytes{
    first_sector:GPU_RENDERER_TEXTURE,sector_count:GPU_RENDERER_TEXTURE_SECTORS,
    byte_len_lo:(GPU_RENDERER_TEXTURE_SECTORS&511)<<7,byte_len_hi:GPU_RENDERER_TEXTURE_SECTORS>>9,
};
pub const BLOX_UI_BYTES:AssetBytes=AssetBytes{
    first_sector:(GPU_RENDERER_ATLAS_BYTE_ADDRESS_HIGH<<9)|(GPU_RENDERER_ATLAS_BYTE_ADDRESS_LOW>>7),
    sector_count:512,byte_len_lo:65280,byte_len_hi:0,
};
pub const BLOX_MESH_DESC:MeshDesc=MeshDesc::from_memory(
    unsafe{GpuMemory::from_asset(BLOX_MESH_BYTES)},
    (GPU_RENDERER_MESH_DIRECTORY-GPU_RENDERER_MESH_FIRST)*64+GPU_RENDERER_MESH_DIRECTORY_WORD,
    GPU_RENDERER_MESH_COUNT,
);
pub const BLOX_TEXTURE_DESC:TextureDesc=TextureDesc::from_memory(
    unsafe{GpuMemory::from_asset(BLOX_TEXTURE_BYTES)},
    TextureLayout::square(1<<GPU_RENDERER_TEXTURE_LOG2,TextureFormat::Rgb565,
        if GPU_RENDERER_TEXTURE_MIP!=0 {Mipmaps::Full}else{Mipmaps::BaseOnly}),
);
pub const BLOX_UI_DESC:UiAtlasDesc=unsafe{UiAtlasDesc::from_asset(BLOX_UI_BYTES)};
"""
    (output/'renderer_assets.rs').write_text(constants)
    for r in manifest['resources']:
        data=(output/r['file']).read_bytes();r.update(bytes=len(data),sha256=hashlib.sha256(data).hexdigest())
        assert r['address']+len(data)<=0x600000 or 0x680000<=r['address']<r['address']+len(data)<=0x7df800
    extents=sorted((r['address'],r['address']+r['bytes'],r['file']) for r in manifest['resources'])
    manifest['runtime_reservations']=[]
    assert all(a[1] <= b[0] for a,b in zip(extents,extents[1:]))
    manifest.update(profile='production',surface='column-major-16x16-pad16',
        borrowed=dict(color=0x600000,depth=0x480000,plane_capacity=262144),catalog=catalog,meshes=meshes,levels=chosen,
        triangles=sum(len(m["triangles"]) for m in meshes),vertices=sum(len(m["vertices"]) for m in meshes),
        bounds=[[-16,-1,-6],[16,1,6]], cameras=[W.camera(l["width"],l["depth"]) for l in chosen], pixel_reference=dict(aspect="5:3", texture_sha256=hashlib.sha256((output/'texture.bin').read_bytes()).hexdigest(), background_srgb="#2c2620"))
    from atlas_borders import contract
    manifest['main_atlas']=contract()
    for path in [Path(__file__).resolve(),Path(__file__).with_name("web_scene.py"),Path(__file__).with_name("atlas_borders.py"),Path(__file__).with_name("source_photos.py"),Path(__file__).with_name("markers.py"),Path(__file__).with_name("goal_carrier.py"),Path(__file__).with_name("ground_batches.py"),*H.SOURCES,background,*A.source.ROOT.glob('*.jpg'),SYSTEM/'tools/gpu_mesh_layout.py',SYSTEM/'tools/gpu_texture_layout.py',SYSTEM/'applications/bloxorz/tools/batch-scene.py',SYSTEM/'applications/bloxorz/tools/static-scene.py',SYSTEM/'applications/bloxorz/assets/reference.txt',SYSTEM/'applications/bloxorz/rcc/bloxorz_core.rs',SYSTEM/'applications/bloxorz/geometry.rs',SYSTEM/'examples/demo_box_rabbit/tools/assets.py']:
        manifest['sources'][path.relative_to(SYSTEM.parents[1]).as_posix()]=hashlib.sha256(path.read_bytes()).hexdigest()
    (output/'assets.json').write_text(json.dumps(manifest,indent=2)+'\n')

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--output',type=Path);parser.add_argument('--compact-reference',type=Path);parser.add_argument('--profile',choices=['column16'],default='column16');parser.add_argument('--main-atlas-log2',type=int,choices=[6,7,8],default=8);parser.add_argument('--marker-atlas-log2',type=int,choices=[7,8],default=8)
    args=parser.parse_args()
    if args.compact_reference:
        rows=[]
        for line in (SYSTEM/'applications/bloxorz/assets/reference.txt').read_text().splitlines():
            if line.startswith('level '):
                fields=line.split();line=' '.join(fields[:3]+list(map(str,compact_level(list(map(int,fields[3:]))))))
            rows.append(line)
        args.compact_reference.parent.mkdir(parents=True,exist_ok=True)
        args.compact_reference.write_text('\n'.join(rows)+'\n')
    else:
        assert args.output is not None
        pack(args.output,args.main_atlas_log2,args.marker_atlas_log2)
