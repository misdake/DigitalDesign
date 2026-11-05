"""Deterministic RGB marker tiles: one 256-square base layer, no alpha/mips."""
import copy
import math
from PIL import Image

SIZE=256

def slot_rect_128(slot):
    if slot<32:return ((slot%8)*16,(slot//8)*16,16,2)
    if slot<48:
        channel=(slot-32)//2
        return (channel*16,64,16,2) if slot%2==0 else (channel*4,80,4,1)
    if slot<56:
        variant=(slot-48)//2
        return (variant*24,88,24,2) if slot%2==0 else (variant*16,112,16,2)
    return (64+(slot-56)*16,112,16,2)

def slot_rect(slot):
    x,y,edge,gutter=slot_rect_128(slot)
    scale=SIZE//128
    return x*scale,y*scale,edge*scale,gutter


def slot_uv(slot):
    x,y,size,gutter=slot_rect(slot)
    low,high=gutter+.5,size-gutter-.5
    result=[(round((x+u)*65535/SIZE),round((y+v)*65535/SIZE)) for u,v in [(low,low),(high,low),(high,high),(low,high)]]
    for u,v in result:
        assert x<=math.floor(u*SIZE/65536-.5) and math.floor(u*SIZE/65536-.5)+1<x+size
        assert y<=math.floor(v*SIZE/65536-.5) and math.floor(v*SIZE/65536-.5)+1<y+size
    return result

def mapped(mesh,top,side,top_face=0):
    mesh=copy.deepcopy(mesh);mesh['color_mode']=0
    mesh['marker_slots']=dict(top=top,side=side,top_face=top_face)
    for face in range(len(mesh['vertices'])//4):
        uv=slot_uv(top if face==top_face else side)
        for k,(u,v) in enumerate(uv):mesh['vertices'][face*4+k][2]=u|(v<<16)
    return mesh


def glyph_mask(pattern,inner):
    """Pixel-centre coverage of the original world-space Three.js definitions.

    No inclusive integer polygon endpoints or resized low-resolution mask.
    Splitter is the reference ring scaled by .8, not a different thin ring.
    """
    def octagon(x,z,radius):
        return all(x*math.cos((i+.5)*math.pi/4)+z*math.sin((i+.5)*math.pi/4)<=radius*math.cos(math.pi/8) for i in range(8))
    def covered(x,z):
        if pattern in ('soft','split'):
            scale=1 if pattern=='soft' else .8
            ring=octagon(x,z,.25*scale) and not octagon(x,z,.15*scale)
            dots=pattern=='split' and abs(z)<=.065 and any(abs(x-d)<=.065 for d in (-.16,.16))
            return ring or dots
        if pattern=='heavy':return abs(x)<=.13/.44 and abs(z)<=.13/.44
        if pattern=='fragile':
            return any(abs(math.cos(a)*x-math.sin(a)*z)<=.33 and abs(math.sin(a)*x+math.cos(a)*z)<=.04 for a in (-math.pi/4,math.pi/4))
        return False
    values=[255 if covered((x+.5)/inner-.5,(y+.5)/inner-.5) else 0 for y in range(inner) for x in range(inner)]
    # All reference glyphs have both reflection symmetries. This also catches
    # stray endpoint pixels and half-pixel shifts before any photo is composited.
    assert all(values[y*inner+x]==values[y*inner+inner-1-x]==values[(inner-1-y)*inner+x] for y in range(inner) for x in range(inner))
    mask=Image.new('L',(inner,inner));mask.putdata(values)
    return mask

def prepare(system,web):
    import source_photos as source
    slots=[];soft=[];heavy=[];fragile=[];split=[];items=[]
    def floor(variant,color):
        def content(inner):
            crop=source.stone(variant,inner)
            crop.putdata([tuple((a*b+127)//255 for a,b in zip(rgb,color)) for rgb in crop.getdata()])
            return crop
        return content
    def tile(base,pattern=None,ink=(0,0,0),**identity):
        index=len(slots);rx,ry,edge,gutter=slot_rect(index);inner=edge-2*gutter
        im=base(inner) if callable(base) else Image.new('RGB',(inner,inner),base)
        mask=glyph_mask(pattern,inner)
        if pattern:im=Image.composite(Image.new('RGB',im.size,tuple(ink)),im,mask)
        expanded=Image.new('RGB',(edge,edge))
        expanded.putdata([im.getpixel((min(inner-1,max(0,x-gutter)),min(inner-1,max(0,y-gutter)))) for y in range(edge) for x in range(edge)])
        slots.append(expanded)
        items.append(dict(slot=index,pattern=pattern,rect=[rx,ry,edge,gutter],inner=inner,ink=list(ink),**identity))
        assert all(expanded.getpixel((x,y))==expanded.getpixel((min(edge-gutter-1,max(gutter,x)),min(edge-gutter-1,max(gutter,y)))) for y in range(edge) for x in range(edge))
        return index
    colors=[0x38bdf8,0xfb923c,0x4ade80,0xa78bfa,0xfacc15,0xf472b6,0x22d3ee,0xf87171]
    palettes=[web.palette(c) for c in colors]
    for channel,palette in enumerate(palettes):
        for active in range(2):
            background=floor(0,palette[active]);top=tile(background,'soft',palette[3],kind='soft',channel=channel,active=bool(active),surface='top');side=tile(background,kind='soft',channel=channel,active=bool(active),surface='side')
            soft.append(mapped(web.unit_tile(0),top,side))
    for channel,palette in enumerate(palettes):
        background=tuple(palette[0]);top=tile(background,'heavy',palette[3],kind='heavy',channel=channel,surface='top');side=tile(background,kind='heavy',channel=channel,surface='side')
        heavy.append(mapped(web.heavy_base(),top,side,2))
    for variant in range(4):
        background=floor(variant,(176,141,87));top=tile(background,'fragile',(63,42,18),kind='fragile',variant=variant,surface='top');side=tile(background,kind='fragile',variant=variant,surface='side')
        fragile.append(mapped(web.unit_tile(variant),top,side))
    for variant in range(4):
        top=tile(floor(variant,(139,92,246)),'split',(237,233,254),kind='split',variant=variant,surface='top')
        split.append(mapped(web.unit_floor_face(0,variant),top,top))
    for prefix,group in [('soft',soft),('heavy',heavy),('fragile',fragile),('split',split)]:
        for i,mesh in enumerate(group):mesh['name']=mesh['group']=f'marked-{prefix}-{i}'
    assert len(slots)==60
    image=Image.new('RGB',(SIZE,SIZE))
    for i,im in enumerate(slots):image.paste(im,slot_rect(i)[:2])
    import sys
    sys.path.insert(0,str(system/'tools'))
    from gpu_texture_layout import pack_levels
    pixels=[((r*31+127)//255<<11)|((g*63+127)//255<<5)|((b*31+127)//255) for r,g,b in image.getdata()]
    binary=pack_levels({SIZE.bit_length()-1:pixels});assert len(binary)==SIZE*SIZE*2
    return binary,image,[soft,heavy,fragile,split],items
