"""Matched shared MC or connected GPU/MC fits at related 54/108MHz clocks."""
from pathlib import Path
import argparse
import concurrent.futures
import hashlib
import json
import os
import re
import subprocess
import time
import shutil

def ports(source):
    source = re.sub(r'\b(input|output|inout)(?=\[)', r'\1 ', source)
    start = re.search(r'\bmodule\s+\w+', source).end()
    head = source[start:source.index(');', start)]
    pattern = r'\b(input|output|inout)\s+(?:(?:wire|reg)\s*)?(?:signed\s*)?(\[\s*\d+:0\]\s*)?([^;]*?)(?=\binput\b|\boutput\b|\binout\b|$)'
    result = []
    for direction, width, names in re.findall(pattern, head):
        for name in names.split(','):
            name = name.strip()
            if name:
                assert re.fullmatch(r'\w+', name), name
                result.append((direction, width.strip(), name))
    assert len(set(n for _, _, n in result)) == len(result)
    return result


HERE = Path(__file__).resolve().parent


def split_pins(sources):
    result = dict(sources)
    for name in ('top.v', 'combination.v', 'bridge.v', 'controller.v'):
        text = result[name]
        text, count = re.subn(r'\binout\s+(?:wire\s+)?\[31:0\]\s*IO_sdram_dq',
                             'input wire [31:0] IO_sdram_dq,\noutput wire [31:0] experiment_dq_output,\noutput wire experiment_dq_oe', text)
        assert count == 1, (name, count)
        if name == 'controller.v':
            old = "    assign IO_sdram_dq = dq_drive ? dq_out : 32'bz;"
            assert text.count(old) == 1
            text = text.replace(old, '    assign experiment_dq_output=dq_out;assign experiment_dq_oe=dq_drive;')
        else:
            old = '.IO_sdram_dq(IO_sdram_dq)'
            assert text.count(old) == 1, name
            text = text.replace(old, old + ',.experiment_dq_output(experiment_dq_output),.experiment_dq_oe(experiment_dq_oe)')
        result[name] = text
    return result


def wrapper(source):
    p = ports(source)
    clocks = {'logic_clk', 'controller_clk', 'sdram_clk', 'reset', 'ce'}
    inputs = [(w, n) for d, w, n in p if d == 'input' and n not in clocks]
    outputs = [(w, n) for d, w, n in p if d == 'output' and n != 'O_sdram_clk']
    width = lambda w: int(re.search(r'\d+', w)[0]) + 1 if w else 1
    iw, ow = sum(width(w) for w, _ in inputs), sum(width(w) for w, _ in outputs)
    text = f'''module timing_top(input clk,reset,ce,serial_in,output[5:0]leds);
wire logic_clk,controller_clk,phy_clk;
TangNano20KSdramPll108M54M u_sdram_pll(.clkin(clk),.controller_clk(controller_clk),.logic_clk(logic_clk),.sdram_clk(phy_clk),.locked());
wire[{iw-1}:0]src;wire[{ow-1}:0]sink;
genvar i;generate for(i=0;i<{iw};i=i+1)begin:source_boundary
if(i==0)DFFE ff(.Q(src[i]),.D(serial_in),.CLK(logic_clk),.CE(ce));
else DFFE ff(.Q(src[i]),.D(src[i-1]),.CLK(logic_clk),.CE(ce));
end endgenerate
'''
    links = ['.logic_clk(logic_clk)', '.controller_clk(controller_clk)', '.sdram_clk(phy_clk)', '.reset(reset)', '.ce(ce)']
    for group, bus in ((inputs, 'src'), (outputs, 'sink')):
        offset = 0
        for w, name in group:
            links.append(f'.{name}({bus}[{offset}+:{width(w)}])')
            offset += width(w)
    links.append('.O_sdram_clk()')
    text += 'PlacementCombination dut(' + ','.join(links) + ');\n'
    text += f'wire[{ow-1}:0]captured;\n'
    # Observe native pins in their own clock domain. A giant mixed-clock XOR
    # would invent a 108->54MHz path which does not exist in the real design.
    groups = {False: [], True: []}
    offset = 0
    for w, name in outputs:
        native = name.startswith(('O_sdram_', 'experiment_dq_'))
        for bit in range(offset, offset + width(w)):
            clock = 'controller_clk' if native else 'logic_clk'
            text += f'DFFE capture{bit}(.Q(captured[{bit}]),.D(sink[{bit}]),.CLK({clock}),.CE(ce));\n'
        groups[native].extend(range(offset, offset + width(w)))
        offset += width(w)
    for j in range(6):
        native = j >= 3
        bits = groups[native][j % 3::3]
        taps = [f'captured[{bit}]' for bit in bits]
        if j == 0: taps.append(f'src[{iw-1}]')
        value = '^{' + ','.join(taps) + '}' if taps else "1'b0"
        clock = 'controller_clk' if native else 'logic_clk'
        text += f'DFFE observe{j}(.Q(leds[{j}]),.D({value}),.CLK({clock}),.CE(ce));\n'
    return text + 'endmodule\n', dict(input_bits=iw, output_bits=ow, probe_ff=iw + ow + 6, observation='Every sink registered in its own domain before observation reduction')


