"""Audit replay identities, including removal of unused imported fixture caches."""
from pathlib import Path
import argparse
import hashlib
import json
import re

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[3]
OUT=ROOT/'target/gpu-vliw-integration-20261018'
SHA=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()


def main():
    ap=argparse.ArgumentParser(__doc__)
    ap.add_argument('--output',type=Path,default=OUT/'frontend-independent-audit.json')
    args=ap.parse_args()
    reference=HERE/'reference'
    origin=json.loads((reference/'origin.json').read_text())
    adopted_hashes=origin.get('adopted_source_sha256',origin['source_sha256'])
    for rel,h in adopted_hashes.items():assert SHA(reference/rel)==h
    clean=json.loads((reference/'uv-inputs.json').read_text())
    removed=origin['omitted_cache_fields']
    receipts=[]
    nonsemantic_adoptions={}
    for tag in ('frontend-stream-final-v1','frontend-stream-stress-v1'):
        base=OUT/tag
        manifest=json.loads((base/'manifest.json').read_text())
        for rel,h in manifest['source_sha256'].items():
            assert SHA(base/'source'/rel)==h
            if Path(rel).name not in ('uv-inputs.json','origin.json'):
                if SHA(ROOT/rel)!=h:
                    before=(base/'source'/rel).read_bytes();after=(ROOT/rel).read_bytes()
                    if Path(rel).is_relative_to(reference.relative_to(ROOT)):
                        # 只允许采用清单已登记的LF规范化与EOF空白整理。
                        local=Path(rel).relative_to(reference.relative_to(ROOT)).as_posix()
                        assert h==origin['source_sha256'][local] and SHA(ROOT/rel)==adopted_hashes[local]
                        assert before.replace(b'\r\n',b'\n').rstrip(b' \t\r\n')==after.replace(b'\r\n',b'\n').rstrip(b' \t\r\n'),('non-whitespace adoption',rel)
                    else:
                        # 此胶合文件只改两条注释及一条已知诊断文字，不能容许控制变化。
                        assert Path(rel).as_posix()=='ip/gpu-v2/src/frontend/vliw/pipeline_stream.v',('changed tested source',rel)
                        old='模块排空未涵盖导入或外部深度事务';new='模块排空未涵盖导入'
                        normalized=lambda b:re.sub(r'\s+','',re.sub(r'//[^\n]*|/\*.*?\*/','',b.decode('utf8'),flags=re.S))
                        assert normalized(before).replace(old,new)==normalized(after),('changed runtime tokens',rel)
                    nonsemantic_adoptions[rel]=dict(replayed_sha256=h,adopted_sha256=SHA(ROOT/rel))
        old=json.loads((base/'source'/reference.relative_to(ROOT)/'uv-inputs.json').read_text())
        for s in old['scenes']:
            for i in s['meshlets']:
                for k in removed:i.pop(k,None)
        assert old==clean,'Only unused metadata may differ from the actually replayed inputs'
        result=json.loads((base/'receipt.json').read_text())
        assert result['status']=='pass' and result['source_manifest_sha256']==SHA(base/'manifest.json')
        for r in result['runs']:
            event=base/r['name']/r['mode']/'events.txt'
            assert SHA(event)==r['event_sha256']
            assert r['status']=='pass' and r['max_cycles']==300000
        receipts.append(dict(path=str(base/'receipt.json'),sha256=SHA(base/'receipt.json'),runs=result['runs']))
    for name in ('bunny_patch_1','box_x_minus'):
        for filename in ('quads.jsonl','packets.json','vertices.hex','words.hex','descriptors.hex','dma.txt'):
            assert SHA(OUT/'uv-inputs-selfcontained'/name/filename)==SHA(OUT/'uv-inputs-final'/name/filename)
    adopted=ROOT/'ip/gpu-v2/src/frontend'
    vendor=json.loads((adopted/'vliw/origin.json').read_text())
    for rel,h in vendor['files'].items():assert SHA(adopted/rel)==h['adopted_sha256']
    result=dict(status='pass',auditor_sha256=SHA(__file__),
        copied_reference_sha256=adopted_hashes,upstream_reference_sha256=origin['source_sha256'],adopted_arithmetic=vendor,
        nonsemantic_adoptions=nonsemantic_adoptions,
        replay_receipts=receipts,
        raw_input_equivalence='Replay snapshots include unused old caches. Current input deletes only the listed cache fields. All remaining source objects are equal and regenerated integer/DMA outputs are byte-identical.',
        omitted_unused_cache_fields=removed,
        coverage='Two actual varying-UV inputs, P/N steady and CE/long sink pauses; 8-credit fullness and repeated wrap; all V96/raw19/h30/mask/result/depth-valid/final LOD events; dummy and empty geometry quads',
        not_covered='No RGB switch, midflight reset, physical pool, sampler/Lighting/ROP or GPU/system/board validation')
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(result,indent=2))
    print(json.dumps(dict(status='pass',runs=sum(len(r['runs']) for r in receipts),output=str(args.output)),indent=2))


if __name__=='__main__':main()
