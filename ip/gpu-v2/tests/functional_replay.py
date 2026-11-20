"""Exact native functional/production GPU+MC comparisons on independent inputs.

Synthetic inputs are test vectors, not CPU producer or physical-board evidence.
The public RCC submit/wait integration is qualified separately.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import subprocess
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[3]
TOOLS = ROOT / 'systems/cpu-v3-tang-nano-20k/tools/gpu_profiler'
sys.path[:0] = [str(TOOLS), str(TOOLS / 'tests')]
from draw_replay import synthetic_snapshot
from test_draw_oracle import SyntheticCapture
from layout import COLUMN
from replay import prepare, execute
from memcpy_fixture import make as memcpy_capture
import snapshot as snapshot_abi


def classic_case():
    """Current production asset packer: one floor and the first two bunny pages.

    This deliberately bounded representative is not a captured CPU frame or the
    historical full-scene visual gate. Matrices and V96 come from scene.json.
    """
    example = ROOT / 'systems/cpu-v3-tang-nano-20k/examples/demo_box_rabbit'
    spec = importlib.util.spec_from_file_location('native_demo_assets', example/'tools/assets.py')
    assets = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(assets)
    scene = json.loads((example/'assets/scene.json').read_text())
    selected = [next(m for m in scene['meshes'] if m['name'] == name)
                for name in ('room_floor', 'bunny_full_0', 'bunny_full_1')]
    camera = scene['cameras'][0]
    source = SyntheticCapture(COLUMN)
    objects = bytearray(source.memory[0x4d0000])
    uniform = [v & 0xffffffff for v in camera['matrix']] + [65536, 0xffffffff] + [0]*9
    struct.pack_into('<32I', objects, 128, *uniform)
    lx, ly, lz = (v & 65535 for v in camera['light'])
    struct.pack_into('<6I', objects, 64, 199*65536, lx | (ly << 16),
                     lz | (8192 << 16), 4915 | (8192 << 16), 128 | (128 << 16), 3 | (12 << 2))
    texture = assets.texture()
    assert len(texture) == 12800  # Production 64x64 complete mip chain.
    descriptor = 0x4f0000//128 | (6 << 16) | (1 << 20) | (2 << 21)
    struct.pack_into('<3I', objects, 32, descriptor, 0x808080, 32 | 8 | 1)
    objects.extend(bytes(32))
    struct.pack_into('<3I', objects, 256, descriptor, 0x808080, 8 | 1)
    source.put(0x4d0000, objects)
    source.put(0x4f0000, texture)
    def dma(address, slot, code):
        return 0x10000000 | ((address//32) << 10) | (slot << 3) | (code << 1)
    source.commands = [0x40000003, 0x70000000, dma(0x4d0000,66,1),0x40000001,
                       dma(0x4d0040,68,1),0x40000001,dma(0x4d0080,80,2),0x40000001,
                       0xa0000042,0x50000044,0x60000043,0x80001234,0x8001ffff]
    records = bytearray()
    for index, mesh in enumerate(selected):
        record = assets.native_record(mesh)
        address = 0x4e0000 + len(records)
        slot = index & 1
        if index == 1:
            source.commands += [0x40000003,0x70000000,dma(0x4d0100,67,0),0x40000001,0x60000043]
        source.commands += [0x20000000 | ((address//128) << 12) | ((len(record)//128-2) << 1) | slot,
                            0x40000001,0x30000000 | slot]
        records += record
    source.put(0x4e0000, records)
    source.commands += [0x40000003,0x70000000,0xf0000000]
    source.update_commands()
    return source


def source_case(name):
    if name == 'classic-floor-two-bunny':
        return classic_case()
    source = SyntheticCapture(COLUMN, tint=0x7f4080ff if name == 'blend' else 0xffffffff,
                              rgb=name in ('rgb', 'blend', 'diffuse', 'specular'))
    objects = bytearray(source.memory[0x4d0000])
    def dma(address, slot, code):
        return 0x10000000 | ((address//32) << 10) | (slot << 3) | (code << 1)
    if name in ('ui-full', 'ui-range'):
        grid = bytearray(b'\xff'*768)
        grid[0], grid[1], grid[749] = 0, 1, 2
        source.put(0x4e0000, grid)
        atlas = bytearray(65536)
        for char in range(3):
            for y in range(16):
                for x in range(8):
                    pixel = (((x+1+char*8) & 31) << 11) | (((y+char*5) & 31) << 6) | (((x+y) & 31) << 1) | int((x+y+char) % 3 != 0)
                    stored_x = x ^ (2 if y & 1 else 0)
                    struct.pack_into('<H',atlas,char*256+y*16+stored_x*2,pixel)
        source.put(0x4f0000,atlas)
        objects[32:64] = bytes(32)
        struct.pack_into('<I',objects,32,0x4e0000//128 | ((0x4f0000//65536) << 16))
        ui = 0xb0000043 if name == 'ui-full' else 0xb0000043 | (1 << 27) | (1 << 7) | (1 << 17)
        source.commands = [0x40000003,0x70000000,dma(0x4d0000,66,1),0x40000001,0xa0000042,
                           0x80001234,0x8001ffff,0x40000003,0x70000000,ui,0xf0000000]
    if name in ('alpha-endpoints', 'depth-functions'):
        mesh = bytearray(source.memory[0x4e0000])
        struct.pack_into('<I',mesh,8,16)  # RGB payload, constant white.
        for vertex in range(3): struct.pack_into('<I',mesh,128+vertex*12+8,0x00ffffff)
        source.put(0x4e0000,mesh)
        source.commands = [0x40000003,0x70000000,dma(0x4d0000,66,1),0x40000001,
                           dma(0x4d0040,68,1),0x40000001,0xa0000042,0x60000043,0x50000044,
                           0x80000000,0x8001ffff,0x20000000 | ((0x4e0000//128) << 12),0x40000001]
        template = list(struct.unpack_from('<32I',objects,128))
        if name == 'alpha-endpoints':
            struct.pack_into('<I',objects,40,32 | 16 | 8 | 7)
            for i,alpha in enumerate((0,1,2,253,254,255)):
                uniform = template[:]
                uniform[0], uniform[5] = 8192, 16384
                uniform[3] = int((-2.5+i)*65536) & 0xffffffff
                uniform[22] = 0x00ffffff | (alpha << 24)
                offset = 128+i*128
                if len(objects)<offset+128: objects.extend(bytes(offset+128-len(objects)))
                struct.pack_into('<32I',objects,offset,*uniform)
                source.commands += [dma(0x4d0000+offset,80,2),0x40000001,0x30000000]
        else:
            objects = objects[:128] + bytearray(32*32+32*128)
            for row,clear in enumerate((0,1023,1024,65535)):
                source.commands += [0x40000003,0x70000000,0x80010000 | clear]
                for function in range(8):
                    index = row*8+function
                    state_offset = 128+index*32
                    state = [0x4f0000//128,0,32 | 8 | function,0,0,0,0,0]
                    struct.pack_into('<8I',objects,state_offset,*state)
                    uniform = template[:]
                    uniform[0],uniform[5] = 4096,8192
                    uniform[3] = round(((25+function*50)-199.5)*4/256*65536) & 0xffffffff
                    uniform[7] = round((119.5-(30+row*60))*4/256*65536) & 0xffffffff
                    uniform[22] = 0xffffffff
                    offset = 128+32*32+index*128
                    struct.pack_into('<32I',objects,offset,*uniform)
                    source.commands += [0x40000003,dma(0x4d0000+state_offset,67,0),0x40000001,0x60000043,
                                        dma(0x4d0000+offset,80,2),0x40000001,0x30000000]
        source.commands += [0x40000003,0x70000000,0xf0000000]
    if name.startswith('normal'):
        negative = name.endswith('negative')
        mesh = bytearray(source.memory[0x4e0000])
        struct.pack_into('<I',mesh,28,8 | (256 if name.startswith('normal18') else 0))
        normal = ((-1 & 1023) | (1 << 10)) if negative else 1
        for vertex in range(3): struct.pack_into('<I',mesh,128+vertex*12+4,normal)
        source.put(0x4e0000,mesh)
        struct.pack_into('<I',objects,128+12*4,48128 if negative else 16896)
        struct.pack_into('<I',objects,128+16*4,32768)
        struct.pack_into('<I',objects,40,8 | 1)
        struct.pack_into('<I',objects,68,(-16384 if negative else 16384) & 65535)
        struct.pack_into('<I',objects,72,0 | (8192 << 16))
        struct.pack_into('<I',objects,80,32 | (224 << 16))
        struct.pack_into('<I',objects,84,2)
    if name == 'vertex-wrap':
        mesh = bytearray(source.memory[0x4e0000])
        struct.pack_into('<I',mesh,16,0x80000000)
        for vertex,(x,y) in enumerate(((0,0),(1,0),(0,256))):
            struct.pack_into('<I',mesh,128+vertex*12,x | (y << 10))
        source.put(0x4e0000,mesh)
        struct.pack_into('<I',objects,128,1 << 24)
        struct.pack_into('<I',objects,128+12,-32768 & 0xffffffff)
    if name == 'blend':
        struct.pack_into('<I', objects, 40, 32 | 16 | 8 | 1)
    if name in ('diffuse', 'specular'):
        struct.pack_into('<I', objects, 40, 8 | 1)
        struct.pack_into('<I', objects, 80, 32 | (224 << 16))
        struct.pack_into('<I', objects, 84, (3 if name == 'specular' else 2) | (12 << 2))
        struct.pack_into('<I', objects, 36, 0x6080a0 if name == 'specular' else 0)
    if name in ('bilinear', 'trilinear', 'rtt-nearest', 'rtt-bilinear'):
        mesh = bytearray(source.memory[0x4e0000])
        for vertex, (u, v) in enumerate(((0, 0), (65535, 0), (32768, 65535))):
            struct.pack_into('<I', mesh, 128 + vertex*12 + 8, u | (v << 16))
        source.put(0x4e0000, mesh)
        fixed = name.startswith('rtt-')
        exponent = 9 if fixed else 8
        full = name == 'trilinear'
        filtering = 0 if name == 'rtt-nearest' else 2 if full else 1
        descriptor = 0x4f0000//128 | (exponent << 16) | (int(full) << 20) | (filtering << 21) | (int(fixed) << 23)
        struct.pack_into('<I', objects, 32, descriptor)
        texture = bytearray()
        levels = (exponent,) if not full else range(exponent+1)
        for level in levels:
            side = 1 << level
            width, height, pitch = (400, 256, 16) if fixed else (max(16, side), max(16, side), max(1, side//16))
            data = bytearray(width*height*2)
            for x in range(width):
                for y in range(height):
                    # Padding repeats the actual POT level; framebuffer padding
                    # is present but Sampling must never depend on rows240..255.
                    xx, yy = (x, y) if fixed else (x % side, y % side)
                    color = (((xx//3 + level*3) & 31) << 11) | (((yy//2 + level*7) & 63) << 5) | ((xx+yy+level) & 31)
                    at = (((x//16)*pitch + y//16)*256 + (y % 16)*16 + x % 16)*2
                    struct.pack_into('<H', data, at, color)
            texture.extend(data)
        source.put(0x4f0000, texture)
    source.put(0x4d0000, objects)
    source.update_commands()
    return source


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--rtl', type=Path, required=True)
    parser.add_argument('--executable', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    supported = ['nearest','rgb','blend','diffuse','specular','bilinear','trilinear','rtt-nearest','rtt-bilinear',
                 'ui-full','ui-range','alpha-endpoints','depth-functions','normal16-half','normal18-half',
                 'normal16-negative','normal18-negative','vertex-wrap','memcpy','classic-floor-two-bunny']
    parser.add_argument('--cases', nargs='+', choices=supported, default=supported)
    parser.add_argument('--reuse-rtl',type=Path,help='Recheck an immutable same-input RTL result instead of rerunning it')
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=False)
    results = []
    for name in args.cases:
        directory = args.out / name
        directory.mkdir()
        capture = memcpy_capture(snapshot_abi.Abi())[0] if name == 'memcpy' else synthetic_snapshot(COLUMN, source_case(name))
        capture.save(directory/'snapshot')
        memory = bytearray(b'\x5a\xa5' * 0x400000)
        for region in capture.ranges:
            memory[region.address:region.address+region.length] = capture.read(region.address,region.length)
        before = bytes(memory)
        source = directory/'memory.bin'
        destination = directory/'functional.bin'
        source.write_bytes(before)
        command = [str(args.executable.resolve()),str(source.resolve()),str(capture.header['cb_base']),
                   str(capture.header['count_minus_one']),'64000000',str(destination.resolve())]
        run = subprocess.run(command,capture_output=True,text=True,timeout=120)
        (directory/'functional.log').write_text(run.stdout+run.stderr)
        if run.returncode:
            raise AssertionError(f'{name}: functional execution failed: {run.stderr}')
        actual = destination.read_bytes()
        if args.reuse_rtl:
            reference = args.reuse_rtl/name/'rtl'
            rtl = json.loads((reference/'replay.json').read_text())
            manifest = json.loads((reference/'replay-manifest.json').read_text())
            if manifest['input_sha256'] != capture.manifest()['input_sha256']:
                raise AssertionError('Frozen RTL input identity changed')
            for source_file in manifest['sources']:
                if hashlib.sha256(Path(source_file['path']).read_bytes()).hexdigest() != source_file['sha256']:
                    raise AssertionError('Frozen production RTL source identity changed')
        else:
            # The production floor/two bunny pages shade about40k pixels; the
            # depth matrix has32 draws and repeated full-plane clears. Keep the
            # same cycle ceiling, with a separate finite wall limit for these
            # larger inputs rather than the one-triangle vectors.
            wall_seconds = 600 if name in ('classic-floor-two-bunny', 'depth-functions') else 180
            manifest = prepare(capture,args.rtl,directory/'rtl',2_000_000,wall_seconds,
                               dict(fixture_only=True,actual_cpu_producer=False,captured_plane_bytes=COLUMN.plane_bytes))
            rtl = execute(capture,manifest)
            reference = directory/'rtl'
        writable = bytearray(len(before))
        for role in ('color','depth'):
            base = capture.header[role+'_base']
            end = base+COLUMN.plane_bytes
            expected = (reference/'capture'/(role+'.bin')).read_bytes()
            output = actual[base:end]
            differences = [i for i in range(0,len(output),2) if output[i:i+2] != expected[i:i+2]]
            if differences:
                mismatch = dict(case=name,role=role,halfwords=len(differences),first=[dict(offset=i,
                    functional=int.from_bytes(output[i:i+2],'little'),rtl=int.from_bytes(expected[i:i+2],'little')) for i in differences[:16]])
                (directory/'mismatch.json').write_text(json.dumps(mismatch,indent=2)+'\n')
                raise AssertionError(mismatch)
            writable[base:end] = b'\1'*(end-base)
        raw_bytes = 0
        for row in rtl.get('raw_buffers',[]) + rtl.get('write_ranges',[]):
            base, size = row['address'], row['bytes']
            expected = (reference/'capture'/row['file']).read_bytes()
            if actual[base:base+size] != expected:
                raise AssertionError(f'{name}: raw writable range mismatch at {base:#x}')
            writable[base:base+size] = b'\1'*size
            raw_bytes += size
        illegal = [i for i in range(len(actual)) if not writable[i] and actual[i] != before[i]]
        if illegal:
            raise AssertionError(f'{name}: functional write outside output planes: {illegal[:16]}')
        result = dict(case=name,input_sha256=capture.manifest()['input_sha256'],memory_sha256=hashlib.sha256(before).hexdigest(),
                      physical_bytes_compared=2*COLUMN.plane_bytes,raw_buffer_bytes_compared=raw_bytes,
                      functional_executable_sha256=hashlib.sha256(args.executable.read_bytes()).hexdigest(),
                      rtl_cycles=rtl['cold_submission_cycles'],rtl_reused=bool(args.reuse_rtl),pass_=True)
        results.append(result)
        print(json.dumps(result),flush=True)
        (args.out/'result.json').write_text(json.dumps(results,indent=2)+'\n')


if __name__ == '__main__':
    main()