def freeze_gpu(gpu, output):
    """Synthesize one pad-free GPU netlist shared byte-for-byte by both trials."""
    output.mkdir(parents=True, exist_ok=False)
    text = gpu.read_text()
    (output / 'gpu.v').write_bytes(gpu.read_bytes())
    pads = [f'PORT "{name}" syn_insert_pad=0;' for _, _, name in ports(text)]
    (output / 'gpu.gsc').write_text('\n'.join(pads))
    (output / 'gpu.sdc').write_text('create_clock -name clk -period 18.518519 [get_ports {clk}]\n')
    name = re.search(r'\bmodule\s+(\w+)', text)[1]
    tcl = [f'cd {{{output.as_posix()}}}', 'set_device -name GW2AR-18C -device_version C GW2AR-LV18QN88C8/I7']
    tcl += [f'add_file {{{(output / n).as_posix()}}}' for n in ['gpu.v', 'gpu.gsc', 'gpu.sdc']]
    tcl += ['set_option -synthesis_tool gowinsynthesis', f'set_option -top_module {name}', 'set_option -output_base_name frozen_gpu', 'set_option -verilog_std v2001', 'run syn']
    (output / 'run.tcl').write_text('\n'.join(tcl))
    gw = os.environ.get('GW_SH_EXE') or str(Path(os.environ['GOWIN_HOME']) / 'IDE/bin/gw_sh.exe')
    with (output / 'tool.log').open('w') as log:
        proc = subprocess.run([gw, str(output / 'run.tcl')], cwd=output, stdout=log, stderr=subprocess.STDOUT, timeout=300)
    netlist = output / 'impl/gwsynthesis/frozen_gpu.vg'
    assert proc.returncode == 0 and netlist.exists(), f'GPU synthesis failed: {output / "tool.log"}'
    return netlist


