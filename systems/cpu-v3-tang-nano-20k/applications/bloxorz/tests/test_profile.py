"""Actual-edge timer semantics, source integrity and bounded runner sidecar."""
import copy
import json
from pathlib import Path
import sys
import unittest
from unittest.mock import patch
import tempfile
import hashlib
import subprocess
sys.path.insert(0,str(Path(__file__).resolve().parents[3]/'tools/gpu_profiler'))
from application_sidecar import codec
bloxorz_profile = codec(1)
derive, timing_report, launch = bloxorz_profile.derive, bloxorz_profile.timing_report, bloxorz_profile.launch


class BloxorzProfile(unittest.TestCase):
    def test_dedicated_final_wait_measures_without_legacy_registers(self):
        plan = dict(
            dedicated_final_wait=True, site_observation='first-unheld-execute',
            sites={'vblank_return': [7, 16], 'final_wait_return': [7, 54]},
            clock_sites={'clear': {'code_segment': 7, 'pc': 18},
                         'read': {'code_segment': 7, 'pc': 55}},
        )
        events = [dict(kind='input', tick=0, buttons_active_low=65535)]

        def site(name, cycle):
            events.append(dict(kind='site', name=name, tick=cycle*148,
                               pc=plan['sites'][name], observation='first-unheld-execute',
                               next_present=2, epoch=None))

        def clock(cycle, pc, write, value):
            events.append(dict(kind='device', tick=cycle*148, pc=[7, pc],
                               device=0, channel=11, write=write, value=value, committed=True))

        clock(4, 55, False, 0)
        site('vblank_return', 10)
        clock(13, 18, True, 1)
        site('final_wait_return', 121)
        clock(122, 55, False, 2)
        r = derive(events, plan, 0, 122*148)
        self.assertEqual([row['valid'] for row in r['intervals']], [False, True])
        report = timing_report(dict(system=dict(runner=r, presents=[[130*148, 'present_request', 2]])))
        self.assertEqual(report['intervals'][-1]['return_interval_cycles'], 111)
        self.assertEqual(report['intervals'][-1]['wait_return_to_read_cycles'], 1)

    def test_nonretiring_return_and_intermediate_wait_keep_distinct_endpoints(self):
        plan=dict(site_observation='first-unheld-execute',gates={},
                  sites=dict(vblank_return=[7,41816],final_wait_return=[7,47954]),
                  clock_sites=dict(clear=dict(code_segment=7,pc=41818),read=dict(code_segment=7,pc=47955)))
        events=[dict(kind='input',tick=0,buttons_active_low=65535),
                dict(kind='site',name='vblank_return',tick=10*148,pc=[7,41816],epoch=5,next_present=2,
                     observation='first-unheld-execute',finish_r10=0),
                dict(kind='device',tick=13*148,pc=[7,41818],device=0,channel=11,write=True,value=1,epoch=5,committed=True),
                dict(kind='device',tick=67*148,pc=[7,47955],device=0,channel=11,write=False,value=0,epoch=5,
                     committed=True,finish_r10=0,wait_started_r8=1,wait_start_epoch_r9=5),
                dict(kind='site',name='final_wait_return',tick=121*148,pc=[7,47954],epoch=5,next_present=2,
                     observation='first-unheld-execute',finish_r10=1),
                dict(kind='device',tick=122*148,pc=[7,47955],device=0,channel=11,write=False,value=2,epoch=5,
                     committed=True,finish_r10=1,wait_started_r8=1,wait_start_epoch_r9=5)]
        def report(rows):
            r=derive(rows,plan,0,122*148)
            return timing_report(dict(system=dict(runner=r,presents=[[130*148,'present_request',2]])))
        r=report(events)['intervals']
        self.assertFalse(r[0]['source_endpoints_bound']);self.assertIsNone(r[0]['within_900000_cycles'])
        self.assertTrue(r[1]['source_endpoints_bound']);self.assertEqual(r[1]['cycles'],109)
        self.assertEqual(r[1]['return_interval_cycles'],111)
        self.assertEqual(r[1]['return_to_clear_cycles'],3);self.assertEqual(r[1]['wait_return_to_read_cycles'],1)
        bad=copy.deepcopy(events);bad[4]['finish_r10']=0
        with self.assertRaisesRegex(ValueError,'finish discriminator'):report(bad)
        bad=copy.deepcopy(events);bad[1]['pc']=[7,41818]
        with self.assertRaisesRegex(ValueError,'site observation'):report(bad)
        bad=copy.deepcopy(events);bad[-1]['wait_started_r8']=0
        self.assertFalse(report(bad)['intervals'][-1]['valid'])
        bad=copy.deepcopy(events);bad[-1]['wait_start_epoch_r9']=6
        self.assertFalse(report(bad)['intervals'][-1]['valid'])

    def test_watchdog_kills_once_and_receipt_prevents_automatic_retry(self):
        root=Path(__file__).resolve().parents[5]/'target/bloxorz-profile-short'
        root.mkdir(parents=True,exist_ok=True)
        with tempfile.TemporaryDirectory(dir=root) as temp:
            root=Path(temp)
            def binding(name):
                (root/name).write_bytes(b'synthetic-no-execution')
                return dict(file=name,bytes=22,sha256=hashlib.sha256(b'synthetic-no-execution').hexdigest())
            plan=dict(schema='digitaldesign.ordinary-profile-plan',version=1,runtime=binding('runtime'),
                      program=binding('program'),bindings=[binding('source')],output='capture',report_output='report',
                      max_host_seconds=1,max_total_host_seconds=61)
            # Length is read from exact fixture bytes, not a guessed literal.
            for b in [plan['runtime'],plan['program'],*plan['bindings']]:b['bytes']=(root/b['file']).stat().st_size
            p=root/'plan.json';p.write_text(json.dumps(plan))
            with patch.object(bloxorz_profile.subprocess, 'Popen') as popen:
                child=popen.return_value;child.wait.side_effect=[subprocess.TimeoutExpired('fixture',61),-9];child.returncode=-9
                self.assertEqual(launch(p),2);child.kill.assert_called_once();self.assertEqual(popen.call_count,1)
                with self.assertRaisesRegex(ValueError,'no automatic rerun'):launch(p)
                self.assertEqual(popen.call_count,1)
            receipt=json.loads(p.with_suffix('.launch.json').read_text())
            self.assertTrue(receipt['wall_timeout']);self.assertFalse(receipt['complete'])

    def test_timer_clear_read_epoch_cold_saturation_and_tamper(self):
        plan={'gates':{'sample':[7,123]},'sites':{'vblank_return':[7,123],'final_wait_return':[7,123]}}
        events=[dict(kind='input',tick=0,buttons_active_low=65535)]
        def device(cycle,write,value,epoch=0):
            events.append(dict(kind='device',tick=cycle*148,pc=[7,123],retired_before=len(events),
                               device=0,channel=11,write=write,value=value,epoch=epoch,committed=True))
        device(1,False,65535);device(2,True,1);device(57,False,1)
        device(100,True,1);device(100+65535*54+1,False,65535)
        start=4_000_000;device(start,True,1);device(start+5401,False,100,1)
        device(start+6000,True,1,1)
        events.append(dict(kind='site',name='vblank_return',pc=[7,123],tick=(start+6000)*148,epoch=1,next_present=38))
        device(start+6055,False,1,1)
        events.append(dict(kind='site',name='final_wait_return',pc=[7,123],tick=(start+6055)*148,epoch=1,next_present=38))
        r=derive(events,plan,0,events[-1]['tick'])
        self.assertEqual([v['valid'] for v in r['intervals']],[False,True,False,False,True])
        self.assertEqual(r['intervals'][-1]['clear_tick'],(start+6000)*148)
        d={'system':{'runner':r,'presents':[[events[-1]['tick']+148,'present_request',38]]}}
        report=timing_report(d);self.assertEqual(report['intervals'][-1]['presentation'],38)
        self.assertTrue(report['intervals'][-1]['within_900000_cycles'])
        bad=copy.deepcopy(events);bad[3]['value']=2
        with self.assertRaisesRegex(ValueError,'prescaler'):derive(bad,plan,0,events[-1]['tick'])
        bad=copy.deepcopy(events);bad.append(dict(kind='input',tick=events[-1]['tick'],buttons_active_low=0))
        with self.assertRaisesRegex(ValueError,'physical input'):derive(bad,plan,0,events[-1]['tick'])
        bad=copy.deepcopy(events);bad[1]['tick']=-1
        with self.assertRaisesRegex(ValueError,'sidecar time'):derive(bad,plan,0,events[-1]['tick'])

    def test_input_and_committed_gate_trace(self):
        plan={'gates':{'consumed':[7,99]}}
        events=[dict(kind='input',tick=0,buttons_active_low=65535),
                dict(kind='input',tick=148,buttons_active_low=65503),
                dict(kind='gate',tick=296,name='consumed',pc=[7,99],state={'phase':0}),
                dict(kind='input',tick=444,buttons_active_low=65535)]
        derive(events,plan,0,444)
        bad=copy.deepcopy(events);bad[2]['pc']=[7,100]
        with self.assertRaisesRegex(ValueError,'committed gate'):derive(bad,plan,0,444)
        bad=events+[dict(kind='input',tick=444,buttons_active_low=65503)]
        with self.assertRaisesRegex(ValueError,'one real'):derive(bad,plan,0,444)


if __name__=='__main__':unittest.main()
