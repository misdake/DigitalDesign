"""Independent native V/T, interpolation, texture and LESS-depth demo reference."""
import argparse
import importlib.util
import json
from pathlib import Path
import struct
import sys
sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[3]/"tests/fixtures/gpu_demo_room/tools"))
import assets
import verify_stage2


def run(output, matrices):
    output.mkdir(parents=True, exist_ok=True)
    resources = output / 'assets'
    assets.pack(resources)
    manifest = json.loads((resources / 'assets.json').read_text())
    spec = importlib.util.spec_from_file_location('demo_pixel_oracle', assets.SCENE / 'reference/scene/oracle.py')
    oracle = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(oracle)
    cameras = json.loads(matrices.read_text())
    assert len(cameras) == 512 and cameras[0] != cameras[1]
    meshes = assets.solid_scene.room()
    for mesh in meshes:
        for i, vertex in enumerate(mesh['vertices']):
            vertex[2] = round(i % 5 / 4 * 65535) | (round(i // 5 / 4 * 65535) << 16)
    scale = manifest['mesh']['scale_s']
    minimum, maximum = 0, 0
    for matrix in cameras:
        uniform = matrix + [65536] + [0] * 10
        for mesh in meshes:
            values = oracle.F.oracle(uniform, mesh['config'], mesh['vertices'])
            for i in range(len(mesh['vertices'])):
                for axis in range(3):
                    value = oracle.signed(values[6*i+axis], 32)
                    minimum, maximum = min(minimum, value), max(maximum, value)
                    assert -131072 <= value >> scale <= 131071
    (output / 'orbit-range.json').write_text(json.dumps(dict(steps=512, scale_s=scale,
        minimum_raw=minimum, maximum_raw=maximum, all_native_positions_fit=True), indent=2)+'\n')
    seed = {}
    for resource in manifest['resources']:
        payload = (resources / resource['file']).read_bytes()
        for offset in range(0, len(payload), 8):
            seed[(resource['address']+offset)//8] = struct.unpack_from('<Q',payload,offset)[0]
    (output / 'assets.hex').write_text(''.join(f'@{a:x}\n{v:016x}\n' for a,v in sorted(seed.items())))
    levels = assets.material_scene.texture_levels()
    for frame, matrix in enumerate(cameras[:2],1):
        color, depth = [0]*96000, [65535]*96000
        accepted = 0
        for index, mesh in enumerate(meshes):
            instance = dict(mesh, uniform=matrix+[65536]+[0]*10, scale_s=scale,
                color_mode=0,cull_negative=1,slot=index%2,
                cfg_tex_w_log2=6,cfg_tex_h_log2=6,cfg_max_mip=6,cfg_far_z=100*65536)
            _, packets = oracle.prepare(instance, output / f'oracle-{frame}-{index}')
            for packet in packets:
                for qi, mask in packet['masks'].items():
                    x, y = qi%200*2, qi//200*2
                    h = tuple(c+2*nx*x+2*ny*y for nx,ny,c in packet['edges'])
                    quad = oracle.M.quad(packet['triangle'],h,mask)
                    for lane, pixel in quad['pixels'].items():
                        if not (quad['passed'] >> lane & 1):
                            continue
                        at = (y+(lane>>1))*400+x+(lane&1)
                        if pixel['Z16'] >= depth[at]:
                            continue
                        rgb = verify_stage2.sample(pixel['uv'],quad['lod']['LOD_q3'],levels)
                        def rnd(v,d):
                            q,r=divmod(v,d)
                            return q+int(2*r>d or 2*r==d and q&1)
                        color[at] = rnd(rgb[0]*31,255)<<11 | rnd(rgb[1]*63,255)<<5 | rnd(rgb[2]*31,255)
                        depth[at] = pixel['Z16']
                        accepted += 1
        assert accepted > 1000
        for name, values in [(f'gold{frame}',color),(f'depth{frame}',depth)]:
            (output / f'{name}.hex').write_text(''.join(f'{v:04x}\n' for v in values))
        (output / f'matrix{frame}.hex').write_text(''.join(f'{(v>>shift)&65535:04x}\n' for v in matrix for shift in (0,16)))
        print(f'PASS demo reference frame={frame} phase={(frame-1)*128} depth_passes={accepted}',flush=True)
    print(f'PASS full orbit: 512 matrices, native scale_s={scale}, raw range={minimum}..{maximum}',flush=True)


if __name__ == '__main__':
    parser=argparse.ArgumentParser(__doc__)
    parser.add_argument('--output',required=True,type=Path)
    parser.add_argument('--matrices',required=True,type=Path)
    args=parser.parse_args()
    run(args.output.resolve(),args.matrices.resolve())