def fit(kind, generated, output, frozen_gpu=None):
    work = output / kind
    work.mkdir(parents=True, exist_ok=False)
    src = split_pins({p.name: p.read_text() for p in (generated / kind).glob('*.v') if p.name != 'pins.v'})
    if frozen_gpu:
        del src['gpu.v']
    src['pll.v'] = (HERE.parents[3] / 'hardware/vendor/gowin/src/targets/tang_nano_20k/sdram/pll_108m_54m.v').read_text()
    source_hashes = {}
    for name, text in src.items():
        (work / name).write_text(text)
        source_hashes[name] = hashlib.sha256(text.encode()).hexdigest()
    if frozen_gpu:
        shutil.copyfile(frozen_gpu, work / 'gpu.vg')
        source_hashes['gpu.vg'] = hashlib.sha256((work / 'gpu.vg').read_bytes()).hexdigest()
    probe, boundary = wrapper(src['top.v'])
    (work / 'probe.v').write_text(probe)
    (work / 'pins.cst').write_text('IO_LOC "clk" 4;\nIO_PORT "clk" IO_TYPE=LVCMOS33 PULL_MODE=UP BANK_VCCIO=3.3;\n' +
         '\n'.join(f'IO_LOC "leds[{j}]" {pin};\nIO_PORT "leds[{j}]" IO_TYPE=LVCMOS33 DRIVE=8 BANK_VCCIO=3.3;' for j, pin in enumerate((15,16,17,18,19,20))))
    (work / 'timing.sdc').write_text('''create_clock -name clk -period 37.037037 [get_ports {clk}]
create_generated_clock -name controller_clk -source [get_ports {clk}] -multiply_by 4 [get_pins {u_sdram_pll/rpll_inst/CLKOUT}]
create_generated_clock -name logic_clk -source [get_pins {u_sdram_pll/rpll_inst/CLKOUT}] -divide_by 2 [get_pins {u_sdram_pll/rpll_inst/CLKOUTD}]
create_generated_clock -name sdram_clk -source [get_ports {clk}] -multiply_by 4 -phase 292.5 [get_pins {u_sdram_pll/rpll_inst/CLKOUTP}]
report_timing -setup -max_paths 60 -max_common_paths 5
report_timing -setup -from [get_clocks {logic_clk}] -to [get_clocks {logic_clk}] -max_paths 10 -max_common_paths 2
''')
    tcl = [f'cd {{{work.as_posix()}}}', 'set_device -name GW2AR-18C -device_version C GW2AR-LV18QN88C8/I7']
    tcl += [f'add_file {{{(work / n).as_posix()}}}' for n in [*src, *(['gpu.vg'] if frozen_gpu else []), 'probe.v', 'pins.cst', 'timing.sdc']]
    tcl += ['set_option -synthesis_tool gowinsynthesis', 'set_option -top_module timing_top',
            'set_option -output_base_name trial', 'set_option -verilog_std v2001',
            'set_option -gen_text_timing_rpt 1', 'set_option -gen_verilog_sim_netlist 1', 'set_option -place_option 0', 'run all']
    (work / 'run.tcl').write_text('\n'.join(tcl))
    gw = os.environ.get('GW_SH_EXE') or str(Path(os.environ['GOWIN_HOME']) / 'IDE/bin/gw_sh.exe')
    before = time.time()
    with (work / 'tool.log').open('w') as log:
        proc = subprocess.run([gw, str(work / 'run.tcl')], cwd=work, stdout=log, stderr=subprocess.STDOUT, timeout=600)
    report, timing = work / 'impl/pnr/trial.rpt.txt', work / 'impl/pnr/trial.tr'
    r = dict(kind=kind, returncode=proc.returncode, seconds=round(time.time()-before, 2), boundary=boundary, source_hashes=source_hashes,
             scope=('Connected GPU+CP+shared MC' if 'gpu.v' in src or frozen_gpu else 'Independent shared MC') + '; split DQ, related 54/108MHz, fixed registered observation; excludes CPU/display engines and board IO timing', frozen_gpu=bool(frozen_gpu))
    if report.exists() and timing.exists():
        rt, tt = report.read_text(errors='replace'), timing.read_text(errors='replace')
        for key, pattern in [('logic',r'^\s*Logic\s*\|\s*(\d+)'), ('logic_ff',r'--Logic Register as FF\s*\|\s*(\d+)'),
                             ('bsram',r'^\s*BSRAM\s*\|\s*([\d.]+)'), ('dsp',r'^\s*DSP\s*\|\s*([\d.]+)'),
                             ('ssram',r'--SSRAM\(RAM16\)\s*\|\s*(\d+)')]:
            m = re.search(pattern, rt, re.M)
            r[key] = float(m[1]) if m else 0
        for key, pattern in [('setup_violations',r'<Numbers of Setup Violated Endpoints>:(\d+)'), ('hold_violations',r'<Numbers of Hold Violated Endpoints>:(\d+)')]:
            m = re.search(pattern, tt)
            r[key] = int(m[1]) if m else None
        r['fmax'] = {n:float(v) for n,v in re.findall(r'\b(controller_clk|logic_clk)\s+[\d.]+\(MHz\)\s+([\d.]+)\(MHz\)', tt)}
        r['report_sha256'] = hashlib.sha256(report.read_bytes()).hexdigest()
        r['timing_sha256'] = hashlib.sha256(timing.read_bytes()).hexdigest()
    (work / 'receipt.json').write_text(json.dumps(r, indent=2))
    print(json.dumps({k:v for k,v in r.items() if k not in ('source_hashes','scope')}), flush=True)
    assert r.get('logic') is not None, f'{kind} failed; inspect {work / "tool.log"}'
    return kind, r



