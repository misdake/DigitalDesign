"""Compare matched8KiB row/column full-scene observations and canonical pixels."""
from pathlib import Path
import argparse,hashlib,json,re


def main():
    ap=argparse.ArgumentParser(__doc__);ap.add_argument('--baseline',type=Path,required=True);ap.add_argument('--candidate',type=Path,required=True);ap.add_argument('--output',type=Path,required=True);a=ap.parse_args()
    directories=[a.baseline.resolve(),a.candidate.resolve()];measurements=[json.loads((p/'texture-counts.json').read_text()) for p in directories];receipts=[json.loads((p/'receipt.json').read_text()) for p in directories]
    legacy_row=measurements[0]['layout']=='16x8'
    assert [m['layout'] for m in measurements] in (['16x8-row','16x8-column16'],['16x8','16x8-column16'])
    if legacy_row:
        assert 'observation_reuse' in receipts[0], 'legacy row must first pass byte-exact trace re-audit'
        assert receipts[0]['dut_sha256']==hashlib.sha256((directories[0]/'dut.v').read_bytes()).hexdigest()
    for p,m,r in zip(directories,measurements,receipts):
        assert r['status']=='pass' and r['native'] and not r['small_materials']
        assert r['info']['submitted_triangles']==1141 and len(json.loads((p/'input.json').read_text())['instances'])==24
        assert (m['data_capacity_bytes'],m['resident_lines'],m['sets'],m['ways'])==(8192,32,8,4)
        assert m['counts']['sector_requests']==2*m['counts']['line_misses']
        assert m['counts']['read_bytes']==128*m['counts']['sector_requests']
        if 'layout_harness_sha256' in m:
            assert hashlib.sha256((p/'layout-harness.py').read_bytes()).hexdigest()==m['layout_harness_sha256']
        assert 'texture_fb400240' not in (p/'dut.v').read_text(), 'pure layout comparison must not include RTT view'
    left,right=measurements
    for key in ['scene_sha256','commandbuffer_sha256','color_sha256','depth_sha256','camera_index']:
        assert left[key]==right[key],(key,left[key],right[key])
    if left['harness_sha256']!=right['harness_sha256']:
        sources=[(p/'measurement-harness.py').read_text() for p in directories]
        old='Complete recorded demo command replay, real GPU/MC/pins, normal54/108 timing; CPU and concurrent Display excluded. No board proof.'
        new='Complete recorded demo replay, real GPU/MC/pins at frozen TB approximately49.5/99MHz CL2; CPU and concurrent Display excluded. No54/108MHz or board qualification claim.'
        assert sources[0].count(old)==1 and sources[1].count(new)==1
        assert sources[0].replace(old,'SCOPE_CORRECTION')==sources[1].replace(new,'SCOPE_CORRECTION'), 'only known scope label may differ'
    assert receipts[0]['gold_sha256']==receipts[1]['gold_sha256']
    for key in ['sample','quads','live_pixels','ROP_pass','ROP_depth_reject','light','final']:
        assert receipts[0]['counts'][key]==receipts[1]['counts'][key],key
    assert receipts[0]['source_sha256'].keys()==receipts[1]['source_sha256'].keys()
    for key,value in receipts[0]['source_sha256'].items():
        if key not in ('dut.v','tb.v'):assert receipts[1]['source_sha256'][key]==value,('MC mismatch',key)
    normalized=[]
    adapters=[]
    for p in directories:
        text=(p/'tb.v').read_text()
        if 'LAYOUT_ADAPTER:' in text:
            declaration=re.search(r'\n// LAYOUT_ADAPTER:.*?endfunction\nfunction integer physical_live;.*?endfunction\n',text,re.S)
            assert declaration is not None
            assert not re.search(r'always|initial|posedge|negedge|<=',declaration[0])
            adapters.append(hashlib.sha256(declaration[0].encode()).hexdigest())
            text=text[:declaration.start()]+text[declaration.end():]
            guard=re.search(r' layout_pad_words=0;.*?\$display\("PASS PADDING.*?;\n',text,re.S)
            assert guard is not None
            assert not re.search(r'seed_word|<=|@|#|posedge|negedge',guard[0]), 'guard must be a pure post-frame memory read'
            assert text.index('frame_end=cycle;')<guard.start()
            text=text[:guard.start()]+text[guard.end():]
            old="if(j<TEX)raw_seed_addr=32'h1000+j;else if(j<TEX+FB)raw_seed_addr=32'h100000+framebuffer_offset(j-TEX);else raw_seed_addr=32'h140000+framebuffer_offset(j-TEX-FB);"
            new="if(j<TEX)raw_seed_addr=32'h1000+j;else if(j<TEX+FB)raw_seed_addr=32'h100000+j-TEX;else raw_seed_addr=32'h140000+j-TEX-FB;"
            assert text.count(old)==1 and text.index(old)>text.index('frame_end=cycle;')
            text=text.replace(old,new)
        else:
            assert legacy_row and p==directories[0]
        normalized.append(hashlib.sha256('\n'.join(line.strip() for line in text.splitlines() if line.strip()).encode()).hexdigest())
    assert normalized[0]==normalized[1],('timed stimulus/counters or canonical dump differ',normalized)
    assert left['definitions']==right['definitions']
    metrics={}
    for key in ['line_misses','line_fills','groups','sector_requests','read_bytes','miss_rate','lookup_cycles','cache_wait_cycles','refill_wait_cycles','tag_wait_cycles','hit_denial_cycles','ce_denial_cycles','request_backpressure_cycles','fifo_full_cycles']:
        x,y=left['counts'][key],right['counts'][key];metrics[key]=dict(row=x,column=y,delta=y-x,percent=(100*(y-x)/x if x else None))
    for key in ['total','preload','frontend','tail']:
        x,y=left['timing'][key],right['timing'][key];metrics[key+'_cycles']=dict(row=x,column=y,delta=y-x,percent=(100*(y-x)/x if x else None))
    for key in ['RO','FBread','FBwrite','RObeats','FBreadbeats','FBwritebeats']:
        x,y=left['timing'][key],right['timing'][key];metrics[key]=dict(row=x,column=y,delta=y-x,percent=(100*(y-x)/x if x else None))
    for label,keys in [('all_read_bytes',('RObeats','FBreadbeats')),('all_write_bytes',('FBwritebeats',))]:
        x,y=[8*sum(m['timing'][k] for k in keys) for m in measurements]
        metrics[label]=dict(row=x,column=y,delta=y-x,percent=(100*(y-x)/x if x else None))
    result=dict(status='pass',data_capacity_bytes=8192,cache_geometry='32x256B,8sets,4ways',scene_triangles=1141,rabbit_triangles=948,meshlets=24,
        texture_outputs=receipts[0]['counts']['sample'],metrics=metrics,measurements=measurements,
        canonical_color_depth_equal=True,comparison='Both exact numerical/whole-image audits pass. Column physical padding and allocation slack are checked separately against guards; the byte-identical legacy row trace is re-audited when reused.',
        matched='Same commands, scene, camera, frozen numerical executable, MC sources and counter definitions. Timed TBs match after removing only combinational readback functions, post-frame guards, and the canonical framebuffer readback address adapter. Legacy row trace reuse additionally requires a fresh byte-exact DUT/TB/fixture/MC identity audit.', normalized_tb_without_readonly_padding_sha256=normalized[0],
        boundary=right['boundary'],cache_state=left['cache_state'],
        traffic_units='Logical parent allocations are misses; every parent fills two128B sectors. read_bytes counts actual valid64-bit texture return data. all_read_bytes adds valid RO and FB-read64-bit beats; all_write_bytes counts FB-write64-bit beats, including CLEAR. Column CLEAR intentionally adds200 sectors/25600 bytes across both padded planes. Wait-cycle observations overlap; do not add them.')
    out=a.output.resolve();out.mkdir(parents=True,exist_ok=False);(out/'comparison.json').write_text(json.dumps(result,indent=2))
    rows=['# Macro row/column complete-scene comparison','','Both caches retain8KiB. All24 meshlets and1141 triangles are included;948 triangles belong to the rabbit. Commands and canonical color/depth match exactly.','','| Metric | Row | Column | Change |','|---|---:|---:|---:|']
    for key in ['line_misses','groups','miss_rate','sector_requests','read_bytes','refill_wait_cycles','cache_wait_cycles','fifo_full_cycles','total_cycles','frontend_cycles','tail_cycles','all_read_bytes','all_write_bytes']:
        v=metrics[key];change='n/a' if v['percent'] is None else f"{v['percent']:+.3f}%";rows.append(f"| {key} | {v['row']} | {v['column']} | {change} |")
    rows += ['',result['matched'],'',result['comparison'],'',result['traffic_units'],'',result['boundary'],'']
    (out/'report.md').write_text('\n'.join(rows));print(json.dumps({'status':'pass','metrics':metrics,'output':str(out)},indent=2))


if __name__=='__main__':main()
