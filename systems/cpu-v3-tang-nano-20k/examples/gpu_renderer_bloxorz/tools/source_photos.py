"""Original photo crop/tone, then one direct final-size Lanczos resample.

Crop/tone definitions derive from the user's BloxorzClone make-atlas.py.
That reference identifies the photographs as CC0 Poly Haven sources.
No prebuilt atlas, mip, RGB565 image or PNG preview is used as a source.
"""
import hashlib
from pathlib import Path
from PIL import Image, ImageEnhance, ImageOps

ROOT = Path(__file__).resolve().parents[1] / 'assets/source'
IDENTITIES = {
    'marble_01.jpg': 'b10dabea976d68baa976a0d4e8dac58789df8d26a1b50697f320106f1b00a229',
    'rusty_metal_sheet.jpg': 'ecfacdea466b43162b63867b419576b7d35d34ea37c96969cf05e5a07faeb705',
}
SLABS = [(695,25,165),(105,250,150),(340,470,150),(320,705,150)]

def photo(name):
    path=ROOT/name
    assert hashlib.sha256(path.read_bytes()).hexdigest()==IDENTITIES[name]
    image=Image.open(path).convert('RGB');assert image.size==(1024,1024)
    return image

def stone(variant, size):
    # The reference V-flip maps logical variants 0/1 to source crops 2/3.
    x,y,s=SLABS[(1-variant//2)*2+variant%2]
    g=ImageOps.grayscale(photo('marble_01.jpg').crop((x,y,x+s,y+s)))
    mean=sum(g.resize((32,32)).getdata())/1024
    g=g.point(lambda v:min(248,max(150,int(208+(v-mean)*.7))))
    data=g.load()
    for y in range(s):
        for x in range(s):
            d=min(x,y,s-1-x,s-1-y)
            if d<3:data[x,y]=max(0,data[x,y]-9*(3-d))
    return Image.merge('RGB',(g,g,g)).resize((size,size) if isinstance(size,int) else size,Image.Resampling.LANCZOS)

def metal(name,size):
    x,y,rotate,warm={'side-a':(512,0,0,1),'top':(0,512,0,1.08),'side-b':(512,512,90,.96)}[name]
    image=photo('rusty_metal_sheet.jpg').crop((x,y,x+512,y+512))
    if rotate:image=image.rotate(rotate)
    image=ImageEnhance.Color(ImageEnhance.Contrast(image).enhance(1.45)).enhance(1.35)
    r,g,b=image.split()
    channels=[c.point(lambda v,f=f:min(255,int(v*f*warm*.85))) for c,f in zip((r,g,b),(1.08,.95,.82))]
    return Image.merge('RGB',channels).resize((size,size),Image.Resampling.LANCZOS)

def provenance():
    return dict(original_directory='BloxorzClone/textures/src',
                original_builder_sha256='4083f8acafb7d7d5e46ebbecd2a9bbaeb4a99b0a3558b55c35319325e192707f',
                declared_license='CC0 as identified by original BloxorzClone builder',license_url='https://polyhaven.com/license',
                source_urls=['https://polyhaven.com/a/marble_01','https://polyhaven.com/a/rusty_metal_sheet'],
                images=[dict(file=n,dimensions=[1024,1024],sha256=v) for n,v in IDENTITIES.items()],
                chain='original JPEG crop/tone, one final LANCZOS, native-size procedural marks, edge extrusion, one RGB565 quantization, column packing',
                color_space='reference encoded RGB; no added transfer curve')