def gpu_memory_top(gpu, memory):
    """Connect the real three-master GPU to the actual seven-client arbiter/MC."""
    gp, mp = ports(gpu), ports(memory)
    bus = lambda name: name.startswith(('ro_', 'fb_r_', 'fb_w_'))
    declarations = []; known = {}; internal = []
    def external(direction, width, name):
        if name in known:
            assert known[name] == (direction, width), name
        else:
            known[name] = (direction, width)
            declarations.append(f'{direction} wire {width} {name}')
    glinks = []
    for direction, width, name in gp:
        if name == 'clk':
            glinks.append('.clk(logic_clk)')
        else:
            glinks.append(f'.{name}({name})')
            if bus(name):internal.append(f'wire {width} {name};')
            else:external(direction, width, name)
    mlinks = []
    for direction, width, name in mp:
        if name.startswith('gpu_'):
            suffix = name[4:]
            prefix, field = next((p, suffix[len(p)+1:]) for p in ['ro', 'fb_r', 'fb_w'] if suffix.startswith(p+'_'))
            if field == 'write':value = "1'b1" if prefix=='fb_w' else "1'b0"
            elif field == 'write_data':value = 'fb_w_data' if prefix=='fb_w' else "64'd0"
            elif field == 'line_count_minus_1':value = prefix+'_count'
            elif (field=='read_data' and prefix=='fb_w') or (field=='write_data_ready' and prefix!='fb_w'):value=''
            elif field in ['error','read_data','write_data_ready']:
                value = prefix+dict(error='_response_error',read_data='_data',write_data_ready='_data_ready')[field]
            else:value = prefix+'_'+field
            mlinks.append(f'.{name}({value})')
        else:
            external(direction, width, name);mlinks.append(f'.{name}({name})')
    assert all(name in known for name in ['logic_clk','controller_clk','sdram_clk','reset','ce'])
    gpu_name=re.search(r'\bmodule\s+(\w+)',gpu)[1]
    return ('module PlacementCombination(\n'+',\n'.join(declarations)+'\n);\n'+'\n'.join(internal)+
            '\n'+gpu_name+' gpu('+','.join(glinks)+');\nGowinSdramCombination core('+','.join(mlinks)+');\nendmodule\n')


def system_binding(gpu):
    """Match CpuV3Gpu's production pins: CE=1, abort=0, output_ready=1.

    Debug observations are not system ports. The unused CE input belongs only
    to the registered measurement boundary, not to GPU execution.
    """
    p = ports(gpu)
    name = re.search(r'\bmodule\s+(\w+)', gpu)[1]
    def keep(direction, pin):
        return (direction == 'input' and pin not in ('abort', 'output_ready')
                or direction == 'output' and pin.startswith(('ro_', 'fb_r_', 'fb_w_', 'device_read_data')))
    visible = [(d,w,n) for d,w,n in p if keep(d,n)]
    links = []
    for d,w,n in p:
        value = "1'b1" if n in ('ce','output_ready') else "1'b0" if n == 'abort' else n if keep(d,n) else ''
        links.append(f'.{n}({value})')
    return ('module SystemGpuBinding(\n' + ',\n'.join(f'{d} wire {w} {n}' for d,w,n in visible)
            + '\n);\n' + name + ' core(' + ','.join(links) + ');\nendmodule\n' + gpu)


