"""Project-font labels and app-owned diagnostic icons in a native UI atlas."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import struct
import sys

EXAMPLE = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(EXAMPLE.parents[3] / "systems/cpu-v3-tang-nano-20k/tools"))
from gpu_layout import validate_asset_range, sectors
import ui_font
# IDs 96..251 are diagnostic icons, so only printable ASCII 32..95 is text here.
FONT = {chr(code): rows for code, rows in ui_font.load_rows().items() if code < 96}
LABELS = [
    (6,0,'DS2 YELLOW RAW / GREEN OUTPUT'), (1,1,'LINK:'), (22,1,'ID:'), (32,1,'SAMPLES:'),
    (1,2,'BUTTONS:'), (17,2,'PRESS:'), (33,2,'RELEASE:'),
    (5,3,'LX:'), (14,3,'LY:'), (31,3,'RX:'), (40,3,'RY:'),
    (6,4,'LEFT'), (32,4,'RIGHT'),
    (1,9,'L'), (13,9,'P'), (27,9,'R'), (39,9,'P'),
    (1,10,'DZ:'), (8,10,'U/D  SEL:CENTER  START:RESET'),
]
BUTTONS = ['SEL','L3','R3','START','UP','RIGHT','DOWN','LEFT',
           'L2','R2','L1','R1','TRI','CIR','CROSS','SQR']
LABELS += [(1+(i%8)*6, 11+(i//8)*2, name) for i,name in enumerate(BUTTONS)]

def rgba(r,g,b):
    return (r<<11)|(g<<6)|(b<<1)|1

def pack(output):
    output.mkdir(parents=True, exist_ok=True)
    pixels = [[[0]*8 for _ in range(16)] for _ in range(256)]
    for char, rows in FONT.items():
        for y, bits in enumerate(rows):
            for x in range(8):
                if bits & (128 >> x):
                    pixels[ord(char)][y][x] = 0xffff
    for id, color, solid in [(1,rgba(8,10,12),False),(2,rgba(8,31,12),True)]:
        for y in range(4,12):
            for x in range(8):
                if solid or x in (0,7) or y in (4,11):
                    pixels[id][y][x] = color
    # Two colors share32 sub-cell positions. Components map symmetrically
    # to even pixel coordinates, with room for a2x2 marker inside the ring.
    for base,color in [(96,rgba(31,25,0)),(128,rgba(8,31,12))]:
        for phase in range(32):
            x,y=(phase%4)*2,(phase//4)*2
            for yy in (y,y+1):
                for xx in (x,x+1):
                    pixels[base+phase][yy][xx]=color
    def panel(x,y,deadzone):
        radius=math.hypot(x-32,y-32)
        color=rgba(1,3,5)
        if radius<=30:
            color=rgba(2,4,7)
            if deadzone and radius<=28*deadzone/256:
                color=rgba(5,7,16)
            if x==32 or y==32:
                color=rgba(9,12,15)
        if deadzone and abs(radius-28*deadzone/256)<=0.65:
            color=rgba(8,12,24)
        if abs(radius-30)<=0.65:
            color=rgba(6,20,24)
        return color
    for row in range(4):
        for col in range(8):
            for y in range(16):
                for x in range(8):
                    pixels[160+row*8+col][y][x]=panel(col*8+x,row*16+y,0)
    # Only the four central tiles change for inner dead zones4..60/256.
    for level in range(1,16):
        for tile in range(4):
            col,row=3+tile%2,1+tile//2
            for y in range(16):
                for x in range(8):
                    pixels[192+(level-1)*4+tile][y][x]=panel(col*8+x,row*16+y,level*4)
    atlas = bytearray(65536)
    for id in range(255):
        for y in range(16):
            for x in range(8):
                struct.pack_into('<H',atlas,id*256+(y*8+(x^((y&1)*2)))*2,pixels[id][y][x])
    # Independent inverse check covers every pixel and the reserved skip entry.
    for id in range(256):
        for y in range(16):
            for x in range(8):
                assert struct.unpack_from('<H',atlas,id*256+(y*8+(x^((y&1)*2)))*2)[0] == pixels[id][y][x]
    assert atlas[255*256:] == bytes(256)
    ids=[];calls=[]
    for x,y,text in LABELS:
        assert 0<=x<x+len(text)<=50 and 0<=y<15 and set(text)<=FONT.keys()
        at=len(ids);ids.extend(map(ord,text))
        calls.append(f'gpu_pass_ui_write(ui,{y*50+x},unsafe{{DS2_LABELS.as_ptr().add({at}).as_u16_array()}},{len(text)})')
    source='use crate::dsl_rt::*;\nmod gpu_legacy;\nuse gpu_legacy::*;\n'
    atlas_base = sectors()['GPU_ASSET_FIRST_SECTOR'] * 128
    source+=f'pub const DS2_UI_ATLAS:u16={atlas_base//128};\n'
    source+=f'static DS2_LABELS:Buf<u16,{len(ids)}>=Buf::new([{",".join(map(str,ids))}]);\n'
    source+='pub fn ds2_ui_labels(ui:u16)->bool{\n'+ ' &&\n'.join(calls)+'\n}\n'
    (output/'ui_layout.rs').write_text(source)
    validate_asset_range(atlas_base, len(atlas))
    (output/'atlas.bin').write_bytes(atlas)
    manifest={'source_root':'repository','resources':[{'file':'atlas.bin','address':atlas_base,'bytes':len(atlas),
                            'sha256':hashlib.sha256(atlas).hexdigest()}],
              'labels':[{'x':x,'y':y,'text':t} for x,y,t in LABELS],
              'font_ids':sorted(map(ord,FONT)), 'buttons':BUTTONS,
              'sources':{p.relative_to(EXAMPLE.parents[3]).as_posix():hashlib.sha256(p.read_bytes()).hexdigest()
                         for p in [Path(__file__), *ui_font.SOURCES]},
              'source_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
    (output/'assets.json').write_text(json.dumps(manifest,indent=2)+'\n')
    print('DS2 UI: 64KiB native atlas, labels,16 indicators and two64px stick panels')

if __name__=='__main__':
    parser=argparse.ArgumentParser(__doc__)
    parser.add_argument('--output',type=Path,required=True)
    pack(parser.parse_args().output.resolve())
