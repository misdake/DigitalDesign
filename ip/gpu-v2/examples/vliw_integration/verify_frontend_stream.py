"""Bounded arithmetic/ownership replay of the adopted direct-output frontend.

The eight-credit behavioral sink is test stimulus, not the final physical pool.
No old transport or E-drive dependency is included in this replay.
"""
from pathlib import Path
import argparse
import hashlib
import importlib.util
import json
import os
import re
import shutil
import subprocess
import sys

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
SHA = lambda p: hashlib.sha256(Path(p).read_bytes()).hexdigest()


def check_events(path, packets):
    lines=path.read_text().splitlines()
    ends={int(a[2]):int(a[3],16) for line in lines if (a:=line.split())[0]=='E'}
    allocations = {}
    coverage = [set() for _ in packets]
    counts = dict(results=0, depth_events=0, lod=0, empty_quads=0)
    packet_headers = 0
    for line in lines:
        a = line.split()
        if a[0] == 'P':
            assert int(a[2]) == packet_headers
            assert int(a[3],16) == packets[packet_headers]['descriptor']
            packet_headers += 1
            continue
        seq = int(a[2])
        if a[0] == 'A':
            pid, slot, x, y = map(int, a[3:])
            assert seq == len(allocations) and 0 <= slot < 8
            assert 0 <= x < 200 and 0 <= y < 120
            qi=y*200+x;mask=ends[seq]
            expected=O.expected_quad(packets[pid],qi) if mask else None
            allocations[seq] = dict(packet=pid, qi=qi, mask=mask, expected=expected, ended=False,
                                    writes=set(), numeric=set(), lod=False)
            continue
        q = allocations[seq]
        p = packets[q['packet']]
        if a[0] == 'E':
            mask, h = int(a[3],16), int(a[4],16)
            assert not q['ended'] and q['mask']==mask
            q['ended']=True
            if mask:
                assert p['masks'].get(q['qi']) == mask
                assert q['qi'] not in coverage[q['packet']]
                coverage[q['packet']].add(q['qi'])
                e = q['expected']
                assert h == e['raw'] >> 21, ('raw h30', seq)
                q['expected'] = e
            else:
                assert q['qi'] not in p['masks']
                counts['empty_quads'] += 1
        elif a[0] == 'W':
            lane, word, got = int(a[3]), int(a[4]), int(a[5],16)
            assert (lane,word) not in q['writes'] and 0 <= lane < 4 and 0 <= word < 3
            q['writes'].add((lane,word))
            e = q['expected']
            if q['mask'] >> lane & 1:
                ordinal = (0,1,3,2).index(lane)
                want = e['words'][1+ordinal*3+word]
                mask = e['compare'][1+ordinal*3+word]
                if word == 0:
                    want |= ((e['passed'] >> lane) & 1) << 35
                assert (got&mask) == (want&mask), ('attribute',seq,lane,word,hex(got),hex(want),hex(mask))
            else:
                n = p['triangle'].normal[2]
                uv = p['triangle'].uv[2]
                want = [(n[0]&4095)|((n[1]&4095)<<12),(n[2]&4095)|(uv[0]<<12),uv[1]][word]
                mask = ((1<<36)-1) if word != 2 else (1<<18)-1
                assert (got&mask) == (want&mask), ('dummy attributes',seq,lane,word)
            counts['results'] += 1
        elif a[0] == 'N':
            lane,z,passed = map(int,a[3:])
            assert lane not in q['numeric']
            q['numeric'].add(lane)
            # Lane0's t13 numerical event legally precedes lane3's t14 end.
            e = q['expected']
            valid = bool(e and (e['passed'] >> lane & 1))
            assert bool(passed) == valid, ('numerical mask',seq,lane)
            if valid:
                x=q['qi']%200*2+(lane&1); y=q['qi']//200*2+(lane>>1)
                assert z == e['depth'][(x,y)], ('Z16',seq,lane)
            counts['depth_events'] += 1
        elif a[0] == 'L':
            assert not q['lod'] and q['mask'] is not None
            q['lod'] = True
            want = (q['expected']['words'][0] >> 19 & 255) if q['mask'] else 0
            assert int(a[3]) == want, ('final LOD',seq,a[3],want)
            counts['lod'] += 1
        else:
            raise AssertionError(a)
    assert packet_headers == len(packets)
    for i,p in enumerate(packets):
        assert coverage[i] == set(p['masks']), ('missing geometry',i)
    for seq,q in allocations.items():
        assert q['ended'] and len(q['writes']) == 12 and len(q['numeric']) == 4 and q['lod'], ('incomplete quad',seq)
    counts.update(quads=len(allocations), geometry_quads=sum(map(len,coverage)), packets=packet_headers)
    return counts


