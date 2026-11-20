"""Render only captured native framebuffer words, without a host drawing path."""
import argparse
from pathlib import Path
from PIL import Image

parser = argparse.ArgumentParser(__doc__)
parser.add_argument('directory',type=Path)
args = parser.parse_args()
captures=sorted(args.directory.glob('actual[12].hex'))
assert captures
for source in captures:
    frame=int(source.stem[-1])
    values=[int(word,16) for word in source.read_text().splitlines()]
    assert len(values)==400*240
    rgb=[]
    for word in values:
        r,g,b=word>>11,(word>>5)&63,word&31
        rgb.append(((r<<3)|(r>>2),(g<<2)|(g>>4),(b<<3)|(b>>2)))
    image=Image.new('RGB',(400,240))
    image.putdata(rgb)
    image.save(args.directory/f'actual{frame}.png')
print(f'PASS actual framebuffer preview: {len(captures)} captured 400x240 frame(s)')
