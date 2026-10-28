"""Frozen-input full-scene reference: direct rays, attributes and ideal lighting.

No RTL, ROM schedule, reduced triangle coefficients or reciprocal table is read.
Native texture storage and documented sampling/Final/ROP quantizers are decoded
explicitly. Ideal Blinn-Phong remains an approximation reference to the hardware
Lighting lookup, so reports separate geometric, texture, lighting and UI checks.
"""
from dataclasses import dataclass
import math
import struct

import numpy as np

from draw_oracle import signed,cross,dot
from layout import header_layout


def expand(words):
    words = np.asarray(words, dtype=np.int64)
    r, g, b = words >> 11, (words >> 5) & 63, words & 31
    return np.stack((r*8+r//4, g*4+g//16, b*8+b//4), axis=-1)


def pack(rgb):
    rgb = np.asarray(rgb, dtype=np.int64)
    q = (rgb*np.array([31,63,31])+127)//255
    return (q[...,0] << 11) | (q[...,1] << 5) | q[...,2]


def normalize(v):
    return v/np.maximum(np.linalg.norm(v, axis=-1, keepdims=True), 1e-30)


def nearest_even_div64(n):
    a = abs(n); q, r = divmod(a, 64)
    q += r > 32 or (r == 32 and q & 1)
    return -q if n < 0 else q


def texture_levels(capture, descriptor):
    n = (descriptor >> 16) & 15
    if n > 10 or ((descriptor >> 21) & 3) > 2:
        raise ValueError('Unsupported texture descriptor')
    column = header_layout(capture.header).abi == 2
    tile = 16 if column else 8
    base = (descriptor & 65535)*128; levels = {}; offset = 0
    for exponent in range(n+1) if descriptor & (1 << 20) else (n,):
        side = 1 << exponent; physical = max(tile, side); blocks = physical//tile
        words = np.frombuffer(capture.read(base+offset, physical*physical*2), dtype='<u2')
        blocks4 = words.reshape(blocks, blocks, tile, tile)
        # Row profile orders (ty,tx); column profile orders (tx,ty).
        axes = (1,2,0,3) if column else (0,2,1,3)
        plane = blocks4.transpose(axes).reshape(physical, physical)[:side,:side]
        levels[exponent] = expand(plane)
        offset += physical*physical*2
    return levels


def sample(levels, descriptor, uv, lod):
    """Documented integer tap weights/Color, independent of cache grouping."""
    n = (descriptor >> 16) & 15; filtering = (descriptor >> 21) & 3
    full = bool(descriptor & (1 << 20))
    lodq = np.floor(np.clip(lod, 0, n if full else 0)*8).astype(np.int64)
    fine = n-lodq//8
    coarse = np.maximum(0, fine-1)
    fractions = lodq & 7
    coarse_parent = np.array([0,64,128,192,256,319,383,447])[fractions]
    coarse_parent[(fine == 0) | (filtering != 2)] = 0
    uv16 = np.floor(uv*65536+.5).astype(np.int64) & 65535
    result = np.zeros((len(uv),3), dtype=np.int64)
    for exponent in range(n+1):
        if exponent not in levels:
            continue
        for layer, parent in ((fine, 511-coarse_parent), (coarse, coarse_parent)):
            active = (layer == exponent) & (parent != 0)
            if not np.any(active):
                continue
            size = 1 << exponent; coords = uv16[active]*size
            p = parent[active]; tex = levels[exponent]
            if filtering == 0:
                color = tex[(coords[:,1] >> 16) % size, (coords[:,0] >> 16) % size]
                result[active] += (color*p[:,None]+256) >> 9
                continue
            coords -= 32768
            xy = coords >> 16; frac = (coords & 65535) >> 8
            bottom = (p*frac[:,1]) >> 8; top = p-bottom
            tr = (top*frac[:,0]) >> 8; br = (bottom*frac[:,0]) >> 8
            weights = (top-tr, tr, bottom-br, br)
            taps = []
            for (dx,dy), weight in zip(((0,0),(1,0),(0,1),(1,1)), weights):
                color = tex[(xy[:,1]+dy) % size, (xy[:,0]+dx) % size]
                taps.append((color*weight[:,None]+256) >> 9)
            result[active] += ((taps[0]+taps[1]) & 255)+((taps[2]+taps[3]) & 255)
    return result & 255


def lighting(normal, x, y, global_words, unlit):
    mode = global_words[5] & 3
    ia, intensity = global_words[4] & 511, (global_words[4] >> 16) & 511
    if unlit or mode == 0:
        return np.full(len(x),256.), np.zeros(len(x))
    if mode == 1:
        return np.full(len(x),float(ia)), np.zeros(len(x))
    light = np.array([signed(global_words[1],16), signed(global_words[1] >> 16,16),
                      signed(global_words[2],16)])/16384
    scales = np.array([signed(global_words[2] >> 16,16), signed(global_words[3],16)])/16384
    k = signed(global_words[3] >> 16,16)/16384
    n = normalize(normal)
    nl = np.sum(n*light,axis=1)
    g = np.minimum(511, ia+intensity*np.maximum(0,nl))
    if mode != 3:
        return g,np.zeros(len(x))
    ray = np.column_stack(((2*x-399)/400*scales[0], (239-2*y)/240*scales[1], np.full(len(x),k)))
    half = normalize(light+normalize(ray))
    nh = np.clip(np.sum(n*half,axis=1),0,1)
    code = (global_words[5] >> 2) & 31
    if code > 16:
        raise ValueError('Unsupported shininess')
    power = (4,5,6,7,8,10,12,14,16,20,24,28,32,40,48,56,64)[code]
    return g, intensity*(nh**power)*(nl > 0)


@dataclass
class SceneResult:
    color: object
    depth: object
    owner: object
    margin: object
    ambiguity: object
    groups: object
    ui: object
    stats: dict

    def compare(self, actual):
        color = np.asarray(actual['color']).reshape(240,400)
        depth = np.asarray(actual['depth']).reshape(240,400)
        expected_coverage = self.owner >= 0
        actual_coverage = depth != 65535
        stable = self.margin > 1
        # Both sides of a visible ownership boundary are uncertain, including
        # a background winner beside a foreground edge. No DUT owner is used.
        boundary = np.zeros((240,400),dtype=bool)
        for dy,dx in ((0,1),(1,0),(1,1),(1,-1)):
            other = np.roll(self.owner,(dy,dx),(0,1)); different = self.owner != other
            if dy: different[:dy,:] = False
            if dx > 0: different[:,:dx] = False
            if dx < 0: different[:,dx:] = False
            boundary |= different | np.roll(different,(-dy,-dx),(0,1))
        stable &= ~boundary & ~self.ambiguity
        depth_error = abs(depth-self.depth)
        rgb_error = np.max(abs(expand(color)-expand(self.color)),axis=-1)
        code_error = np.maximum.reduce([abs(((color >> shift)&mask)-((self.color >> shift)&mask))
                                        for shift,mask in ((11,31),(5,63),(0,31))])
        coverage_error = expected_coverage != actual_coverage
        report = dict(reference='Frozen commands/resources -> exact V windows, float64 direct rays, perspective attributes and ideal Blinn-Phong; documented texture/Final quantizers',
                      budgets=dict(depth_q8_8_codes=2, unlit_rgb565_channel_codes=1, texture_rgb8=24, lit_rgb8=16,
                                   texture_and_lighting_mean_rgb8=2,
                                   edge_pixels=1, depth_tie_codes=2),
                      stats=self.stats, logical_pixels=96000, stable_pixels=int(stable.sum()),
                      ui_pixels=int(self.ui.sum()), coverage_false_positive=int(np.sum(actual_coverage & ~expected_coverage)),
                      coverage_false_negative=int(np.sum(expected_coverage & ~actual_coverage)),
                      stable_coverage_errors=int(np.sum(stable & coverage_error)), groups={})
        passed = report['stable_coverage_errors'] == 0
        for group,name,budget in ((0,'background',0),(1,'unlit_rgb',8),(2,'texture',24),(3,'lighting',16)):
            mask = stable & (self.groups == group) & ~self.ui
            errors = rgb_error[mask]; zs = depth_error[mask]
            outside = code_error[mask]>1 if group==1 else errors>budget
            report['groups'][name] = dict(pixels=int(mask.sum()), color_max=int(errors.max()) if errors.size else None,
                color_mean=float(errors.mean()) if errors.size else None, color_p99=float(np.quantile(errors,.99)) if errors.size else None,
                depth_max=int(zs.max()) if zs.size else None, color_outside_budget=int(np.sum(outside)),
                depth_outside_budget=int(np.sum(zs > (0 if group == 0 else 2))))
            passed &= not report['groups'][name]['color_outside_budget'] and not report['groups'][name]['depth_outside_budget']
            if group in (2,3) and errors.size:
                passed &= float(errors.mean())<=2
        ui_error = self.ui & (color != self.color)
        report['ui_color_exact_errors'] = int(ui_error.sum())
        report['ui_depth_outside_budget'] = int(np.sum(self.ui & (depth_error > 2)))
        passed &= not report['ui_color_exact_errors'] and not report['ui_depth_outside_budget']
        report['all_pixels_depth_max'] = int(depth_error.max())
        report['all_pixels_color_max_rgb8'] = int(rgb_error.max())
        report['all_pixels_color_p99_rgb8'] = float(np.quantile(rgb_error,.99))
        report['omitted_edge_or_tie_pixels'] = int((~stable & ~self.ui).sum())
        per_pixel_budget = np.choose(self.groups,[0,9,24,16])
        color_bad = np.where(self.groups==1,code_error>1,rgb_error>per_pixel_budget)
        report['outside_stable_region_deviations'] = dict(
            color_outside_group_budget=int(np.sum(~stable & ~self.ui & color_bad)),
            depth_outside_budget=int(np.sum(~stable & (depth_error>2))),
            examples=[dict(x=int(x),y=int(y),color_error_rgb8=int(rgb_error[y,x]),
                           depth_error=int(depth_error[y,x]),ideal_edge_margin=float(self.margin[y,x]),
                           edge_or_depth_tie=bool(self.ambiguity[y,x]))
                      for y,x in np.argwhere(~stable & ~self.ui & (color_bad | (depth_error>2)))[:12]])
        bad = (stable & ((depth_error > 2) | coverage_error | color_bad)) | ui_error
        report['examples'] = [dict(x=int(x),y=int(y),expected_color=int(self.color[y,x]),actual_color=int(color[y,x]),
                                  expected_depth=int(self.depth[y,x]),actual_depth=int(depth[y,x])) for y,x in np.argwhere(bad)[:12]]
        report['pass'] = bool(passed)
        return report


def render(capture):
    h = capture.header; layout = header_layout(h)
    scratch = bytearray(4096); pages = {}; context = None; material = None; target = False
    color = np.array(layout.words(capture.read(h['color_base'],layout.plane_bytes)),dtype=np.int64).reshape(240,400)
    depth = np.array(layout.words(capture.read(h['depth_base'],layout.plane_bytes)),dtype=np.int64).reshape(240,400)
    owner = np.full((240,400),-1,dtype=np.int64); margin = np.full((240,400),np.inf)
    ambiguity = np.zeros((240,400),dtype=bool); groups = np.zeros((240,400),dtype=np.int8); ui = np.zeros((240,400),dtype=bool)
    stats = dict(commands=0,draws=0,triangles=0,culled=0,behind=0,degenerate=0,texture_draws=0,lit_draws=0,ui_commands=0,glyphs=0,ui_writes=0,clears={})
    cache = {}; page_addresses = {}; stats['draw_records'] = []
    yy,xx = np.mgrid[:240,:400]
    rays = np.stack(((2*xx-399)/512,(2*yy-239)/512,np.ones_like(xx)),axis=-1)
    words = list(struct.iter_unpack('<I',capture.read(h['cb_base'],h['cb_bytes'])))
    if not words or words[-1][0] != 0xf0000000 or len(words)>16384:
        raise ValueError('Full-scene command stream needs bounded count and final END')
    for pc,(word,) in enumerate(words):
        opcode = word >> 28; stats['commands'] += 1
        if opcode==15 and pc != len(words)-1: raise ValueError('Early END in scene command stream')
        if opcode in (0,4,7,9,15):
            continue
        if opcode == 1:
            code = (word >> 1) & 3
            if code == 3: raise ValueError('Invalid DMA size')
            at = ((word >> 3) & 127)*32; count = 32 << code
            if at+count>4096: raise ValueError('DMA outside local storage')
            scratch[at:at+count] = capture.read(((word >> 10) & 0x3ffff)*32,count)
        elif opcode == 2:
            address = ((word >> 12) & 65535)*128
            page_addresses[word & 1] = address
            pages[word & 1] = (capture.read(address,32),capture.read(address+128,(((word >> 1) & 7)+1)*128))
        elif opcode in (5,6,10):
            at = (word & 127)*32
            if opcode == 5: context = struct.unpack_from('<16I',scratch,at)
            elif opcode == 6:
                material = struct.unpack_from('<8I',scratch,at)
                if material[2] & 31 != 9: raise ValueError('Scene reference requires opaque LESS and depth writes')
            else:
                value, = struct.unpack_from('<I',scratch,at)
                if (value & 0x3fff)*512!=h['color_base'] or ((value >> 14) & 0x3fff)*512!=h['depth_base']:
                    raise ValueError('Scene target/header mismatch')
                target = True
        elif opcode == 8:
            if not target: raise ValueError('CLEAR before target')
            if stats['draws']: raise ValueError('Scene reference requires plane clears before DRAW')
            if word & 65536:
                if word & 65535 != 65535: raise ValueError('Scene reference needs depth clear 0xffff')
                depth.fill(65535); owner.fill(-1)
            else: color.fill(word & 65535)
            stats['clears']['depth' if word & 65536 else 'color']=word & 65535
        elif opcode == 11:
            stats['ui_commands'] += 1
            descriptor, = struct.unpack_from('<Q',scratch,(word & 127)*32)
            grid,atlas = (descriptor & 65535)*128, ((descriptor >> 16) & 127)*65536
            first,last = (((word >> 7)&1023),((word >> 17)&1023)) if word & (1 << 27) else (0,749)
            if not 0<=first<=last<750: raise ValueError('UI range outside the fixed grid')
            for cell in range(first,last+1):
                char = capture.read(grid+cell,1)[0]
                if char==255: continue
                stats['glyphs'] += 1; glyph = capture.read(atlas+char*256,256)
                for gy in range(16):
                    for gx in range(8):
                        tx = gx ^ (2 if gy & 1 else 0)
                        texel, = struct.unpack_from('<H',glyph,gy*16+tx*2)
                        if texel & 1:
                            x,y = cell%50*8+gx,cell//50*16+gy
                            r,g,b = texel >> 11,(texel >> 6)&31,(texel >> 1)&31
                            color[y,x] = (r << 11)|(((g << 1)|(g >> 4)) << 5)|b
                            ui[y,x] = True; stats['ui_writes'] += 1
        elif opcode == 3:
            if not target or context is None or material is None or (word & 1) not in pages:
                raise ValueError('Incomplete DRAW bindings')
            stats['draws'] += 1
            if stats['draws']>24: raise ValueError('Complete-scene oracle is bounded to 24 meshlets')
            meta,payload = pages[word & 1]; q0,q1,q2,q3 = struct.unpack('<4Q',meta)
            vc,tc = q0 & 127,(q0 >> 8)&127
            if not 1<=vc<=64 or not 1<=tc<=96: raise ValueError('Invalid meshlet counts')
            matrix = struct.unpack_from('<32i',scratch,((q0 >> 16)&63)*128)
            tint = np.array([(matrix[22] >> shift)&255 for shift in (0,8,16)])
            if (matrix[22] >> 24)&255 != 255: raise ValueError('Translucent mesh not supported')
            rgb = bool(q1 & 16); unlit = bool(material[2] & 32)
            stats['texture_draws'] += not rgb; stats['lit_draws'] += not unlit
            stats['draw_records'].append(dict(record_address=page_addresses[word & 1], vertices=vc,
                                              triangles=tc, rgb=rgb, unlit=unlit))
            positions=[]; raw_positions=[]; normals=[]; attributes=[]
            origins = (signed(q2,32),signed(q2 >> 32,32),signed(q3,32))
            for v in range(vc):
                p,n,a = struct.unpack_from('<3I',payload,v*12)
                xyz = [signed(origins[k]+(((p >> (10*k))&1023) << ((q3 >> 32)&31)),32) for k in range(3)]
                raw = [signed((sum(xyz[k]*matrix[4*j+k] for k in range(3))+(matrix[4*j+3] << 16)) >> 16,32) for j in range(3)]
                if any(not -(1 << 25)<=v<(1 << 25) for v in raw): raise ValueError('V position outside signed26')
                raw_positions.append(raw)
                positions.append(np.array(raw)/65536)
                nn = [signed(n >> (10*k),10)*128 for k in range(3)]
                nv = [signed(sum(nn[k]*matrix[12+3*j+k] for k in range(3)) >> 16,32) for j in range(3)]
                q = [signed(v,18) if q3 & (1 << 40) else 4*signed(v >> 2,16) for v in nv]
                normals.append([max(-2048,min(2047,nearest_even_div64(v))) for v in q])
                attributes.append([(a >> (8*k))&255 for k in range(3)] if rgb else [(a & 65535)/65536,(a >> 16)/65536,0])
            positions,normals,attributes = map(np.array,(positions,normals,attributes))
            if not rgb and material[0] not in cache: cache[material[0]] = texture_levels(capture,material[0])
            for t in range(tc):
                pid=stats['triangles']; stats['triangles'] += 1
                if stats['triangles']>1141: raise ValueError('Complete-scene oracle is bounded to 1141 triangles')
                indices, = struct.unpack_from('<Q',payload,((q0 >> 24)&127)*8+(t//3)*8)
                index=[(indices >> ((t%3)*18+6*k))&63 for k in range(3)]
                if max(index)>=vc: raise ValueError('Triangle index outside V count')
                p = positions[index]
                if np.max(p[:,2])<=0: stats['behind'] += 1; continue
                raw=[raw_positions[j] for j in index]
                cofactors=[cross(raw[1],raw[2]),cross(raw[2],raw[0]),cross(raw[0],raw[1])]
                determinant=dot(raw[0],cofactors[0])
                if determinant==0: stats['degenerate'] += 1; continue
                if q0 & (1 << 31) and determinant<0: stats['culled'] += 1; continue
                inverse=np.array(cofactors,dtype=float)*(65536/determinant)
                if np.min(p[:,2])>0:
                    projected=p[:,:2]/p[:,2,None]*256+np.array([199.5,119.5])
                    x0,y0=np.maximum(0,np.floor(projected.min(axis=0)-2)).astype(int)
                    x1,y1=np.minimum([400,240],np.ceil(projected.max(axis=0)+3)).astype(int)
                else: x0,y0,x1,y1=0,0,400,240
                if x1<=x0 or y1<=y0: continue
                sl=np.s_[y0:y1,x0:x1]; rr=rays[sl]; cc=rr @ inverse.T; d=cc.sum(axis=-1)
                sign=1 if determinant>0 else -1
                inclusive=np.array([n[0]*sign>0 or (n[0]==0 and n[1]*sign>0) for n in cofactors])
                valid=np.all((cc>1e-12)|((abs(cc)<=1e-12)&inclusive),axis=-1)&(d>0)
                z=1/np.maximum(d,1e-30); zword=np.minimum(65536,np.floor(z*256)).astype(np.int64)
                valid &= zword <= (context[0] >> 8)
                distances=cc/np.maximum(np.linalg.norm(inverse[:,:2]*2/512,axis=1),1e-30)
                near=(distances.min(axis=-1)>=-1)&(distances.min(axis=-1)<=1)&(d>0)&(zword<depth[sl]+3)
                ambiguity[sl] |= near
                ambiguity[sl] |= valid & (owner[sl]>=0) & (abs(zword-depth[sl])<=2)
                take=valid&(zword<depth[sl])
                if not take.any(): continue
                weights=cc[take]/d[take,None]; uv_or_rgb=weights @ attributes[index]
                px,py=xx[sl][take],yy[sl][take]
                if rgb: source=np.floor(uv_or_rgb+.5).astype(np.int64)
                else:
                    ax,ay=px&~1,py&~1
                    ar=np.stack(((2*ax[:,None]+2*np.array([0,1,0,1])-399)/512,
                                 (2*ay[:,None]+2*np.array([0,0,1,1])-239)/512,np.ones((len(ax),4))),axis=-1)
                    ac=ar @ inverse.T; ad=ac.sum(axis=-1)
                    anchor=(np.all((ac>1e-12)|((abs(ac)<=1e-12)&inclusive),axis=-1)&(ad>0)).argmax(axis=1)
                    ac=ac[np.arange(len(ax)),anchor]; ad=ad[np.arange(len(ax)),anchor]
                    av=(ac/ad[:,None]) @ attributes[index,:2]
                    derivative=np.einsum('pia,ic->pac',attributes[index,:2][None,:,:]-av[:,None,:],inverse[:,:2]*2/512)/ad[:,None,None]
                    rho=np.max(abs(derivative),axis=(1,2))*(1 << ((material[0] >> 16)&15))*math.sqrt(2)
                    lod=np.log2(np.maximum(rho,1e-30))+signed(material[3],8)/16
                    source=sample(cache[material[0]],material[0],uv_or_rgb[:,:2],lod)
                normal=weights @ normals[index]
                g,hfactor=lighting(normal,px,py,context,unlit)
                product=source*tint; temp=product+128; base=(temp+(temp >> 8)) >> 8
                spec=np.array([(material[1] >> shift)&255 for shift in (0,8,16)])
                output=np.clip(np.floor(base*g[:,None]/256)+np.floor(spec*hfactor[:,None]/256),0,255).astype(np.int64)
                color[sl][take]=pack(output); depth[sl][take]=zword[take]; owner[sl][take]=pid
                margin[sl][take]=distances[take].min(axis=-1)
                groups[sl][take]=3 if not unlit else 1 if rgb else 2
        else:
            raise ValueError('Unsupported native command '+str(opcode))
    return SceneResult(color,depth,owner,margin,ambiguity,groups,ui,stats)
