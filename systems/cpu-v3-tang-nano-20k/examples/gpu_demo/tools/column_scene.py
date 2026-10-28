"""Audit the explicit non-RTT column scene package; never certify board matching."""
from pathlib import Path
import argparse
import hashlib
import json
import struct
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[5]
SYSTEM = ROOT/'systems/cpu-v3-tang-nano-20k'
sys.path.insert(0, str(SYSTEM/'tools'))
from gpu_layout import sectors


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def audit(output, contract):
    output = Path(output).resolve()
    contract = Path(contract).resolve()
    expected = json.loads(contract.read_text())
    assert expected['hardware_profile'] == 'texture16x8-column16'
    assert expected['layout_identity'] == 'rgb565-macro16x16-column-major-pad16-cache16x8-8KiB'
    bounds = sectors('column16')
    names = ['colorA','colorB','depth','slot0CB','slot0obj','assets','UI0','UI1','slot1CB','slot1obj']
    starts = [0x400000,0x440000,0x480000,0x4c0000,0x4d0000,0x4e0000,0x7df800,0x7dfc00,0x7e0000,0x7f0000]
    ends = starts[1:] + [0x800000]
    ranges = [dict(name=n,start=a,end=b) for n,a,b in zip(names,starts,ends)]
    assert [(r['name'],int(r['start'],16),int(r['end'],16)) for r in expected['ranges']] == list(zip(names,starts,ends))
    keys = ['GPU_COLOR_A_SECTOR','GPU_COLOR_B_SECTOR','GPU_DEPTH_SECTOR','GPU_COMMAND_FIRST_SECTOR','GPU_OBJECT_FIRST_SECTOR','GPU_ASSET_FIRST_SECTOR','GPU_UI_PAGE0_SECTOR','GPU_UI_PAGE1_SECTOR','GPU_SLOT1_COMMAND_FIRST_SECTOR','GPU_SLOT1_OBJECT_FIRST_SECTOR']
    assert [bounds[k]*128 for k in keys] == starts
    assert bounds['GPU_PLANE_SECTORS']*128 == 262144
    assert bounds['GPU_FRAMEBUFFER_PHYSICAL_SECTORS'] == 1600
    assert (output/'gpu-column-layout.rs').read_bytes() == (SYSTEM/'rcc/gpu_layout_column16.rs').read_bytes()

    # Independent scan traversal: advance vertically through padded macro rows,
    # then the next column. Compare every pixel against the contract expression.
    seen = set()
    visible = padding = 0
    for tile_x in range(25):
        for tile_y in range(16):
            for iy in range(16):
                for ix in range(16):
                    x,y = tile_x*16+ix,tile_y*16+iy
                    at = ((tile_x*16+tile_y)*256+iy*16+ix)*2
                    assert at == 512*(((x>>4)<<4)|(y>>4))+32*(y&15)+2*(x&15)
                    assert at not in seen
                    seen.add(at)
                    if y<240: visible+=1
                    else: padding+=1
    assert (visible,padding,len(seen),max(seen)+2) == (96000,6400,102400,204800)

    assets = output/'gpu-demo-assets'
    manifest = json.loads((assets/'assets.json').read_text())
    assert manifest['hardware_profile'] == expected['hardware_profile']
    assert manifest['layout_identity'] == expected['layout_identity']
    resources = manifest['resources']
    spans=[]
    for item in resources:
        data=(assets/item['file']).read_bytes()
        assert len(data)==item['bytes'] and sha(assets/item['file'])==item['sha256']
        assert 0x4e0000 <= item['address'] < item['address']+len(data) <= 0x7df800
        spans.append((item['address'],item['address']+len(data)))
    assert all(a[1]<=b[0] for a,b in zip(sorted(spans),sorted(spans)[1:]))

    # Decode all relocated directory pointers and geometry directly from scene data.
    scene=json.loads((SYSTEM/'examples/gpu_demo/assets/scene.json').read_text())
    mesh=(assets/'room.bin').read_bytes()
    mesh_base=next(r['address'] for r in resources if r['file']=='room.bin')
    cursor=128
    for i,record in enumerate(scene['meshes']):
        sector,payload=struct.unpack_from('<HH',mesh,4*i)
        assert sector*128==mesh_base+cursor
        nv,nt,uniform,offset,tag=struct.unpack_from('<Q',mesh,cursor)[0]&255,(struct.unpack_from('<Q',mesh,cursor)[0]>>8)&255,(struct.unpack_from('<Q',mesh,cursor)[0]>>16)&255,(struct.unpack_from('<Q',mesh,cursor)[0]>>24)&127,(struct.unpack_from('<Q',mesh,cursor)[0]>>31)&1
        assert (nv,nt,uniform,tag)==(len(record['vertices']),len(record['triangles']),20,1)
        for j,v in enumerate(record['vertices']):
            assert struct.unpack_from('<III',mesh,cursor+128+j*12)==tuple(v)
        for j,tri in enumerate(record['triangles']):
            word=struct.unpack_from('<Q',mesh,cursor+128+(offset+j//3)*8)[0]>>(18*(j%3))
            assert tuple((word>>(6*k))&63 for k in range(3))==tuple(tri)
        cursor+=(payload+1)*128
    assert len(scene['meshes'])==24 and sum(len(m['triangles']) for m in scene['meshes'])==1141
    assert manifest['orbit']['camera_first_sector']*128==mesh_base+cursor
    assert cursor+256*128==len(mesh)
    for i,camera in enumerate(scene['cameras']):
        record=mesh[cursor+i*128:cursor+(i+1)*128]
        codes=struct.pack('<21I3h',*(v&0xffffffff for v in camera['matrix']),*camera['light'])
        assert record[:len(codes)]==codes and record[len(codes):]==bytes(128-len(codes))

    # Decode every real texel and each periodic small-mip padding texel by byte address.
    scene_path=ROOT/'ip/gpu-v2/examples/vliw_integration'
    sys.path.insert(0,str(scene_path))
    import material_scene
    levels=material_scene.texture_levels()
    texture=(assets/'texture.bin').read_bytes()
    cursor=0
    texels=0
    for n in range(7):
        assert cursor//128==expected['texture']['prefix_sectors'][n]
        side=1<<n
        padded=max(16,side)
        for x in range(padded):
            for y in range(padded):
                tile=(x//16)*(padded//16)+y//16
                at=cursor+tile*512+(y%16)*32+(x%16)*2
                assert struct.unpack_from('<H',texture,at)[0]==levels[n][(y%side)*side+x%side]
                texels+=1
        cursor+=padded*padded*2
    assert cursor==len(texture)==12800
    assert manifest['texture']['sectors']==100
    assert next(r['address'] for r in resources if r['file']=='texture.bin')%256==0

    # UI alpha and bank-swizzle remain independent of ordinary texture macros.
    atlas=(assets/'atlas.bin').read_bytes()
    for line in (SYSTEM/'examples/gpu_demo/assets/font-8x16.txt').read_text().splitlines():
        if not line or line.startswith('#'): continue
        words=line.split();glyph=int(words[0])
        for y,row in enumerate(words[1:]):
            row=int(row,16)
            for x in range(8):
                at=glyph*256+2*(y*8+(x^(2 if y&1 else 0)))
                assert struct.unpack_from('<H',atlas,at)[0]==(65535 if row&(128>>x) else 0)
    assert len(atlas)==65536 and atlas[255*256:]==bytes(256)

    # Independent CB06 decode: exact application and resource COPY payloads.
    package=(output/'cpu-v3-boot.bin').read_bytes();assert package[:4]==b'CB06'
    cursor=4;copies={}
    for _ in range(64):
        address,control=struct.unpack_from('<II',package,cursor);cursor+=8
        if address==control==0: break
        length=control&0x7fffffff
        assert length>0 and address+length<=0x800000
        if control>>31: continue
        copies[address]=package[cursor:cursor+length];cursor+=length
    else: raise AssertionError('CB06 command bound')
    assert cursor==len(package)
    assert copies.pop(0xe0000)==(output/'application.v3bin').read_bytes()
    for r in resources: assert copies.pop(r['address'])==(assets/r['file']).read_bytes()
    assert not copies
    assert (output/'independent.bin').read_bytes()==package

    files={p.relative_to(output).as_posix():dict(bytes=p.stat().st_size,sha256=sha(p)) for p in output.rglob('*') if p.is_file() and p.name!='column-firmware-receipt.json'}
    tracked=subprocess.check_output(['git','ls-files'],cwd=ROOT,text=True).splitlines()
    added=subprocess.check_output(['git','ls-files','--others','--exclude-standard'],cwd=ROOT,text=True).splitlines()
    source={p:sha(ROOT/p) for p in sorted(set(tracked+added)) if (ROOT/p).is_file()}
    receipt=dict(schema=1,status='firmware-host-qualified; hardware matching pending',hardware_profile=expected['hardware_profile'],layout_identity=expected['layout_identity'],expected_gpu_export_sha256=expected['gpu_export_sha256'],hardware=None,contract_sha256=sha(contract),ranges=ranges,framebuffer=dict(visible_bytes=192000,physical_bytes=204800,reserved_bytes=262144,physical_padding_bytes=12800,reserved_slack_bytes=57344,clear_sectors=1600,capture='Capture all 204800 physical bytes, including padding. Remaining 57344-byte reserved tail is not CLEAR-owned; capture as preimage/guard if referenced.'),oracle=dict(visible_pixels=visible,padding_pixels=padding,texture_decodes=texels,meshlets=24,triangles=1141,ui_abi='unchanged'),files=files,source_sha256=source,source_head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),source_dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT,text=True).strip()),boundary='Software/CB06/static assets only; no RTL, device, physical timing or framebuffer image qualification. Layout owner must re-export actual hardware configuration, compare, refit/audit as necessary before programming.')
    (output/'column-firmware-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
    print(json.dumps({k:v for k,v in receipt.items() if k not in ('files','source_sha256','ranges')}))
    return receipt


if __name__=='__main__':
    parser=argparse.ArgumentParser(__doc__)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--contract',type=Path,required=True)
    args=parser.parse_args()
    audit(args.output,args.contract)
