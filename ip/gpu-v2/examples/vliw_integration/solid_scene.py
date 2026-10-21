"""Solid RGB room and complete supplied bunny, with real full-H9 lighting.

Only scene inputs, the testbench context and framebuffer presentation change.
All six inward room faces are submitted, including faces behind the camera.
"""
from pathlib import Path
import hashlib
import math
import re


def unit(v):
    length = math.sqrt(sum(x*x for x in v))
    assert length > 0
    return [x/length for x in v]


def cross(a, b):
    return [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]]


def camera(name):
    eyes = {'front': [0, .22, 2.8], 'side': [2.8, .22, 0], 'oblique': [1.98, .6, 1.98]}
    eye = eyes[name]
    target = [0, -.02, 0]
    forward = unit([b-a for a, b in zip(eye, target)])
    right = unit(cross([0, 1, 0], forward))
    up = cross(forward, right)
    position_basis = [right, [-x for x in up], forward]
    # Position H is right/down/forward. Lighting NDC/view rays use
    # right/up/back, so normals and light share that explicitly chosen basis.
    normal_basis = [right, up, [-x for x in forward]]
    focal = 200
    h = [[x*(focal/256 if axis < 2 else 1) for x in row]
         for axis, row in enumerate(position_basis)]
    h = [row+[-sum(a*b for a, b in zip(row, eye))] for row in h]
    uniform = [round(x*65536) for row in h for x in row]
    uniform += [round(x*65536) for row in normal_basis for x in row]+[65536]+[0]*10
    assert len(uniform) == 32
    world_light = unit([-.45, .8, 1.1])
    shading = dict(light=[round(sum(a*b for a, b in zip(row, world_light))*16384)
                          for row in normal_basis], ambient=64, directional=192,
                   ray_scale=[-8192, -4915], ray_k=8192, shininess=12, specular=[160]*3)
    return dict(name=name, eye=eye, target=target, focal_pixels=focal,
                normal_basis=normal_basis, uniform=uniform, shading=shading)


def room():
    low, high = [-3, -.8125, -3], [3, 3.1875, 3]
    faces = [('left', 0, 1, [133, 141, 153]), ('right', 0, -1, [146, 134, 120]),
             ('floor', 1, 1, [115, 123, 134]), ('ceiling', 1, -1, [151, 151, 151]),
             ('back', 2, 1, [125, 142, 159]), ('front', 2, -1, [146, 142, 135])]
    batches = []
    for name, axis, sign, color in faces:
        u, v = (axis+1) % 3, (axis+2) % 3
        positions = []
        for y in range(5):
            for x in range(5):
                p = [0.0]*3
                p[axis] = low[axis] if sign > 0 else high[axis]
                p[u] = low[u]+(high[u]-low[u])*x/4
                p[v] = low[v]+(high[v]-low[v])*y/4
                positions.append([round(value*65536) for value in p])
        origin = [min(p[k] for p in positions) for k in range(3)]
        extent = max(max(p[k] for p in positions)-origin[k] for k in range(3))
        shift = max(0, extent.bit_length()-10)
        while round(extent/(1 << shift)) > 1023:
            shift += 1
        normal = (sign*511 & 1023) << (10*axis)
        attribute = sum(c << (8*i) for i, c in enumerate(color))
        vertices = []
        for p in positions:
            values = [round((p[k]-origin[k])/(1 << shift)) for k in range(3)]
            assert all(0 <= x <= 1023 for x in values)
            vertices.append([sum(x << (10*k) for k, x in enumerate(values)), normal, attribute])
        triangles = []
        for y in range(4):
            for x in range(4):
                a = y*5+x
                for triangle in ([a, a+1, a+6], [a, a+6, a+5]):
                    triangles.append(triangle if sign > 0 else triangle[::-1])
        batches.append(dict(name='room_'+name, group='box', vertices=vertices,
                            triangles=triangles, config=origin+[shift | 256], color=color))
    assert sum(len(b['triangles']) for b in batches) == 192
    return batches


