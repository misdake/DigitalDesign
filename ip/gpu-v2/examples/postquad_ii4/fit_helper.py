from pathlib import Path
import re, json, hashlib, subprocess, time, concurrent.futures

OUT = Path('target/postquad-fit')
SOURCE = OUT / 'source'
GW = 'gw_sh'
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()

def wrapper(rtl):
    head = rtl[re.search(r'^module\s+\w+',rtl,re.M).start():].split(');', 1)[0]
    module = re.search(r'module (\w+)', head).group(1)
    ports = re.findall(r'^\s*(input|output)\s+(?:(?:reg|wire)\s+)?(?:signed\s+)?(?:\[(\d+):0\]\s+)?(\w+)', head, re.M)
    widths = {name: int(high)+1 if high else 1 for _, high, name in ports}
    inputs = [n for d, _, n in ports if d == 'input' and n not in ('clk','ce','reset','in_valid')]
    outputs = [n for d, _, n in ports if d == 'output']
    iw, ow = sum(widths[n] for n in inputs), sum(widths[n] for n in outputs)
    body = f'module timing_top(input clk,ce,reset,in_valid,serial_in,output [5:0] leds);\nwire [{iw-1}:0] src;\nwire [{ow-1}:0] sink;\ngenvar i;\ngenerate for(i=0;i<{iw};i=i+1) begin: source_boundary\nif(i==0) DFFE ff(.Q(src[i]),.D(serial_in),.CLK(clk),.CE(ce));\nelse DFFE ff(.Q(src[i]),.D(src[i-1]),.CLK(clk),.CE(ce));\nend endgenerate\n'
    conn = [f'.{n}({n})' for n in ('clk','ce','reset','in_valid')]
    for names, bus in ((inputs,'src'),(outputs,'sink')):
        offset = 0
        for n in names:
            conn.append(f'.{n}({bus}[{offset} +: {widths[n]}])')
            offset += widths[n]
    body += f'{module} dut('+','.join(conn)+');\n'
    for j in range(6):
        taps = ','.join(f'sink[{k}]' for k in range(j, ow, 6))
        body += f'DFFE observe{j}(.Q(leds[{j}]),.D(^{{{taps}}}),.CLK(clk),.CE(ce));\n'
    return body+'endmodule\n', dict(input_bits=iw, output_bits=ow, ports=ports)

def run(job):
    version, kind, place, mhz = job
    name = f'{version}-{kind}-p{place}-m{mhz}'
    work = OUT / name
    work.mkdir(parents=True, exist_ok=True)
    src = SOURCE / version / f'{kind}.v'
    original = src.read_bytes()
    original_sha = hashlib.sha256(original).hexdigest()
    rtl = original.decode().replace('\r\n','\n')
    wrap, ports = wrapper(rtl)
    (work/'dut.v').write_bytes(original)
    (work/'top.v').write_text(wrap)
    pins = ['IO_LOC "clk" 4;', 'IO_PORT "clk" IO_TYPE=LVCMOS33 PULL_MODE=UP BANK_VCCIO=3.3;']
    for j, pin in enumerate((15,16,17,18,19,20)):
        pins += [f'IO_LOC "leds[{j}]" {pin};',f'IO_PORT "leds[{j}]" IO_TYPE=LVCMOS33 DRIVE=8 BANK_VCCIO=3.3;']
    (work/'pins.cst').write_text('\n'.join(pins)+'\n')
    (work/'timing.sdc').write_text(f'create_clock -name clk -period {1000/mhz:.6f} [get_ports {{clk}}]\nreport_timing -setup -max_paths 40 -max_common_paths 5\n')
    tcl = [f'cd {{{work.as_posix()}}}', 'set_device -name GW2AR-18C -device_version C GW2AR-LV18QN88C8/I7']
    tcl += [f'add_file {{{(work/n).as_posix()}}}' for n in ('dut.v','top.v','pins.cst','timing.sdc')]
    tcl += ['set_option -synthesis_tool gowinsynthesis','set_option -top_module timing_top','set_option -output_base_name leaf','set_option -verilog_std v2001','set_option -gen_text_timing_rpt 1',f'set_option -place_option {place}','run all']
    (work/'run.tcl').write_text('\n'.join(tcl)+'\n')
    started=time.time()
    with (work/'gowin.log').open('w') as log:
        p=subprocess.run([GW,str(work/'run.tcl')],cwd=work,stdout=log,stderr=subprocess.STDOUT,timeout=900)
    r=dict(version=version,kind=kind,placement=place,mhz=mhz,returncode=p.returncode,seconds=round(time.time()-started,2),source_sha256=original_sha,snapshot_sha256=sha(work/'dut.v'),wrapper_sha256=sha(work/'top.v'),ports=ports)
    rpt=work/'impl/pnr/leaf.rpt.txt'; tr=work/'impl/pnr/leaf.tr'
    if rpt.exists() and tr.exists():
        rt=rpt.read_text(errors='replace'); tt=tr.read_text(errors='replace')
        r.update(report_sha256=sha(rpt),timing_sha256=sha(tr))
        for key,pattern in {'logic':r'^\s*Logic\s*\|\s*(\d+)', 'register':r'^\s*Register\s*\|\s*(\d+)', 'ssram':r'--SSRAM\(RAM16\)\s*\|\s*(\d+)'}.items():
            m=re.search(pattern,rt,re.M); r[key]=int(m.group(1)) if m else None
        for key,pattern in {'setup_violations':r'<Numbers of Setup Violated Endpoints>:(\d+)', 'hold_violations':r'<Numbers of Hold Violated Endpoints>:(\d+)', 'fmax':r'\bclk\s+[\d.]+\(MHz\)\s+([\d.]+)\(MHz\)'}.items():
            m=re.search(pattern,tt); r[key]=float(m.group(1)) if m else None
        r['resources']=[l for l in rt.splitlines() if re.search(r'Logic |Register |LUT|ALU|BSRAM|SSRAM|DSP|MULT9',l)]
    (work/'receipt.json').write_text(json.dumps(r,indent=2))
    print(name,r.get('logic'),r.get('register'),r.get('fmax'),r.get('setup_violations'),flush=True)
    return r
