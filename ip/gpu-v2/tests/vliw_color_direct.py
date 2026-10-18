"""Bounded direct-completion Color replay against the retained FIFO reference."""
from pathlib import Path
import argparse,hashlib,json,os,re,subprocess
def module(s,n):return re.search(r'\bmodule '+n+r'\b.*?endmodule',s,re.S)[0]
TB = Path(__file__).with_name("vliw_color_direct_tb.v").read_text()

def main():
    ap=argparse.ArgumentParser();ap.add_argument('--exports',type=Path,required=True);ap.add_argument('--output',type=Path,required=True);ap.add_argument('--reference',type=Path,required=True)
    a=ap.parse_args();a.output.mkdir(parents=True,exist_ok=False)
    reference=a.reference.read_text();receipts=[]
    for mode in ['portable','native']:
        d=a.output/mode;d.mkdir()
        active=(a.exports/f'backend-{mode}.v').read_text()
        text=('`define GPU_V2_GOWIN_DSP\n' if mode=='native' else '')+module(reference,'gpu_v2_texture_runtime_color_checked').replace('gpu_v2_texture_runtime_color_checked','gold',1)+'\n'+module(active,'gpu_v2_texture_runtime_color_checked').replace('gpu_v2_texture_runtime_color_checked','candidate',1)
        for n in ['color_serial_registered_mul9','color_packed_accumulator']:text+='\n'+module(reference,n)
        (d/'dut.v').write_text(text);(d/'tb.v').write_text(TB)
        cmd=[os.environ.get('IVERILOG_EXE','iverilog'),'-g2012','-s','tb','-o',str(d/'sim.vvp'),str(d/'dut.v'),str(d/'tb.v')]
        if mode=='native':cmd+=[str(Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v')]
        for label,args in [('compile',cmd),('run',[os.environ.get('VVP_EXE','vvp'),str(d/'sim.vvp')])]:
            p=subprocess.run(args,cwd=d,capture_output=True,timeout=90);(d/f'{label}.log').write_bytes(p.stdout+p.stderr)
            assert p.returncode==0,(p.stdout+p.stderr).decode(errors='replace')[-3000:]
        assert 'PASS color miter' in p.stdout.decode()
        print(mode,p.stdout.decode(),flush=True)
        receipts.append(dict(mode=mode,passed=True,summary=p.stdout.decode(),source_sha256=hashlib.sha256(text.encode()).hexdigest()))
    (a.output/'receipt.json').write_text(json.dumps(receipts,indent=2))
if __name__=='__main__':main()
