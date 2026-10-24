"""Preview compiled CPU UI records; not a GPU RTL or board image comparison."""
import argparse
import json
from pathlib import Path
import struct
from PIL import Image

def preview(fixture, atlas, output):
    data=json.loads(fixture.read_text())
    font=atlas.read_bytes()
    c=data['clear']
    image=Image.new('RGB',(400,240),((c>>11)*255//31,((c>>5)&63)*255//63,(c&31)*255//31))
    for layer in data['layers']:
        for cell in range(layer['first'],layer['end']):
            id=layer['cells'][cell]
            if id==255:
                continue
            for y in range(16):
                for x in range(8):
                    value=struct.unpack_from('<H',font,id*256+(y*8+(x^((y&1)*2)))*2)[0]
                    if value&1:
                        image.putpixel(((cell%50)*8+x,(cell//50)*16+y),
                                       ((value>>11)*255//31,((value>>6)&31)*255//31,((value>>1)&31)*255//31))
    image.resize((800,480),Image.Resampling.NEAREST).save(output)

if __name__=='__main__':
    p=argparse.ArgumentParser(__doc__)
    p.add_argument('--fixture',required=True,type=Path)
    p.add_argument('--atlas',required=True,type=Path)
    p.add_argument('--output',required=True,type=Path)
    a=p.parse_args()
    preview(a.fixture,a.atlas,a.output)
