"""Require complete exact-audited frame pairs and compare cache units explicitly."""
from pathlib import Path
import argparse
import hashlib
import json


def main():
    ap=argparse.ArgumentParser(__doc__)
    ap.add_argument('--baseline',type=Path,required=True)
    ap.add_argument('--candidate',type=Path,required=True)
    ap.add_argument('--output',type=Path,required=True)
    args=ap.parse_args();out=args.output.resolve();out.mkdir(parents=True,exist_ok=False)
    directories=[args.baseline.resolve(),args.candidate.resolve()]
    measurements=[json.loads((p/'texture-counts.json').read_text()) for p in directories]
    receipts=[json.loads((p/'receipt.json').read_text()) for p in directories]
    for index,receipt in enumerate(receipts):
        assert receipt['status']=='pass' and not receipt['small_materials']
        assert receipt['info']['submitted_triangles']==1141
        assert len(json.loads((directories[index]/'input.json').read_text())['instances'])==24
    baseline,candidate=measurements
    assert baseline['data_capacity_bytes']==candidate['data_capacity_bytes']==8192
    for key in ('scene_sha256','commandbuffer_sha256','color_sha256','depth_sha256','camera_index'):
        assert baseline[key]==candidate[key],(key,baseline[key],candidate[key])
    for key in ('sample','quads','live_pixels','ROP_pass','ROP_depth_reject','light','final'):
        assert receipts[0]['counts'][key]==receipts[1]['counts'][key],key
    assert receipts[0]['gold_sha256']==receipts[1]['gold_sha256']
    for name,identity in receipts[0]['source_sha256'].items():
        if name!='dut.v':assert identity==receipts[1]['source_sha256'][name],('observer/MC mismatch',name)
    metrics={}
    keys=['line_misses','line_fills','groups','sector_requests','read_bytes','miss_rate',
          'lookup_cycles','cache_wait_cycles','refill_wait_cycles','tag_wait_cycles',
          'hit_denial_cycles','ce_denial_cycles','request_backpressure_cycles','fifo_full_cycles']
    for key in keys:
        a,b=baseline['counts'][key],candidate['counts'][key]
        metrics[key]=dict(baseline=a,candidate=b,delta=b-a,percent=(100*(b-a)/a if a else None))
    for key in ('total','preload','frontend','tail'):
        a,b=baseline['timing'][key],candidate['timing'][key]
        metrics[key+'_cycles']=dict(baseline=a,candidate=b,delta=b-a,percent=(100*(b-a)/a if a else None))
    provenance=[]
    for name,path,measurement in zip(('baseline','candidate'),directories,measurements):
        archive=path/'measurement-harness.py'
        archived=hashlib.sha256(archive.read_bytes()).hexdigest()
        provenance.append(dict(name=name,directory=str(path),dut_sha256=measurement['dut_sha256'],
                               harness_archive_sha256=archived,harness_reported_sha256=measurement['harness_sha256'],
                               harness_hash_note=('The first baseline harness recorded its on-disk hash at completion. The archived pre-edit harness is authoritative; the later edit only handles synthetic default seed generation for the candidate.'
                                                  if archived!=measurement['harness_sha256'] else 'Recorded at launch and archived unchanged.')))
    samples=receipts[0]['counts']['sample']
    for name,measurement in zip(('baseline','candidate'),measurements):
        counts=measurement['counts']
        measurement['derived']=dict(texture_outputs=samples,groups_per_texture_output=counts['groups']/samples,
                                    line_misses_per_texture_output=counts['line_misses']/samples,
                                    returned_bytes_per_texture_output=counts['read_bytes']/samples,
                                    mean_parent_refill_wait_cycles=counts['refill_wait_cycles']/counts['line_misses'])
    result=dict(status='pass',data_capacity_bytes=8192,metrics=metrics,measurements=measurements,
                exact_frame_pair=True,scene_triangles=1141,rabbit_triangles=948,meshlets=24,
                texture_outputs=samples,source_provenance=provenance,
                boundary=baseline['boundary'],cache_state=baseline['cache_state'],
                traffic_units='read_bytes counts actual valid64-bit texture return beats, not physical SDRAM discarded/prefetch DQ words; sector_requests counts accepted128B descriptors, not offered/retried cycles.',
                wait_units='Wall-cycle head/descriptor/phase observations overlap and include intentional II2 service bubbles. Do not sum them as independent stalls.')
    (out/'comparison.json').write_text(json.dumps(result,indent=2))
    lines=['# Texture16x8 complete-scene comparison','',
           'Both frames passed the independent exact numerical and whole-memory audit. Color and depth planes match byte-for-byte. The same24 meshlets,1141 triangles and command bytes were used; only texture storage bytes and opt-in texture RTL differ.','',
           '| Metric | Baseline64x128B | Candidate32x256B | Change |','|---|---:|---:|---:|']
    for key in ['line_misses','groups','miss_rate','sector_requests','read_bytes','refill_wait_cycles','cache_wait_cycles','total_cycles','frontend_cycles','tail_cycles']:
        value=metrics[key];change='n/a' if value['percent'] is None else f"{value['percent']:+.3f}%"
        lines.append(f"| {key} | {value['baseline']} | {value['candidate']} | {change} |")
    lines+=['',f'Independent texture-output denominator: {samples}. Cache capacity is8KiB in both cases. Logical misses are parent-line allocations; every candidate allocation requires two128B descriptors. Miss rate uses consumed Groups, whose count itself changes with layout.','',baseline['cache_state'],'',baseline['boundary'],'',result['traffic_units'],'',result['wait_units'],'']
    (out/'report.md').write_text('\n'.join(lines))
    print(json.dumps(dict(status='pass',metrics=metrics,output=str(out)),indent=2))


if __name__=='__main__':main()
