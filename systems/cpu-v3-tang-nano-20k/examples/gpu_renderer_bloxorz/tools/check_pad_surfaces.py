"""Audit decoded native pad surfaces, without a renderer-derived oracle."""
import argparse
import json
from pathlib import Path


def check(directory):
    data = json.loads((directory / 'assets.json').read_text())
    group = next(g for g in data['catalog'] if g['name'] == 'WELL')
    assert group['count'] == 1
    mesh = data['meshes'][group['first']]
    assert mesh['cull_negative'] and mesh['color_mode'] == 1
    assert len(mesh['vertices']) == 16 and len(mesh['triangles']) == 8
    vertices = []
    for position, normal, _ in mesh['vertices']:
        xyz = [(mesh['config'][i] + ((position >> (10*i)) & 1023)*512)/65536 for i in range(3)]
        n = [(normal >> (10*i)) & 1023 for i in range(3)]
        n = [v-1024 if v & 512 else v for v in n]
        vertices.append((xyz, n))
    expected = [(0, -60/128), (2, -60/128), (0, 63/128), (2, 63/128)]
    for face, (axis, plane) in enumerate(expected):
        points = vertices[face*4:face*4+4]
        assert all(p[axis] == plane and n[axis] == 511 and n[1] == 0 for p, n in points)
        assert {p[1] for p, _ in points} == {-1/128, -266/128}
        for tri in mesh['triangles'][face*2:face*2+2]:
            assert all(face*4 <= i < face*4+4 for i in tri)
            a, b, c = [vertices[i][0] for i in tri]
            u = [b[i]-a[i] for i in range(3)]
            v = [c[i]-a[i] for i in range(3)]
            cross = [u[1]*v[2]-u[2]*v[1], u[2]*v[0]-u[0]*v[2], u[0]*v[1]-u[1]*v[0]]
            assert cross[axis] < 0
    hatch_group = next(g for g in data['catalog'] if g['name'] == 'TILE0')
    hatch = data['meshes'][hatch_group['first']]
    assert hatch_group['count'] == 1 and hatch['cull_negative']
    assert len(hatch['vertices']) == 20 and len(hatch['triangles']) == 10
    normals = {tuple((n >> (10*i)) & 1023 for i in range(3)) for _, n, _ in hatch['vertices']}
    assert normals == {(511,0,0),(513,0,0),(0,511,0),(0,0,511),(0,0,513)}
    result = dict(wall_quads=4, inner_quads=2, outer_quads=2, bottom=False,
                  top_caps=False, variants=1, inward_winding=True,
                  hatch_top_and_four_sides=True, local_culling=True,
                  scope='Decoded native positions/normals/triangles; not a hardware depth or all-pose visibility proof.')
    (directory / 'pad-surface-qualification.json').write_text(json.dumps(result, indent=2)+'\n')
    print(json.dumps(result))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('directory', type=Path)
    check(parser.parse_args().directory)
