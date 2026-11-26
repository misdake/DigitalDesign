"""Extract actual cross-cell sampler transactions and surviving screen pixels."""
import argparse
from collections import Counter
import json
import math
from pathlib import Path
import struct
import zipfile
from PIL import Image, ImageDraw
from check_atlas_probe import check
from opaque_atlas import unpack


def expand(word):
    r,g,b=word>>11,(word>>5)&63,word&31
    return [(r<<3)|(r>>2),(g<<2)|(g>>4),(b<<3)|(b>>2),255]


def pack(rgb):
    r,g,b=rgb
    return ((r*31+127)//255)<<11 | ((g*63+127)//255)<<5 | (b*31+127)//255


def display(word):
    return ((word>>11)*255//31,((word>>5)&63)*255//63,(word&31)*255//31)


def shade(taps, uniform):
    # Exact native unlit RGB565 path, including each tap's integer rounding.
    source=[sum((t['rgba'][c]*t['weight']+256)>>9 for t in taps)&255 for c in range(3)]
    result=[]
    for c,value in enumerate(source):
        product=((uniform>>(8*c))&255)*value+128
        result.append((product+(product>>8))>>8)
    return result


def extract(snapshot, frames, atlas, source, reference, output):
    if output.exists():
        raise ValueError('Use a fresh output directory')
    footprints=check(snapshot,frames,atlas,source)
    lookup={(c['frame'],c['meshlet']):(c['group'],c['texture']) for c in footprints['cases']}
    with zipfile.ZipFile(atlas) as z:
        scene=json.loads(z.read('scene.json'))
        audit=scene['opaque_atlas']
        texture=z.read(next(m['texture'] for m in scene['materials'] if not m.get('blend') and m.get('texture')))
    levels=unpack(texture,audit['side'])
    atlas_exponent=audit['side'].bit_length()-1
    with zipfile.ZipFile(source) as z:
        tails={name:expand(struct.unpack_from('<H',z.read(name))[0]) for name in audit['regions']}
    output.mkdir(parents=True)
    counts=Counter()
    visible=[]
    originals={}
    actuals={}
    corrected={}
    paths=sorted(frames.glob('frame-*.samples.json'))
    if len(paths)!=footprints['frames']:
        raise ValueError('Incomplete pixel sample capture')
    for path in paths:
        data=json.loads(path.read_text())
        frame=data['frame']
        minimum=min((atlas_exponent-int(math.log2(l['side'])) for c in footprints['cases'] if c['frame']==frame for l in c['levels']),default=atlas_exponent)
        if data['minimum_lod_eighths']>minimum*8:
            raise ValueError('Pixel probe omits a mip level known to cross cells')
        name=f'frame-{frame:03}'
        metadata=json.loads((frames/(name+'.json')).read_text())
        if (reference/(name+'.d16')).read_bytes()!=(frames/(name+'.d16')).read_bytes():
            raise ValueError('Reference and atlas depth differ')
        current=Image.open(frames/(name+'.ppm')).convert('RGB')
        original=Image.open(reference/(name+'.ppm')).convert('RGB')
        correction=current.copy()
        counts['recorded_samples']+=len(data['samples'])
        for sample in data['samples']:
            if sample['target']!=metadata['color'] or sample['descriptor']&(1<<24):
                continue
            intended=lookup.get((frame,sample['meshlet']))
            if intended is None:
                continue
            group,source_name=intended
            region=audit['regions'][source_name]
            fixed=[]
            crossings=[]
            for at,tap in enumerate(sample['taps']):
                level=tap['level']
                side=1<<level
                scale=audit['side']//side
                cell=region['cell']//scale
                changed=dict(tap)
                if cell==0:
                    changed['rgba']=tails[source_name]
                    crossings.append(dict(tap=at,reason='mip merges source cells',**tap))
                else:
                    lo=[region['x']//scale,region['y']//scale]
                    hi=[v+cell-1 for v in lo]
                    clamped=[min(b,max(a,v)) for v,a,b in zip(tap['xy'],lo,hi)]
                    if clamped!=tap['xy']:
                        changed['rgba']=expand(levels[level][clamped[1]*side+clamped[0]])
                        crossings.append(dict(tap=at,reason='weighted tap outside source cell',
                                              own_cell_min=lo,own_cell_max=hi,own_xy=clamped,**tap))
                fixed.append(changed)
            if not crossings:
                continue
            counts['crossed_samples']+=1
            counts['crossed_depth_passed']+=sample['depth_passed']
            if not sample['visible']:
                continue
            counts['surviving_opaque_crossed_pixels']+=1
            if sample['overlaid']:
                counts['overlaid_crossed_pixels']+=1
                continue
            counts['unobscured_crossed_pixels']+=1
            if not sample['unlit']:
                raise ValueError('Diagnostic reconstruction requires the native unlit path')
            reconstructed=shade(sample['taps'],sample['uniform_color'])
            if reconstructed!=sample['shaded'][:3]:
                raise ValueError('Recorded tap colours do not reproduce the shaded pixel')
            xy=tuple(sample['xy'])
            word=pack(reconstructed)
            if pack(current.getpixel(xy))!=word:
                raise ValueError('Visible probe owner does not match the final framebuffer')
            own_word=pack(shade(fixed,sample['uniform_color']))
            own_rgb=display(own_word)
            actual_rgb=current.getpixel(xy)
            delta=max(abs(a-b) for a,b in zip(actual_rgb,own_rgb))
            counts['unobscured_colour_affected_pixels']+=delta!=0
            correction.putpixel(xy,own_rgb)
            visible.append(dict(frame=frame,xy=sample['xy'],group=group,texture=source_name,
                lod=sample['lod_eighths']/8,meshlet=sample['meshlet'],uv_q16=sample['uv_q16'],
                actual_rgb=actual_rgb,own_cell_rgb=own_rgb,reference_rgb=original.getpixel(xy),
                isolated_crossing_delta=delta,shaded=sample['shaded'],uniform_color=sample['uniform_color'],
                crossings=crossings,taps=sample['taps']))
        originals[frame]=original
        actuals[frame]=current
        corrected[frame]=correction
    candidates=sorted((r for r in visible if r['isolated_crossing_delta']),key=lambda r:r['isolated_crossing_delta'])
    selected=[]
    # Include the largest visible merged-mip error, a different source's boundary
    # crossing, and a typical sample. Never substitute a hidden fragment.
    merged=[r for r in candidates if any(c['reason']=='mip merges source cells' for c in r['crossings'])]
    if merged:
        selected.append(merged[-1])
    boundary=[r for r in candidates if r['texture'] not in {s['texture'] for s in selected}
              and any(c['reason']=='weighted tap outside source cell' for c in r['crossings'])]
    if boundary:
        selected.append(boundary[-1])
    for percentile in ([0.5] if len(selected)>1 else [0.95,0.5]):
        pool=[r for r in candidates if r['texture'] not in {s['texture'] for s in selected}]
        if pool:
            selected.append(pool[min(len(pool)-1,int((len(pool)-1)*percentile))])
    sheet=Image.new('RGB',(1068,294*max(1,len(selected))),'#181b20')
    draw=ImageDraw.Draw(sheet)
    for row,sample in enumerate(selected):
        frame=sample['frame'];x,y=sample['xy'];top=row*294
        caption=f"Frame {frame} | {Path(sample['texture']).stem} | pixel ({x},{y}) | LOD {sample['lod']:.3f} | crossing-only max RGB delta {sample['isolated_crossing_delta']}"
        draw.text((6,top+4),caption,fill='white')
        context=actuals[frame].copy()
        context_draw=ImageDraw.Draw(context)
        x0=max(0,min(376,x-12));y0=max(0,min(216,y-12))
        context_draw.rectangle((x0,y0,x0+23,y0+23),outline='#ffed00')
        context_draw.rectangle((x-1,y-1,x+1,y+1),outline='#ff4040')
        sheet.paste(context,(6,top+28))
        for col,(im,label) in enumerate([(originals[frame],'Original separate textures'),(actuals[frame],'Actual atlas'),(corrected[frame],'Only cross-cell taps corrected')]):
            left=414+col*216
            draw.text((left,top+28),label,fill='white')
            crop=im.crop((x0,y0,x0+24,y0+24)).resize((192,192),Image.Resampling.NEAREST)
            cd=ImageDraw.Draw(crop);cx=(x-x0)*8;cy=(y-y0)*8
            cd.rectangle((cx-1,cy-1,cx+8,cy+8),outline='#ffed00',width=1)
            sheet.paste(crop,(left,top+49))
    sheet.save(output/'examples.png')
    by_frame=Counter(r['frame'] for r in visible)
    if by_frame:
        frame=by_frame.most_common(1)[0][0]
        mask=actuals[frame].copy();md=ImageDraw.Draw(mask)
        for r in visible:
            if r['frame']==frame: md.point(tuple(r['xy']),fill='#ff2020')
        mask.save(output/f'frame-{frame:03}-crossed-pixels.png')
    result=dict(format='tankracer.crossed-pixels.v1',artifact_id=footprints['artifact_id'],
        frames=len(paths),counts=dict(counts),examples=selected,
        scope='Samples have nonzero weights. Selected examples are final unobscured opaque writers, checked against framebuffer RGB565. The diagnostic corrects only crossed taps, preserving UV, LOD and tint; it is not a changed renderer. Original-vs-atlas differences also include UV quantization.')
    (output/'summary.json').write_text(json.dumps(result,indent=2)+'\n')
    (output/'visible-pixels.json').write_text(json.dumps(visible,separators=(',',':')))
    print(json.dumps({k:v for k,v in result.items() if k!='examples'}))
    print(json.dumps([dict(frame=r['frame'],texture=r['texture'],xy=r['xy'],lod=r['lod'],delta=r['isolated_crossing_delta']) for r in selected]))


if __name__=='__main__':
    p=argparse.ArgumentParser(__doc__)
    p.add_argument('snapshot',type=Path);p.add_argument('frames',type=Path)
    p.add_argument('--atlas',type=Path,required=True);p.add_argument('--source',type=Path,required=True)
    p.add_argument('--reference',type=Path,required=True);p.add_argument('--out',type=Path,required=True)
    a=p.parse_args()
    extract(a.snapshot,a.frames,a.atlas,a.source,a.reference,a.out)