def main():
    global O
    ap=argparse.ArgumentParser(__doc__)
    ap.add_argument('--tag',required=True)
    ap.add_argument('--mode',choices=['portable','native','both'],default='both')
    ap.add_argument('--name',choices=['bunny_patch_1','box_x_minus','both'],default='both')
    ap.add_argument('--stress',action='store_true')
    ap.add_argument('--max-cycles',type=int,default=300000)
    a=ap.parse_args()
    assert re.fullmatch('[A-Za-z0-9_.-]+',a.tag)
    out=ROOT/'target/gpu-vliw-integration-20261018'/a.tag
    assert not out.exists(),'Use a fresh tag'
    out.mkdir(parents=True)
    vendor=ROOT/'ip/gpu-v2/src/frontend/vliw'
    files=[p for p in vendor.rglob('*') if p.suffix in ('.v','.vh','.json')]
    files += [HERE/'frontend_stream_tb.v',Path(__file__)] + [p for p in (HERE/'reference').rglob('*') if p.is_file()]
    source=out/'source'
    hashes={}
    for p in files:
        rel=p.relative_to(ROOT);dst=source/rel;dst.parent.mkdir(parents=True,exist_ok=True)
        shutil.copyfile(p,dst);hashes[rel.as_posix()]=SHA(dst);assert SHA(p)==SHA(dst)
    (out/'manifest.json').write_text(json.dumps(dict(source_sha256=hashes),indent=2))
    ref=source/HERE.relative_to(ROOT)/'reference'
    sp=importlib.util.spec_from_file_location('adopted_front_oracle',ref/'scene/oracle.py')
    O=importlib.util.module_from_spec(sp);sp.loader.exec_module(O)
    scene=json.loads((ref/'uv-inputs.json').read_text())['scenes'][0]
    rows=[]
    for instance in scene['meshlets']:
        if a.name!='both' and instance['name']!=a.name:continue
        data=out/instance['name'];meta,packets=O.prepare(instance,data)
        for mode in (['portable','native'] if a.mode=='both' else [a.mode]):
            run=data/mode;run.mkdir()
            for p in data.iterdir():
                if p.is_file():shutil.copyfile(p,run/p.name)
            tb=(source/HERE.relative_to(ROOT)/'frontend_stream_tb.v').read_text()
            fields=dict(DATA=run.as_posix(),NV=meta['vertices'],NT=meta['triangles'],NP=meta['packets'],MAXC=a.max_cycles,
                STRESS=int(a.stress),SLOT=meta['slot'],FAR=meta['draw']['far_raw'],TW=meta['draw']['tex_log'][0],
                TH=meta['draw']['tex_log'][1],MIP=meta['draw']['max_mip'],BIAS=meta['draw']['bias_q4'],MAXTIME=a.max_cycles*10+10000)
            for key,value in fields.items():tb=tb.replace('@'+key+'@',str(value))
            assert not re.search('@[A-Z]+@',tb)
            (run/'tb.v').write_text(tb)
            sv=source/vendor.relative_to(ROOT)
            cmd=[os.environ.get('IVERILOG_EXE','iverilog'),'-g2012','-s','tb','-I'+str(sv/'vertex'),'-o',str(run/'sim.vvp')]
            if mode=='portable':cmd+=['-DPORTABLE']
            else:cmd+=[str(Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v')]
            cmd+=list(map(str,sorted(sv.rglob('*.v'))))+[str(run/'tb.v')]
            for label,command,timeout in [('compile',cmd,60),('run',[os.environ.get('VVP_EXE','vvp'),str(run/'sim.vvp')],180)]:
                r=subprocess.run(command,cwd=run,capture_output=True,timeout=timeout)
                (run/(label+'.log')).write_bytes(r.stdout+r.stderr)
                assert r.returncode==0,(r.stdout+r.stderr).decode(errors='replace')[-4000:]
            assert b'PASS FRONTEND' in r.stdout
            counts=check_events(run/'events.txt',packets)
            report=dict(status='pass',name=instance['name'],mode=mode,stress=a.stress,counts=counts,
                event_sha256=SHA(run/'events.txt'),manifest_sha256=SHA(out/'manifest.json'),max_cycles=a.max_cycles,
                log=r.stdout.decode(errors='replace'),scope='Actual V/T/R/I arithmetic and eight old-state sink credits; no physical result pool or ROP')
            (run/'receipt.json').write_text(json.dumps(report,indent=2));rows.append(report)
            print(json.dumps(report),flush=True)
    if a.mode=='both':
        for name in set(r['name'] for r in rows):
            pair=[r for r in rows if r['name']==name];assert pair[0]['event_sha256']==pair[1]['event_sha256']
    (out/'receipt.json').write_text(json.dumps(dict(status='pass',source_manifest_sha256=SHA(out/'manifest.json'),runs=rows),indent=2))


if __name__=='__main__':main()
