"""Bounded production CP CLEAR replay with retained V golden and real dirty cache."""
from pathlib import Path
import argparse, copy, hashlib, importlib.util, json, os, re, struct, subprocess, sys
ROOT = Path(__file__).resolve().parents[3]
HERE = ROOT / 'ip/gpu-v2/examples/vliw_integration'
sys.dont_write_bytecode = True
sys.path.insert(0, str(HERE))
from replay_command import CommandBuffer, texture_word, target_params


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def assets(out):
    oracle = load('clear_oracle', HERE / 'reference/scene/oracle.py')
    oracle = load('clear_transport', HERE / 'transport.py').adapt_oracle(oracle)
    fixture = json.loads((HERE / 'reference/uv-inputs.json').read_text())['scenes'][0]['meshlets'][0]
    memory = bytearray(65536)
    def put(address, data):
        memory[address:address + len(data)] = data
    records, vectors, uniform = [], [], None
    for i in range(4):
        instance = copy.deepcopy(fixture)
        indices = instance['triangles'][0]
        instance['vertices'] = [copy.deepcopy(instance['vertices'][v]) for v in indices]
        instance.update(slot=i % 2, triangles=[[0, 1, 2]], cull_negative=1, generation=i, seq_base=i)
        if i == 3:
            instance['color_mode'] = 1
            for vertex in instance['vertices']:
                vertex[2] = 0xe07030
        meta, _ = oracle.prepare(instance, out / f'trial{i}')
        if bool(meta['packets']) != (i == 3):
            instance['triangles'] = [[0, 2, 1]]
        meta, _ = oracle.prepare(instance, out / f'task{i}')
        assert bool(meta['packets']) == (i == 3)
        image = {int(a): int(v, 16) for a, v in
                 (line.split() for line in (out / f'task{i}/dma.txt').read_text().splitlines())}
        page = i % 2
        rows = [a - page * 128 for a in image if page * 128 <= a < (page + 1) * 128]
        raw = b''.join(struct.pack('<Q', image.get(page * 128 + j, 0)) for j in range(max(rows) + 1))
        size = (len(raw) + 127) // 128 * 128
        header = b''.join(struct.pack('<Q', image[256 + page * 4 + k]) for k in range(4))
        address = 0x8000 + i * 0x500
        put(address, header + bytes(96) + raw + bytes(size - len(raw)))
        records.append((address, size))
        vectors += (out / f'task{i}/vertices.hex').read_text().splitlines()
        if uniform is None:
            uniform = b''.join(struct.pack('<Q', image[j]) for j in range(320, 336))
    put(0x4000, struct.pack('<16I', 199 * 65536, 0, 0, 0, 32 | (192 << 16),
                            3 | (8 << 2) | (1 << 16), *([0] * 10)))
    uniform = bytearray(uniform)
    struct.pack_into('<II', uniform, 88, 0xffffffff, 0)
    put(0x4280, uniform)
    for address, unlit in [(0x4100, True), (0x4140, False)]:
        put(address, struct.pack('<8I', texture_word(0x1000, 6, True, 2), 0,
                                 9 | (int(unlit) << 5), 0, 0, 0, 0, 0))
    put(0x4460, target_params(0x100000, 0x140000))
    return memory, put, records, vectors


