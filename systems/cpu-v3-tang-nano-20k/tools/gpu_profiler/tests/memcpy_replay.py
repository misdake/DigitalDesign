"""Cold, synthetic RTT4 MEMCPY replay, including mutable-buffer output dumps."""
import argparse
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import snapshot as s
from replay import prepare, execute
from memcpy_fixture import make


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--rtl',type=Path,required=True)
    parser.add_argument('--out',type=Path,required=True)
    args = parser.parse_args()
    capture, original = make(s.Abi())
    path = prepare(capture,args.rtl,args.out,max_cycles=100000,timeout=180)
    result = execute(capture,path)
    assert result['command_abi']==s.Abi()['RTT400_COMMAND_ABI']
    assert (args.out/'capture/color.bin').read_bytes()==original[:512]+b'\x34\x12'*(102400-256)
    assert (args.out/'capture/depth.bin').read_bytes()==b'\xff\xff'*102400
    for item in result['raw_buffers']:
        want = b'\x69\x69'+original+b'\x96\x96' if item['address']==0x2ffffe else original[:512]
        assert (args.out/'capture'/item['file']).read_bytes()==want,item
    assert len(result['raw_buffers'])==2
    profile=json.loads((args.out/'profile/profile.json').read_text())
    copies=[c for c in profile['commands'] if c['opcode']==12]
    assert len(copies)==3 and all(c['end']>c['execute'] for c in copies)
    print('PASS RTT4 cold production replay: 3 copies, page crossing, live write/read lineage, exact planes and raw buffers',result['cold_submission_cycles'])


if __name__ == '__main__': main()
