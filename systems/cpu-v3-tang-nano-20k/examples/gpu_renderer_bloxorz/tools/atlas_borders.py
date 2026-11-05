"""Base-only RGB atlas with independently extruded item edges.

RGB values stay in the source's encoded color space, as in the original producer.
No gamma conversion or whole-atlas downsampling follows the regional extrusion.
"""
from PIL import Image
import source_photos as source

LOG2=8


def regions():
    size=1<<LOG2;q=size//4;h=size//2
    return [(f'floor-{i}',(i%2)*q,(1-i//2)*q,q) for i in range(4)]+[
        ('side-a',h,0,h),('top',0,h,h),('side-b',h,h,h)]


def extrude(content, edge, gutter):
    inner = edge - 2 * gutter
    assert inner > 0 and content.size == (inner, inner)
    result = Image.new('RGB', (edge, edge))
    result.putdata([content.getpixel((min(inner - 1, max(0, x - gutter)),
                                     min(inner - 1, max(0, y - gutter))))
                    for y in range(edge) for x in range(edge)])
    return result


def base():
    """Resample each original photo crop once, then extrude at the final resolution."""
    size=1<<LOG2;gutter=max(1,size//64);image=Image.new('RGB',(size,size))
    for name,x,y,edge in regions():
        inner=edge-2*gutter
        content=source.stone(int(name[-1]),inner) if name.startswith('floor-') else source.metal(name,inner)
        image.paste(extrude(content,edge,gutter),(x,y))
    return image


def uv_rect(which):
    size=1<<LOG2;gutter=max(1,size//64)
    name=f'floor-{which}' if isinstance(which,int) else which
    _,x,y,edge=next(r for r in regions() if r[0]==name)
    return ((x+gutter+.5)/size,(size-y-edge+gutter+.5)/size,
            (edge-2*gutter-1)/size,(edge-2*gutter-1)/size)


def contract():
    size=1<<LOG2
    return dict(size=size,regions=[dict(name=n,x=x,y=y,edge=e,gutter=max(1,size//64),inner=e-2*max(1,size//64)) for n,x,y,e in regions()],
                encoded_color_space_unchanged=True,full_mip=False,mip_exponents=[LOG2],sources=source.provenance(),
                UV='first and last content texel centres, quantized UNORM16',
                base_only_tradeoff='isolated bilinear base; no mip minification, possible aliasing')