def program(records, dirty=False):
    cb = CommandBuffer()
    cb.emit('DMA', 288, 0x4460, 32); cb.emit('WAIT', a=3); cb.emit('FLUSH'); cb.emit('SET_TARGET', 288)
    def clear(plane, value):
        cb.emit('WAIT', a=3)
        cb.emit('FLUSH')
        cb.emit('CLEAR', plane, value)
    clear(0, 0x6b4d)
    cb.emit('DMA', 272, 0x4000, 64); cb.emit('DMA', 320, 0x4280, 128)
    cb.emit('DMA', 280, 0x4100, 32); cb.emit('WAIT', a=3)
    cb.emit('SET_GLOBAL', 272); cb.emit('SET_RENDER', 280)
    cb.emit('DMA_MESHLET', 0, *records[0]); cb.emit('WAIT', a=1)
    while len(cb.records) < 30: cb.emit('NOP')
    cb.emit('DMA_MESHLET', 1, *records[1]); cb.emit('DRAW_MESHLET', 0); cb.emit('WAIT', a=1)
    while len(cb.records) < 62: cb.emit('NOP')
    cb.emit('DMA_MESHLET', 0, *records[2]); cb.emit('DRAW_MESHLET', 1); cb.emit('WAIT', a=1)
    cb.emit('DMA', 280, 0x4140, 32); cb.emit('WAIT', a=3); cb.emit('SET_RENDER', 280)
    cb.emit('DRAW_MESHLET', 0)
    if dirty:
        cb.emit('DMA', 280, 0x4100, 32); cb.emit('WAIT', a=3); cb.emit('SET_RENDER', 280)
        cb.emit('DMA_MESHLET', 1, *records[3]); cb.emit('WAIT', a=1); cb.emit('DRAW_MESHLET', 1)
    clear(1, 0x1357)
    cb.emit('FLUSH'); cb.emit('END')
    return cb.encode()


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--export', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--native', action='store_true')
    parser.add_argument('--modes', nargs='+', type=int, choices=range(9), default=list(range(9)))
    parser.add_argument('--hardware-errors', action='store_true', help='Legacy checked command baseline')
    args = parser.parse_args()
    out = args.output.resolve(); out.mkdir(parents=True, exist_ok=False)
    memory, put, records, vectors = assets(out)
    put(0x4460, target_params(0x110000, 0x150000))
    normal = program(records); dirty = program(records, dirty=True)
    put(0x1000, normal + bytes((-len(normal)) % 128))
    put(0x1800, dirty + bytes((-len(dirty)) % 128))
    (out / 'memory.hex').write_text(''.join(f'{int.from_bytes(memory[i:i+8], "little"):016x}\n'
                                              for i in range(0, len(memory), 8)))
    (out / 'vertices.hex').write_text('\n'.join(vectors) + '\n')
    source = args.export.resolve(); sha = hashlib.sha256(source.read_bytes()).hexdigest()
    (out / 'dut.v').write_bytes(source.read_bytes())
    sys.path.insert(0, str(ROOT / 'ip/gpu-v2/examples/postquad_ii4'))
    from wrapper import wrapper
    _, ports = wrapper(re.sub(r'\b(input|output)(?=\[)', r'\1 ', source.read_text()))
    declarations = []
    for port in ports['ports']:
        width = f'[{port["bits"]-1}:0]' if port['bits'] > 1 else ''
        declarations.append(('reg ' if port['direction'] == 'input' else 'wire ') + width + ' ' +
                            port['name'] + ('=0;' if port['direction'] == 'input' else ';'))
    tb = Path(__file__).with_name('vliw_clear_tb.v').read_text()
    tb = tb.replace('@PORTS@', '\n'.join(declarations)).replace('@BYTES@', str(len(normal)))
    tb = tb.replace('@DIRTY_BYTES@', str(len(dirty)))
    first_clear = next(i for i in range(len(normal)//4) if (int.from_bytes(normal[4*i:4*i+4], 'little') >> 28) == 8)
    tb = tb.replace('@CLEAR_Q@', str((0x1000 + first_clear*4)//8)).replace('@CLEAR_BIT@', str((first_clear % 2)*32))
    from device_bus import bench as device_bench
    tb=device_bench(tb)
    (out / 'tb.v').write_text(tb)
    cmd = [os.environ['IVERILOG_EXE'], '-g2012', '-s', 'tb', '-o', out / 'sim.vvp', out / 'dut.v', out / 'tb.v']
    if args.native:
        cmd.append(Path(os.environ['GOWIN_HOME']) / 'IDE/simlib/gw2a/prim_sim.v')
    def run(name, command, diagnostic=False):
        assert hashlib.sha256((out / 'dut.v').read_bytes()).hexdigest() == sha
        with (out / (name + '.log')).open('wb') as log:
            result = subprocess.run(list(map(str, command)), cwd=out, stdout=log, stderr=subprocess.STDOUT, timeout=90)
        text=(out / (name + '.log')).read_text(errors='replace')
        if diagnostic:
            assert result.returncode != 0 and any(label in text for label in
                ['GPU contract:', 'CP CLEAR memory response', 'CP state data contract', 'CP plane CLEAR contract']),text[-2000:]
        else:
            assert result.returncode == 0,text[-2000:]
    run('compile', cmd)
    summaries = []
    for mode in args.modes:
        diagnostic=mode in (3,4,5) and not args.hardware_errors
        run(f'run-{mode}', [os.environ['VVP_EXE'], out / 'sim.vvp', f'+MODE={mode}'],diagnostic)
        if diagnostic:summaries.append(f'PASS CLEAR mode={mode} diagnosed by independent simulation assertion')
        else:summaries += [line for line in (out / f'run-{mode}.log').read_text().splitlines() if line.startswith('PASS ')]
    receipt = dict(status='pass', native=args.native, export_sha256=sha, command_bytes=len(normal),
                   test_sources={p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in
                                 [Path(__file__), Path(__file__).with_name('vliw_clear_tb.v')]}, cases=summaries)
    (out / 'receipt.json').write_text(json.dumps(receipt, indent=2))
    print(json.dumps(receipt))


if __name__ == '__main__': main()
