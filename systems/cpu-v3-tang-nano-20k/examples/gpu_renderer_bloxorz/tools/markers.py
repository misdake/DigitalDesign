"""Deterministic RGB marker tiles: one 256-square base layer, no alpha/mips."""
import copy
import math
from PIL import Image, ImageDraw

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
    return x*2,y*2,edge*2,gutter


def slot_uv(slot):
    x,y,size,gutter=slot_rect(slot)
    low,high=gutter+.5,size-gutter-.5
    result=[(round((x+u)*65535/SIZE),round((y+v)*65535/SIZE)) for u,v in [(low,low),(high,low),(high,high),(low,high)]]
    for u,v in result:
        assert x<=math.floor(u*SIZE/65535-.5) and math.floor(u*SIZE/65535-.5)+1<x+size
        assert y<=math.floor(v*SIZE/65535-.5) and math.floor(v*SIZE/65535-.5)+1<y+size
    return result

def mapped(mesh,top,side,top_face=0):
    mesh=copy.deepcopy(mesh);mesh['color_mode']=0
    mesh['marker_slots']=dict(top=top,side=side,top_face=top_face)
    for face in range(len(mesh['vertices'])//4):
        uv=slot_uv(top if face==top_face else side)
        for k,(u,v) in enumerate(uv):mesh['vertices'][face*4+k][2]=u|(v<<16)
    return mesh

def prepare(system,web):
    atlas=Image.open(system/'examples/bloxorz/assets/atlas-low.png').convert('RGB')
    slots=[];soft=[];heavy=[];fragile=[];split=[]
    def floor(variant,color):
        u,v,w,h=web.uv_rect(variant)
        crop=atlas.crop((round(u*512),round((1-v-h)*512),round((u+w)*512),round((1-v)*512))).resize((96,96),Image.Resampling.BOX)
        crop.putdata([tuple((a*b+127)//255 for a,b in zip(rgb,color)) for rgb in crop.getdata()])
        return crop
    def tile(base,pattern=None,ink=(0,0,0)):
        index=len(slots);rx,ry,edge,gutter=slot_rect(index);inner=edge-2*gutter
        im=base.resize((inner,inner),Image.Resampling.BOX)
        mask=Image.new('L',im.size);draw=ImageDraw.Draw(mask)
        def xy(x,z):return ((x+.5)*inner,(z+.5)*inner)
        if pattern in ('soft','split'):
            radius=.15 if pattern=='soft' else .19
            draw.polygon([xy(math.cos(i*math.pi/4)*.25,math.sin(i*math.pi/4)*.25) for i in range(8)],fill=255)
            draw.polygon([xy(math.cos(i*math.pi/4)*radius,math.sin(i*math.pi/4)*radius) for i in range(8)],fill=0)
            if pattern=='split':
                for x in [-.16,.16]:draw.rectangle([xy(x-.065,-.065),xy(x+.065,.065)],fill=255)
        elif pattern=='fragile':
            for a in [math.pi/4,-math.pi/4]:
                c,s=math.cos(a),math.sin(a)
                draw.polygon([xy(c*x+s*z,-s*x+c*z) for x,z in [(-.02,-.31),(.02,-.31),(.02,.31),(-.02,.31)]],fill=255)
        elif pattern=='heavy':
            a=.13/.44;draw.rectangle([xy(-a,-a),xy(a,a)],fill=255)
        if pattern:im=Image.composite(Image.new('RGB',im.size,tuple(ink)),im,mask)
        expanded=Image.new('RGB',(edge,edge))
        expanded.putdata([im.getpixel((min(inner-1,max(0,x-gutter)),min(inner-1,max(0,y-gutter)))) for y in range(edge) for x in range(edge)])
        slots.append(expanded)
        assert all(expanded.getpixel((x,y))==expanded.getpixel((min(edge-gutter-1,max(gutter,x)),min(edge-gutter-1,max(gutter,y)))) for y in range(edge) for x in range(edge))
        return index
    colors=[0x38bdf8,0xfb923c,0x4ade80,0xa78bfa,0xfacc15,0xf472b6,0x22d3ee,0xf87171]
    palettes=[web.palette(c) for c in colors]
    for channel,palette in enumerate(palettes):
        for active in range(2):
            background=floor(0,palette[active]);top=tile(background,'soft',palette[3]);side=tile(background)
            soft.append(mapped(web.unit_tile(0),top,side))
    for channel,palette in enumerate(palettes):
        background=Image.new('RGB',(96,96),tuple(palette[0]));top=tile(background,'heavy',palette[3]);side=tile(background)
        heavy.append(mapped(web.heavy_base(),top,side,2))
    for variant in range(4):
        background=floor(variant,(176,141,87));top=tile(background,'fragile',(63,42,18));side=tile(background)
        fragile.append(mapped(web.unit_tile(variant),top,side))
    for variant in range(4):
        top=tile(floor(variant,(139,92,246)),'split',(237,233,254))
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
    binary=pack_levels({8:pixels});assert len(binary)==131072
    return binary,image,[soft,heavy,fragile,split]
