"""Source-hashed registered full-postquad fit. No board access.

Usage: fit.py SOURCE OUTPUT 0|1. Set GOWIN_HOME or GW_SH_EXE.
"""
from pathlib import Path
import hashlib, json, os, re, sys
sys.dont_write_bytecode = True
import fit_helper as fit
from wrapper import wrapper

source, output = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve()
placement = int(sys.argv[3])
assert placement in (0,1) and not output.exists()
fit.OUT, fit.SOURCE, fit.wrapper = output/'fits', output/'source', wrapper
fit.GW = os.environ.get('GW_SH_EXE') or str(Path(os.environ['GOWIN_HOME'])/'IDE/bin/gw_sh.exe')
incoming = fit.SOURCE/'selected'
incoming.mkdir(parents=True)
rtl = source.read_text()
assert rtl.count('module gpu_v2_postquad(') == 1
(incoming/'native.v').write_text(rtl.replace('module gpu_v2_postquad(', 'module probe(', 1))
result = fit.run(('selected','native',placement,54))
report = fit.OUT/f'selected-native-p{placement}-m54'/'impl/pnr/leaf.rpt.txt'
if report.exists():
    text = report.read_text(errors='replace')
    result['primitives'] = {k:int(v) for k,v in re.findall(r'--(MULT\w+|ALU54D)\s*\|\s*(\d+)',text)}
    for key,pattern in [('logic_ff',r'--Logic Register as FF\s*\|\s*(\d+)'),('bsram',r'^\s*BSRAM\s*\|\s*([\d.]+)'),('dsp',r'^\s*DSP\s*\|\s*([\d.]+)')]:
        match = re.search(pattern,text,re.M)
        result[key] = float(match[1]) if match else None
result.update(original_source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(), wrapper_generator_sha256=hashlib.sha256(Path(__file__).with_name('wrapper.py').read_bytes()).hexdigest(), scope='Complete registered post-interpolator chain only; geometry/CPU/board excluded.')
(output/'receipt.json').write_text(json.dumps(result, indent=2))
print(json.dumps(result, indent=2))
