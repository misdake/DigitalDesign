"""Mixed UV/unlit, RGB/unlit and solid RGB/full-light scene fixtures."""
from pathlib import Path
import math

import solid_scene


def texture_levels():
    """Prepared 64-square RGB565 tile asset and box-filtered complete mip chain."""
    from PIL import Image
    image = Image.open(Path(__file__).with_name('assets')/'room-tiles.png').convert('RGB')
    assert image.size == (64, 64)
    levels = {}
    for n in range(7):
        pixels = list(image.resize((1 << n, 1 << n), Image.Resampling.BOX).getdata())
        levels[n] = [round(r*31/255) << 11 | round(g*63/255) << 5 | round(b*31/255)
                     for r, g, b in pixels]
    return levels


def rgb_triangle(cam):
    import numpy as np
    matrix = np.array(cam['uniform'][:12]).reshape(3, 4)/65536
    screen = [(316, 181), (380, 221), (316, 221)]
    positions = []
    for x, y in screen:
        h = np.array([(2*x-399)/512, (2*y-239)/512, 1])*.75
        p = np.linalg.solve(matrix[:, :3], h-matrix[:, 3])
        positions.append([round(v*65536) for v in p])
    origin = [min(p[k] for p in positions) for k in range(3)]
    extent = max(max(p[k] for p in positions)-origin[k] for k in range(3))
    shift = max(0, extent.bit_length()-10)
    while round(extent/(1 << shift)) > 1023:
        shift += 1
    colors = [(255, 0, 0), (0, 255, 0), (0, 0, 255)]
    vertices = [[sum(round((p[k]-origin[k])/(1 << shift)) << (10*k) for k in range(3)),
                 511 << 20, sum(v << (8*k) for k, v in enumerate(color))]
                for p, color in zip(positions, colors)]
    return dict(name='rgb_corner', group='vertex_color', vertices=vertices,
                triangles=[[0, 1, 2]], config=origin+[shift | 256], color_mode=1, unlit=True)


def instances(bunny, ply, oracle, view):
    values, info, shading = solid_scene.instances(bunny, ply, oracle, view)
    room = [value for value in values if value['group'] == 'box']
    for instance in room:
        instance.update(color_mode=0, unlit=True)
        for vertex, uv in zip(instance['vertices'], instance['texcoords']):
            vertex[2] = uv
    extra = rgb_triangle(info['camera'])
    extra.update(uniform=info['camera']['uniform'], scale_s=0, cull_negative=1,
                 seq_base=0, cfg_tex_w_log2=6, cfg_tex_h_log2=6, cfg_max_mip=6)
    values = room+[extra]+[value for value in values if value['group'] != 'box']
    for i, instance in enumerate(values):
        instance.update(slot=i % 2, generation=i % 16)
        instance.setdefault('unlit', False)
        if instance is extra:
            words = oracle.F.oracle(instance['uniform'], instance['config'], instance['vertices'])
            maximum = max(abs(oracle.signed(words[6*j+k], 32)) for j in range(3) for k in range(3))
            while maximum >> instance['scale_s'] > 131071:
                instance['scale_s'] += 1
    info.update(scene='textured_room_rgb_triangle_lit_bunny', submitted_triangles=1057,
                material=dict(box='unlit RGB565 texture with all seven mip levels',
                              triangle='unlit interpolated RGB888 vertex colors',
                              bunny='solid RGB888 with diffuse and Blinn-Phong specular'))
    return values, info, shading


def configure_bench(text, shading, values):
    text = solid_scene.configure_bench(text, shading)
    assert text.count('draw_unlit=0') == 1
    text = text.replace('draw_unlit=0', 'draw_unlit=1')
    previous = True
    for i, instance in enumerate(values):
        unlit = instance['unlit']
        if unlit != previous:
            needle = f'prepare_task({i},'
            assert text.count(needle) == 1
            text = text.replace(needle, 'while(!pipeline_idle)tick();\n'
                                f'draw_unlit={int(unlit)};context_epoch=context_epoch+1;\n'
                                'draw_rebind=1;tick();draw_rebind=0;\n'+needle)
            previous = unlit
    return text
