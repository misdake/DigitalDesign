"""Freeze native16px Vonwaon ASCII for the existing8x16 binary UI format."""
import argparse
import hashlib
import json
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

def extract(ttf):
    font=ImageFont.truetype(str(ttf),16)
    assert font.getmetrics()==(14,2)
    rows={}
    for code in range(32,127):
        assert font.getlength(chr(code))==8
        im=Image.new('1',(8,16))
        ImageDraw.Draw(im).text((0,14),chr(code),font=font,anchor='ls',fill=1)
        rows[code]=[sum((128>>x) for x in range(8) if im.getpixel((x,y))) for y in range(16)]
    return rows

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--ttf',type=Path,required=True);p.add_argument('--bitmap',type=Path,required=True);p.add_argument('--provenance',type=Path,required=True);a=p.parse_args()
    digest=hashlib.sha256(a.ttf.read_bytes()).hexdigest();assert digest=='0fc24cfa3c134fe3482962be69a57bd419fb9ceec1b6dbb7fd3f7a054087f59a'
    rows=extract(a.ttf);a.bitmap.write_text('# VonwaonBitmap16pxLite1.02 ASCII; native16px, baseline14, advance8, binary pixels.\n'+''.join(str(k)+' '+ ' '.join(f'{r:02x}' for r in v)+'\n' for k,v in rows.items()))
    a.provenance.write_text(json.dumps(dict(family='VonwaonBitmap16pxLite',version='1.02',author='Haoyu Qiu',source_sha256=digest,source_bytes=a.ttf.stat().st_size,source_url='https://timothyqiu.itch.io/vonwaon-bitmap',declared_license='CC0 1.0 on the author page',upstream_note='The author also acknowledges uncertainty in upstream HZK/ASC mould rights. This records the declaration, not a claim that all upstream rights are resolved.',usage='User-provided font for the requested local HUD; only native ASCII bitmap glyphs frozen, original TTF unchanged.',pixel_size=16,cell=[8,16],baseline=14,advance=8,render_mode='Pillow/FreeType1-bit at16px; no resize, smoothing, grayscale or added line spacing',glyph_range=[32,126],atlas_format='Native UI65536-byte binary atlas; PNG decodes the same actual lookup into16x16 glyph cells'),indent=2)+'\n')
    assert hashlib.sha256(a.ttf.read_bytes()).hexdigest()==digest
