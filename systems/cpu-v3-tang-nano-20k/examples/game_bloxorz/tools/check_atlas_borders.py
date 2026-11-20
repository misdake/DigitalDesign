"""Independent packed-ring/UV guards and all-level camera density analysis."""
import argparse
import json
import math
from pathlib import Path
import struct
import atlas_borders as a
import assets
from gpu_texture_layout import level_offset

def taps(uv,size):
    xy=[((v*size)>>8)-128 for v in uv]
    x,y=[v>>8 for v in xy];fx,fy=[v&255 for v in xy]
    bottom=511*fy>>8;top=511-bottom;rt=top*fx>>8;rb=bottom*fx>>8
    return [(x+(i&1),y+(i>>1),w) for i,w in enumerate([top-rt,rt,bottom-rb,rb]) if w]

def check(output):
    metadata=json.loads((output/'assets.json').read_text())
    a.LOG2=metadata['main_atlas']['size'].bit_length()-1
    assets.M.SIZE=metadata['marker_atlas']['size']
    checks=[]
    for name,size,rects in [('texture',1<<a.LOG2,[(n,x,y,e,max(1,(1<<a.LOG2)//64)) for n,x,y,e in a.regions()]),
                           ('markers',assets.M.SIZE,[(f'marker-{i}',*assets.M.slot_rect(i)) for i in range(60)])]:
        binary=(output/f'{name}.bin').read_bytes();assert len(binary)==size*size*2
        exponent=size.bit_length()-1
        def pixel(x,y):return struct.unpack_from('<H',binary,level_offset(exponent,x,y))[0]
        for label,x,y,edge,gutter in rects:
            for py in range(y,y+edge):
                for px in range(x,x+edge):
                    assert pixel(px,py)==pixel(min(x+edge-gutter-1,max(x+gutter,px)),min(y+edge-gutter-1,max(y+gutter,py)))
            low=[round((v+gutter+.5)*65535/size) for v in (x,y)]
            high=[round((v+edge-gutter-.5)*65535/size) for v in (x,y)]
            # Full rectangular UV range is monotone. Include one Q16 unit of
            # interpolation/quantization guard around all four extrema.
            for u in (low[0]-1,low[0],high[0],high[0]+1):
                for v in (low[1]-1,low[1],high[1],high[1]+1):
                    assert all(x<=tx<x+edge and y<=ty<y+edge for tx,ty,w in taps((u,v),size))
            checks.append(dict(atlas=name,slot=label,edge=edge,gutter=gutter,content=edge-2*gutter,guard_taps_inside=True))
    # Original levels/camera formula, no GPU-derived geometry or image-size guess.
    cameras=[]
    for number,level in enumerate(assets.B.levels().values()):
        if number>=33:break
        width,depth=level['width'],level['depth'];cam=assets.W.camera(width,depth)
        back=[cam['eye'][i]-cam['target'][i] for i in range(3)];norm=math.sqrt(sum(v*v for v in back));back=[v/norm for v in back]
        right=[back[2],0,-back[0]];norm=math.sqrt(sum(v*v for v in right));right=[v/norm for v in right]
        up=[back[1]*right[2]-back[2]*right[1],back[2]*right[0]-back[0]*right[2],back[0]*right[1]-back[1]*right[0]]
        def project(p):
            delta=[p[i]-cam['eye'][i] for i in range(3)];z=-sum(delta[i]*back[i] for i in range(3));assert z>0
            return (200+cam['focal']*sum(delta[i]*right[i] for i in range(3))/z,120-cam['focal']*sum(delta[i]*up[i] for i in range(3))/z,z)
        lengths=[];points=[]
        for z in range(depth):
            for x in range(width):
                if level['cells'][z*width+x][0]==0:continue
                corners=[project((x-width/2+dx,0,z-depth/2+dz)) for dx,dz in [(0,0),(1,0),(1,1),(0,1)]]
                points+=corners
                lengths.extend(math.hypot(corners[i][0]-corners[(i+1)%4][0],corners[i][1]-corners[(i+1)%4][1]) for i in range(4))
        far,_=assets.W.conservative_far(level)
        assert max(p[2] for p in points)<far
        cameras.append(dict(level=number+1,width=width,depth=depth,camera_distance=cam['distance'],focal_pixels=cam['focal'],
                            tile_edge_pixel_range=[min(lengths),max(lengths)],
                            floor_content_texels_per_projected_edge_range=[(1<<a.LOG2)//4-2*max(1,(1<<a.LOG2)//64)] if not lengths else [(((1<<a.LOG2)//4-2*max(1,(1<<a.LOG2)//64))/max(lengths)),(((1<<a.LOG2)//4-2*max(1,(1<<a.LOG2)//64))/min(lengths))],
                            positive_camera_depth=True,within_archived_far=True))
    assert len(cameras)==33
    result=dict(packed_ring_and_quantized_UV_checks=checks,all33_camera_analysis=cameras,
                camera_scope='Independent floating projection of actual static tile dimensions. Animated/off-board bounds reuse existing conservative far qualification; scalar edge density does not bound foreshortened triangle short axis or temporal alias.',
                primary_footprint_evidence='Actual native per-quad LOD/tap distributions from atlas_trace, not image dimensions.',
                source=metadata['main_atlas']['sources'])
    (output/'atlas-qualification.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(dict(slots=len(checks),cameras=len(cameras),main_size=1<<a.LOG2,marker_size=assets.M.SIZE)))

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('output',type=Path);args=parser.parse_args();check(args.output)
