"""Compare real backing mutations for the same tiny CB, preserving clock facts."""
import json
from pathlib import Path
import sys
from fractions import Fraction
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from event_protocol import validate


def check(emu_capture,emu_report,rtl_capture,rtl_report):
    events=[];profiles=[]
    for capture,report in [(emu_capture,emu_report),(rtl_capture,rtl_report)]:
        capture,report=Path(capture),Path(report)
        meta=validate(json.loads((capture/'capture.json').read_text()))
        assert meta['version']==2 and meta['clock_hz']==54000000
        rows=[json.loads(l) for l in (capture/'events.jsonl').read_text().splitlines()]
        end=next(t for t,k,v in rows if k=='cb_drain' and v==[1])
        writes=[(Fraction(*t),v) for t,k,v in rows if k=='physical_write' and Fraction(*t)<end]
        assert writes and all(a[0]<=b[0] for a,b in zip(writes,writes[1:]))
        events.append(writes);profiles.append(json.loads((report/'profile.json').read_text()))
    assert [v for t,v in events[0]]==[v for t,v in events[1]],'native address/data/mask mismatch'
    images=[]
    for profile in profiles:
        end=next(c for c in profile['commands'] if c['submission']==1 and c['opcode']==15)
        image=profile['images']['commands'][str(end['id'])]
        images.append([image[k] for k in ('plane_sha256','depth_plane_sha256')])
    assert images[0]==images[1],'native completion planes differ'
    result=dict(passed=True,physical_writes=len(events[0]),color_depth_sha256=images[0],
                first_write_time=[str(rows[0][0]) for rows in events],
                scope='same CB/resources and native memory mutation point; actual source timestamps retained; '
                      'different declared startup/CE/pin phases are not a timing equivalence claim')
    print(json.dumps(result))
    return result


if __name__=='__main__':check(*sys.argv[1:5])