def main():
    ap = argparse.ArgumentParser(__doc__)
    ap.add_argument('--base', type=Path, required=True, help='Preserved baseline MC peer')
    ap.add_argument('--candidate', type=Path, required=True, help='Current exported MC peer')
    ap.add_argument('--output', type=Path, required=True)
    ap.add_argument('--gpu-base', type=Path)
    ap.add_argument('--gpu-candidate', type=Path)
    ap.add_argument('--system-binding', action='store_true', help='Match system GPU constants and production observations in both trials')
    ap.add_argument('--fixed-gpu', action='store_true', help='Compile one pad-free GPU netlist, then import exactly that netlist in both connected trials')
    args = ap.parse_args()
    assert not args.system_binding or (args.gpu_base and not args.fixed_gpu), "System binding requires live GPU sources"
    assert bool(args.gpu_base) == bool(args.gpu_candidate), "Provide both GPU variants"
    out = args.output.resolve(); out.mkdir(parents=True, exist_ok=False)
    frozen_gpu = None
    if args.fixed_gpu:
        assert args.gpu_base and args.gpu_candidate, '--fixed-gpu requires GPU source paths'
        assert args.gpu_base.read_bytes() == args.gpu_candidate.read_bytes(), 'GPU sources differ'
        frozen_gpu = freeze_gpu(args.gpu_base, out / 'frozen-gpu')
    generated = out / 'sources'
    raw_hashes = {}
    for kind, peer in [('base', args.base.resolve()), ('candidate', args.candidate.resolve())]:
        dest = generated / kind; dest.mkdir(parents=True)
        raw_hashes[kind] = {}
        for name in ['combination.v', 'bridge.v', 'controller.v', 'shared_port.v', 'cpu_v3_memory_arbiter.v']:
            text = (peer / name).read_text()
            raw_hashes[kind][name] = hashlib.sha256((peer / name).read_bytes()).hexdigest()
            # Only whitespace normalization; the split-DQ boundary is below.
            text = re.sub(r'inout\s*(?:wire\s*)?\[31:0\]\s*IO_sdram_dq', 'inout wire [31:0] IO_sdram_dq', text)
            (dest / name).write_text(text)
        text = (dest / 'combination.v').read_text()
        header = text.split(');', 1)[0].replace('GowinSdramCombination', 'PlacementCombination', 1)
        names = re.findall(r'\b(?:input|output|inout)\s+(?:wire\s+)?(?:\[[^]]+\]\s*)?(\w+)', header)
        links = ','.join(f'.{name}({name})' for name in names)
        (dest / 'top.v').write_text(header + ',input ce\n);\nGowinSdramCombination core(' + links + ');\nendmodule\n')
        if args.gpu_base:
            gpu=(args.gpu_base if kind=='base' else args.gpu_candidate).read_text()
            raw_hashes[kind]['gpu.v']=hashlib.sha256(gpu.encode()).hexdigest()
            if args.system_binding:gpu=system_binding(gpu)
            (dest/'gpu.v').write_text(gpu)
            (dest/'top.v').write_text(gpu_memory_top(gpu,text))
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
        results = dict(pool.map(lambda n: fit(n, generated, out, frozen_gpu), ['base', 'candidate']))
    receipt = dict(raw_peer_sha256=raw_hashes, fits=results,
                   test_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                   fixed_gpu_netlist_sha256=(hashlib.sha256(frozen_gpu.read_bytes()).hexdigest() if frozen_gpu else None),
                   system_binding=args.system_binding,scope=('Connected GPU+CP+shared arbiter/MC' if args.gpu_base else 'Independent shared MC')+'; fixed boundary, excludes CPU/display engines and board IO timing')
    (out / 'receipt.json').write_text(json.dumps(receipt, indent=2))


if __name__ == '__main__':
    main()
