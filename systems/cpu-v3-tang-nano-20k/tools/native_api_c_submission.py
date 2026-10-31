"""Actual capture -> existing production GPU/MC replay, then CPU memory handback."""
import argparse
import hashlib
import json
from pathlib import Path
import sys
sys.path.insert(0,str(Path(__file__).resolve().parent/'gpu_profiler'))
from snapshot import Abi, encode_reference, decode_stream
from replay import run


def main():
    parser=argparse.ArgumentParser(__doc__)
    for name in ('capture','build','program','recipe','rtl','out'):
        parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--max-cycles',type=int,default=500_000)
    parser.add_argument('--timeout',type=int,default=300)
    args=parser.parse_args()
    raw=json.loads(args.capture.read_text());rows=[raw['ranges'][i:i+8]for i in range(0,len(raw['ranges']),8)];abi=Abi()
    captures,report=decode_stream(encode_reference(raw['header'],rows,[bytes(b)for b in raw['blobs']],abi),abi)
    if report['failures'] or len(captures)!=1:raise RuntimeError('Actual C capture failed decoding')
    capture=captures[0];args.out.mkdir(parents=True,exist_ok=False);capture.save(args.out/'frozen')
    source=dict(schema=1,source='system-simulation',boundary='before-profiler-gate',gpu_consumed=False,input_sha256=capture.manifest()['input_sha256'],compiled_program_sha256=hashlib.sha256(args.program.read_bytes()).hexdigest(),recipe_sha256=hashlib.sha256(args.recipe.read_bytes()).hexdigest(),scope='actual CPU callback; CPU cannot retire until this production GPU/MC replay completes; cold per submission, no physical CPU cache timing')
    provenance=args.out/'system-source.json';provenance.write_text(json.dumps(source))
    result=run(args.out/'frozen/snapshot.json',args.rtl,args.out/'replay',args.max_cycles,args.timeout,build=args.build,simulation_source=provenance)
    handback=[]
    for row in result['write_ranges']:
        handback.append(dict(address=row['address'],bytes=row['bytes'],file=str((args.out/'replay/capture'/row['file']).resolve()),sha256=row['sha256']))
    (args.out/'memory-handback.json').write_text(json.dumps(dict(complete=True,cycles=result['cold_submission_cycles'],writes=handback,scope=source['scope']),indent=2))
    print(json.dumps(dict(complete=True,cycles=result['cold_submission_cycles'],writes=len(handback))))


if __name__=='__main__':main()