def instances(bunny, ply, oracle, view):
    cam = camera(view)
    batches, model = bunny.mesh(ply)
    assert model['triangles'] == 948
    objects = room()
    color = [165, 85, 47]
    for i, batch in enumerate(batches):
        vertices = [row[:2]+[sum(c << (8*k) for k, c in enumerate(color))]
                    for row in batch['vertices']]
        objects.append(dict(name=f'bunny_full_{i}', group='bunny', vertices=vertices,
                            triangles=batch['triangles'], config=model['config'], color=color))
    result = []
    for i, obj in enumerate(objects):
        instance = dict(name=obj['name'], group=obj['group'], slot=i % 2,
            uniform=cam['uniform'], config=obj['config'], vertices=obj['vertices'],
            triangles=obj['triangles'], color_mode=1, cull_negative=1,
            seq_base=0, generation=i % 16, scale_s=0,
            cfg_tex_w_log2=6, cfg_tex_h_log2=6, cfg_max_mip=6)
        values = oracle.F.oracle(instance['uniform'], instance['config'], instance['vertices'])
        maximum = max(abs(oracle.signed(values[6*j+k], 32))
                      for j in range(len(instance['vertices'])) for k in range(3))
        while maximum >> instance['scale_s'] > 131071:
            instance['scale_s'] += 1
        assert instance['scale_s'] < 16
        result.append(instance)
    return result, dict(scene='solid_box_complete_bunny', camera=cam,
        box=dict(faces=6, triangles=192, meshlets=6, subdivisions_per_axis=4),
        bunny=model, submitted_triangles=1140,
        material=dict(route='RGB888 constant albedo per object/face; texture bypass',
                      bunny_rgb=color, specular=cam['shading']['specular'], shininess=32)), cam['shading']


def configure_bench(text, shading):
    changes = {'context_mode=2': 'context_mode=3',
               'context_code=8': f'context_code={shading["shininess"]}',
               'specular=0': f'specular=24\'h{sum(x << (8*k) for k, x in enumerate(shading["specular"])):06x}'}
    for old, new in changes.items():
        assert text.count(old) == 1, old
        text = text.replace(old, new)
    for name, value in zip(('light_x', 'light_y', 'light_z', 'ray_x', 'ray_y', 'ray_k'),
                           shading['light']+shading['ray_scale']+[shading['ray_k']]):
        text, count = re.subn(r'\b'+name+r'=-?\d+', name+'='+str(value), text)
        assert count == 1, name
    for name in ('ambient', 'directional'):
        text, count = re.subn(r'\b'+name+r'=\d+', f'{name}={shading[name]}', text)
        assert count == 1, name
    return text


def render(out, root):
    from PIL import Image, PngImagePlugin
    dump = bytes(int(x, 16) for x in (out/'memory.mem').read_text().splitlines())
    assert len(dump) == 400384
    text = (root/'systems/cpu-v3-tang-nano-20k/src/display.rs').read_text()
    lut = list(map(int, re.search(r'LINEAR6_TO_SRGB8: \[u8; 64\] = \[(.*?)\];', text, re.S)[1].replace('\n', '').split(',')[:-1]))
    assert len(lut) == 64
    raw, display = [], []
    visible = 0
    for y in range(240):
        for x in range(400):
            offset=((y//16)*25+x//16)*512+(y%16)*32+(x%16)*2
            word=int.from_bytes(dump[16384+offset:16384+offset+2], 'little')
            r, g, b = word >> 11, word >> 5 & 63, word & 31
            raw.append(((r << 3)|(r >> 2), (g << 2)|(g >> 4), (b << 3)|(b >> 2)))
            display.append((lut[(r << 1)|(r >> 4)], lut[g], lut[(b << 1)|(b >> 4)]))
            visible += int.from_bytes(dump[208384+offset:208384+offset+2], 'little') != 65535
    metadata=PngImagePlugin.PngInfo()
    metadata.add_text('Source', 'Verified RTL SDRAM readback after flush')
    metadata.add_text('MemorySHA256', hashlib.sha256((out/'memory.mem').read_bytes()).hexdigest())
    images={}
    for name, pixels in [('framebuffer-raw', raw), ('framebuffer-srgb', display)]:
        image=Image.new('RGB', (400, 240));image.putdata(pixels)
        for suffix, item in [('', image), ('-2x', image.resize((800, 480), Image.Resampling.NEAREST))]:
            path=out/(name+suffix+'.png');item.save(path, pnginfo=metadata)
            images[path.name]=hashlib.sha256(path.read_bytes()).hexdigest()
    return dict(images=images, unique_depth_pixels=visible,
                display='Existing Display LINEAR6_TO_SRGB8 lookup; raw RGB565 expansion also saved')
