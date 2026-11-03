"""Bounded source, byte-boundary, identity and re-render regression (no RTL)."""
import copy
import json
from pathlib import Path
import sys
import subprocess
import tempfile
import unittest
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
import reporting
import system_emu as s


class SystemEmuTests(unittest.TestCase):
    def setUp(self):
        root=ROOT.parents[3]/'target/c4-profile/python-tests';root.mkdir(parents=True,exist_ok=True)
        self.temp=tempfile.TemporaryDirectory(dir=root);self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name);self.source=self.root/'capture';self.source.mkdir()
        (self.source/'program.bin').write_bytes(b'CB06-test')
        (self.source/'source-executable.bin').write_bytes(b'synthetic-unit-fixture')
        self.rows=[[0,'state',[0]*10],[0,'cpu',[0,0]],
                   [148,'accept',[2,0,True,1]],[148,'payload',[2,0,8]],
                   [185,'native',[0,4,2]],[259,'native',[0,4,0]],
                   [296,'terminal',[2,False]],[296,'accept',[1,32,False,1]],
                   [296,'payload',[1,8,0]],[296,'terminal',[1,False]],
                   [370,'native',[4,0,0]]]
        self.meta=dict(schema=s.SOURCE,version=1,source_kind='system-cycle-emu',ports=s.PORTS,
                       quantum_hz=7992000000,logic_period=148,origin_tick=0,end_tick=592,
                       coverage_end_tick=592,events=len(self.rows),truncated=False,reached=True,
                       scanouts=[],capabilities=dict(client_payload64=True,native_model32=True,pin_electrical=False,instruction_owner_lineage=False),
                       host_step_seconds=0.1,host_export_seconds=0.01)
        self.save()

    def save(self):
        self.meta['events']=len(self.rows)
        (self.source/'capture.json').write_text(json.dumps(self.meta))
        (self.source/'system-events.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in self.rows))

    def test_zero_submit_boundaries_and_rerender(self):
        d=reporting.build_report(self.source,self.root/'report')
        self.assertFalse(d['partial']);self.assertEqual(d['system']['submissions'],[])
        self.assertEqual([t['read_bytes'] for t in d['system']['transactions']],[0,8])
        self.assertEqual(sum(row[2] for row in d['system']['native_model32']),8)
        self.assertEqual(sum(row[3] for row in d['system']['native_model32']),2)
        r=reporting.render_report(self.root/'report/profile.json',self.root/'render')
        self.assertEqual(r,d)
        self.assertEqual((self.root/'report/system-memory.csv').read_bytes(),(self.root/'render/system-memory.csv').read_bytes())
        self.assertIn('not physical pin bandwidth',d['system']['client_boundary'])

    def test_no_terminal_payload_and_hash_binding(self):
        d=reporting.build_report(self.source,self.root/'report')
        self.assertEqual(sum(x[3] for x in d['system']['client_payload']),8)
        (self.source/'program.bin').write_bytes(b'changed')
        with self.assertRaisesRegex(ValueError,'binding mismatch'):
            reporting.build_report(self.source,self.root/'wrong',binding=self.root/'report/report-inputs.json')

    def test_lookahead_write_payload_can_precede_older_read_terminal(self):
        self.rows=self.rows[:2]+[[148,'accept',[2,0,False,1]],[296,'accept',[2,8,True,1]],
                                [296,'payload',[2,0,8]],[296,'payload',[2,8,0]],
                                [296,'terminal',[2,False]],[444,'terminal',[2,False]]]
        self.save();d=s.build(self.source,self.root/'overlap')
        self.assertEqual([(t['read_bytes'],t['write_bytes']) for t in d['system']['transactions']],[(8,0),(0,8)])

    def test_unknown_ownership_direction_and_time_are_rejected(self):
        original=copy.deepcopy(self.rows)
        for row in ([0,'payload',[2,0,8]],[148,'payload',[2,8,0]],[593,'native',[4,0,0]]):
            self.rows=copy.deepcopy(original);self.rows[3]=row;self.save()
            with self.assertRaises(ValueError):s.build(self.source,self.root/'bad')

    def test_truncated_is_partial_and_missing_raster_is_unavailable(self):
        self.meta.update(truncated=True,coverage_end_tick=370);self.save()
        with self.assertRaisesRegex(ValueError,'allow-partial'):s.build(self.source,self.root/'refused')
        d=s.build(self.source,self.root/'partial',allow_partial=True)
        self.assertTrue(d['partial'])
        with self.assertRaisesRegex(ValueError,'no complete scanout'):
            s.build(self.source,self.root/'no-image',allow_partial=True,require_images=True)

    def test_registered_png_binding_and_render(self):
        raw=bytes([61,124,183])*(800*480);(self.source/'scanout-000000.rgb').write_bytes(raw)
        self.meta['scanouts']=[dict(file='scanout-000000.rgb',frame=0,start_tick=0,end_tick=480,effective_pixels=384000,applied_present_count=2,present_id=None,source='registered-rgb')];self.save()
        d=s.build(self.source,self.root/'image',require_images=True)
        reporting.render_report(self.root/'image/profile.json',self.root/'rerender',require_images=True)
        self.assertEqual((self.root/'image/scanout-000000.png').read_bytes(),(self.root/'rerender/scanout-000000.png').read_bytes())
        d['system']['scanouts'][0]['png_sha256']='0'*64
        with self.assertRaisesRegex(ValueError,'PNG binding'):s.validate(d)

    def render_cli(self, data, name):
        source=self.root/(name+'.json');source.write_text(json.dumps(data))
        return subprocess.run([sys.executable,'-B',str(ROOT/'profiler.py'),'render',str(source),
                               '--out',str(self.root/(name+'-render'))],capture_output=True,text=True,timeout=20)

    def test_public_render_rejects_closed_size_and_terminal_before_payload(self):
        valid=s.build(self.source,self.root/'valid')
        for name,changes in [('closed-size-mismatch',dict(requested_beats=2)),
                             ('closed-before-payload',dict(end_tick=148))]:
            with self.subTest(name=name):
                data=copy.deepcopy(valid);data['system']['transactions'][0].update(changes)
                result=self.render_cli(data,name)
                self.assertEqual(result.returncode,1,result.stdout)
                self.assertIn('Report refused:',result.stderr)
                self.assertFalse((self.root/(name+'-render/profile.json')).exists())
        self.rows[2][2][3]=2;self.save()
        with self.assertRaisesRegex(ValueError,'terminal before full payload'):
            s.build(self.source,self.root/'raw-invalid')

    def test_render_checks_direction_last_data_and_partial_success_semantics(self):
        valid=s.build(self.source,self.root/'valid')
        for changes in [dict(writing=False),dict(last_data_tick=None),dict(last_data_tick=296),
                        dict(read_bytes=8,write_bytes=0),dict(error=None),dict(end_tick=297)]:
            with self.subTest(changes=changes):
                data=copy.deepcopy(valid);data['system']['transactions'][0].update(changes)
                with self.assertRaises(ValueError):s.validate(data)
        data=copy.deepcopy(valid);data['partial']=True
        data['system']['transactions'][0]['requested_beats']=2
        with self.assertRaisesRegex(ValueError,'terminal before full payload'):s.validate(data)

    def test_public_render_allows_same_edge_terminal_error_and_open_prefix(self):
        valid=s.build(self.source,self.root/'valid')
        # The terminal edge is end_tick-148; payload and terminal on148 are legal.
        same=copy.deepcopy(valid);same['system']['transactions'][0]['end_tick']=296
        self.assertEqual(self.render_cli(same,'same-edge').returncode,0)
        error=copy.deepcopy(valid);error['system']['transactions'][0].update(requested_beats=2,error=True)
        self.assertEqual(self.render_cli(error,'error-prefix').returncode,0)
        self.rows[2][2][3]=2;self.rows[6][2][1]=True;self.save()
        self.assertFalse(s.build(self.source,self.root/'raw-error')['partial'])
        opened=copy.deepcopy(valid);opened['partial']=True
        opened['system']['transactions'][0].update(requested_beats=2,end_tick=None,error=None)
        self.assertEqual(self.render_cli(opened,'open-prefix').returncode,0)
        self.rows.pop(6);self.save()
        self.assertTrue(s.build(self.source,self.root/'raw-open',allow_partial=True)['partial'])

if __name__=='__main__':unittest.main()
