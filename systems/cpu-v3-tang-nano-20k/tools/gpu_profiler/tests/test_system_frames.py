"""Finite event fixtures: official frame export, scope and carry conservation."""
import copy
import csv
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
import reporting
import system_emu
import system_frames


class FrameReports(unittest.TestCase):
    def test_html_projection_preserves_visible_facts_and_crossing_baselines(self):
        from model import viewer_model
        d=self.build('compact',frames=[1,2]);frame=system_frames.make(d,2)
        before=json.dumps(frame,sort_keys=True);v=viewer_model(frame)
        self.assertEqual(json.dumps(frame,sort_keys=True),before)
        self.assertEqual(v['commands'],[c for c in frame['commands'] if c['submission'] in frame['system']['frame_view']['owned_submission_ids']])
        f=frame['system']['frame_view']
        self.assertEqual(v['system']['client_payload'],[p for p in frame['system']['client_payload'] if f['start_tick']<=p[0]<f['end_tick']])
        self.assertTrue(any(t['start_tick']<f['start_tick'] for t in v['system']['transactions']))
        out=self.root/'compact/frames/frame-2'
        html=(out/'profiler.html').read_text();self.assertIn('viewer_projection',html)

    def test_runner_sidecar_is_bound_in_formal_build_and_scoped_exports(self):
        plan=dict(schema='digitaldesign.ordinary-profile-plan',version=1,
                  gates=dict(before_input=[7,1],consumed=[7,2],sample=[7,3]))
        events=[dict(kind='input',tick=0,buttons_active_low=65535),
                dict(kind='gate',name='sample',tick=148,pc=[7,3],next_present=1,state=dict(phase=2))]
        for f in self.scanouts[1:]:
            events.append(dict(kind='raster',tick=f['end_tick'],start_tick=f['start_tick'],
                               presentation=f['applied_present_count'],front_halfword=f['front_at_start_halfword'],effective_pixels=384000))
        self.meta.update(runner_version=1,runner_files=['runner-plan.json','runner-events.jsonl'],
                         runner_sidecar_events=len(events),runner_selected_presentations=[1,2],
                         event_limit=112000000,event_byte_limit=4294967296,
                         event_bytes=(self.capture/'system-events.jsonl').stat().st_size)
        (self.capture/'runner-plan.json').write_text(json.dumps(plan))
        (self.capture/'runner-events.jsonl').write_text(''.join(json.dumps(e)+'\n' for e in events));self.save()
        d=self.build('runner',frames=[1,2]);system_emu.validate(d)
        out=self.root/'runner'
        scoped=json.loads((out/'frames/frame-1/runner-frame-timings.json').read_text())
        self.assertEqual(scoped['frame_states'],[dict(presentation=1,tick=148,state=dict(phase=2))])
        self.assertIn('runner-plan.json',d['provenance']['report_inputs']['files'])
        bad=copy.deepcopy(d);bad['system']['runner']['events'][-1]['front_halfword']+=1
        with self.assertRaisesRegex(ValueError,'raster not bound'):system_emu.validate(bad)

    def setUp(self):
        temp_root = ROOT.parents[3]/'target/frame-reports/tests'
        temp_root.mkdir(parents=True, exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(dir=temp_root)
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.capture = self.root/'capture'; self.capture.mkdir()
        (self.capture/'program.bin').write_bytes(b'synthetic-frame-fixture')
        (self.capture/'source-executable.bin').write_bytes(b'not-a-DUT-run')
        self.rows = [[0,'state',[0]*10],[0,'cpu',[0,0]],
                     [148,'submit',1],[296,'gpu_done',1],[444,'present_request',1],
                     [592,'present_applied',[1,220]], [592,'cpu',[10,1]],
                     [592,'accept',[2,100,False,1]],[592,'state',[0]*9+[4]],
                     [740,'cpu',[12,2]],[740,'submit',2],[740,'payload',[2,8,0]],
                     [814,'native',[4,0,0]],[851,'native',[0,4,3]],
                     [888,'gpu_done',2],[1036,'present_request',2],[1036,'cpu',[16,3]],
                     [1036,'terminal',[2,False]],[1036,'state',[0]*10],
                     [1184,'present_applied',[2,200]],[1184,'cpu',[18,4]],
                     [1184,'accept',[1,200,False,1]],[1184,'payload',[1,8,0]],
                     [1332,'submit',3],[1332,'cpu',[20,5]],
                     [1332,'accept',[4,300,False,1]],[1332,'payload',[4,8,0]],
                     [1332,'terminal',[4,False]],[1480,'terminal',[1,False]],
                     [1480,'gpu_done',3],[1628,'present_request',3]]
        self.scanouts = []
        for index, (number, start, end, applied, front, effective) in enumerate([
                (0,148,444,0,200,0),(7,740,1036,1,220,384000),(9,1332,1628,2,200,384000)]):
            name=f'raster-{index}.rgb'; (self.capture/name).write_bytes(bytes([index])*1152000)
            self.scanouts.append(dict(file=name,frame=number,start_tick=start,end_tick=end,
                                     applied_present_count=applied,front_at_start_halfword=front,
                                     effective_pixels=effective,present_changed_during_scanout=False,
                                     present_id=None,source='registered-rgb'))
        self.meta=dict(schema=system_emu.SOURCE,version=1,source_kind='system-cycle-emu',
                       ports=system_emu.PORTS,quantum_hz=7992000000,logic_period=148,
                       origin_tick=0,end_tick=1776,coverage_end_tick=1776,reached=True,
                       truncated=False,events=len(self.rows),scanouts=self.scanouts,
                       capabilities=dict(client_payload64=True,native_model32=True,
                                         instruction_owner_lineage=False,pin_electrical=False))
        self.save()
        self.identity=self.root/'identity.json'
        self.identity.write_text(json.dumps(dict(source='8'*40,tree='9'*40,
            boot_sha256=hashlib.sha256((self.capture/'program.bin').read_bytes()).hexdigest(),
            runtime_sha256=hashlib.sha256((self.capture/'source-executable.bin').read_bytes()).hexdigest())))

    def save(self):
        self.meta['events']=len(self.rows)
        (self.capture/'capture.json').write_text(json.dumps(self.meta))
        (self.capture/'system-events.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in self.rows))

    def build(self, name='report', **kwargs):
        return reporting.build_report(self.capture,self.root/name,**kwargs)

    def test_only_gpu_frame_needs_no_applied_present_or_scanout(self):
        canonical=self.build('full')
        d=self.build('only',only_frame=3)
        f=d['system']['frame_view']
        self.assertEqual(f['owned_submission_ids'],[3])
        self.assertEqual((f['start_tick'],f['end_tick']),(1332,1480))
        self.assertIsNone(f['scanout_index'])
        with self.assertRaisesRegex(ValueError,'no complete scanout or native image'):self.build('no-images',only_frame=3,require_images=True)
        self.assertEqual(d['system']['transactions'],canonical['system']['transactions'])
        from model import viewer_model
        v=viewer_model(d)
        self.assertEqual([s['id'] for s in v['system']['submissions']],[3])
        self.assertTrue(all(c['submission']==3 for c in v['commands']))
        self.assertFalse((self.root/'only/scanout-selected.png').exists())
        out=self.root/'rerender'
        reporting.render_report(self.root/'only/profile.json',out)
        result=subprocess.run(['node',str(ROOT/'tests/system_viewer_checks.js'),str(out/'profile.json')],
                              capture_output=True,text=True,timeout=20)
        self.assertEqual(result.returncode,0,result.stderr)
        with self.assertRaisesRegex(ValueError,'not applied'):self.build('strict',frames=[3])
        for number in (0,-1,True,4):
            with self.assertRaises(ValueError):system_frames.make_gpu(canonical,number)
        d['system']['frame_view']['owned_submission_ids']=[2]
        with self.assertRaisesRegex(ValueError,'differs'):system_emu.validate(d)

    def test_only_frame_metadata_and_multiple_cbs(self):
        self.rows.insert(-1,[1480,'submit',4])
        self.rows.insert(-1,[1628,'gpu_done',4])
        self.rows[-1][0]=1628
        self.meta['profile_frame']=3;self.save()
        d=self.build('selected')
        self.assertEqual(d['system']['frame_view']['owned_submission_ids'],[3,4])
        with self.assertRaisesRegex(ValueError,'differs'):self.build('conflict',only_frame=2)
        with self.assertRaisesRegex(ValueError,'mutually exclusive'):self.build('both',frames=[1],only_frame=3)

    def test_frame_scope_keeps_raw_context_but_filters_output_and_statistics(self):
        canonical=self.build(frames=[1,2],source_identity=self.identity)
        self.assertNotIn('frame_view',canonical['system'])
        frame=json.loads((self.root/'report/frames/frame-2/profile.json').read_text())
        f=frame['system']['frame_view']
        self.assertEqual([f['start_tick'],f['end_tick']],[740,1332])
        self.assertEqual(f['owned_submission_ids'],[2])
        self.assertEqual(f['scanout_index'],2)  # Display raster9, not application2.
        self.assertEqual(f['summary']['client_payload_bytes'][2],[8,0])
        self.assertEqual(f['summary']['client_payload_bytes'][1],[8,0])
        self.assertEqual(f['summary']['client_payload_bytes'][4],[0,0])
        self.assertEqual(f['summary']['native_model32_bytes'],[4,4,3])
        self.assertEqual(f['summary']['cpu_counter']['retired_words'],8)
        self.assertEqual((f['summary']['carry_in'],f['summary']['carry_out']),(1,1))
        self.assertEqual(frame['system']['transactions'],canonical['system']['transactions'])
        self.assertEqual(frame['system']['presents'],canonical['system']['presents'])
        self.assertFalse(frame['partial'])
        folder=self.root/'report/frames/frame-2'
        with (folder/'system-transactions.csv').open() as stream:tx=list(csv.DictReader(stream))
        self.assertEqual(len(tx),2)
        self.assertEqual(tx[0]['start_tick'],'592');self.assertEqual(tx[0]['visible_start_tick'],'740')
        self.assertEqual(tx[1]['end_tick'],'1628');self.assertEqual(tx[1]['visible_end_tick'],'1332')
        self.assertEqual(tx[1]['carry_out'],'True')  # No fabricated terminal at1332.
        self.assertEqual(len(list(folder.glob('*.png'))),1)
        self.assertIn('not captured by this legacy system source',(folder/'profiler.html').read_text())
        for profile in (self.root/'report/profile.json', folder/'profile.json'):
            result=subprocess.run(['node',str(ROOT/'tests/system_viewer_checks.js'),str(profile)],
                                  capture_output=True,text=True,timeout=20)
            self.assertEqual(result.returncode,0,result.stderr)

    def test_frame_tampering_and_missing_scanout_are_rejected(self):
        canonical=self.build()
        frame=system_frames.make(canonical,2)
        frame['system']['frame_view']['summary']['carry_out']=0
        with self.assertRaisesRegex(ValueError,'differs from saved events'):
            system_emu.validate(frame)
        self.meta['scanouts'][2]['present_changed_during_scanout']=True;self.save()
        with self.assertRaisesRegex(ValueError,'no complete stable'):
            self.build('missing',frames=[1,2])
        self.assertFalse((self.root/'missing/frames').exists())

    def test_frame_bundle_replacement_archives_all_generated_frames(self):
        self.build(frames=[1,2])
        out=self.root/'report'
        before={str(p.relative_to(out)):reporting.digest(p) for p in out.rglob('*') if p.is_file()}
        self.build(frames=[2],replace_report=True)
        mapping=json.loads((out/'report-replacement.json').read_text())
        archive=Path(mapping['archive_directory'])
        self.assertEqual(before,mapping['files'])
        self.assertEqual(before,{str(p.relative_to(archive)):reporting.digest(p) for p in archive.rglob('*') if p.is_file()})
        self.assertFalse((out/'frames/frame-1').exists())
        self.assertTrue((out/'frames/frame-2/profiler.html').is_file())
        (out/'frames/frame-2/notes.txt').write_text('user content')
        with self.assertRaisesRegex(ValueError,'unknown report content'):
            self.build(frames=[2],replace_report=True)

    def test_partial_context_is_not_changed_to_complete(self):
        self.rows=[r for r in self.rows if r[0] < 1332]+[[1332,'submit',3]]
        self.save()
        d=self.build(allow_partial=True,frames=[2])
        frame=json.loads((self.root/'report/frames/frame-2/profile.json').read_text())
        self.assertTrue(d['partial']);self.assertTrue(frame['partial'])
        self.assertEqual(frame['system']['submissions'][-1]['end_tick'],None)

    def test_official_build_and_render_cli_and_identity_binding(self):
        args=[sys.executable,'-B',str(ROOT/'profiler.py'),'build',str(self.capture),
              '--out',str(self.root/'cli'),'--frames','1,2','--source-identity',str(self.identity),'--require-images']
        run=subprocess.run(args,capture_output=True,text=True,timeout=20)
        self.assertEqual(run.returncode,0,run.stderr)
        source=self.root/'cli/frames/frame-2/profile.json'
        out=self.root/'render-frame'
        result=reporting.render_report(source,out,require_images=True)
        f=result['system']['frame_view']
        self.assertEqual((out/f['session_report']).resolve(),(self.root/'cli/profiler.html').resolve())
        self.assertEqual(f['owned_submission_ids'],[2])
        changed=copy.deepcopy(result);changed['provenance']['application_identity']['program_sha256']='0'*64
        with self.assertRaisesRegex(ValueError,'identity binding differs'):
            system_emu.validate(changed)
        optional=json.loads(self.identity.read_text());optional.pop('runtime_sha256')
        self.identity.write_text(json.dumps(optional))
        checked=system_frames.attach_identity(result,self.identity)
        self.assertEqual(checked['provenance']['application_identity']['executable_sha256'],
                         result['provenance']['report_inputs']['files']['source-executable.bin'])
        wrong=json.loads(self.identity.read_text());wrong['boot_sha256']='0'*64
        self.identity.write_text(json.dumps(wrong))
        with self.assertRaisesRegex(ValueError,'does not match captured'):
            self.build('bad-identity',source_identity=self.identity)


if __name__=='__main__':
    unittest.main()
