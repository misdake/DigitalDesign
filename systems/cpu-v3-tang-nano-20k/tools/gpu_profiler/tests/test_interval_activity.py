"""Exact full-system interval accounting, independent of applications."""
from pathlib import Path
import sys
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from interval_activity import interval_activity


class IntervalActivity(unittest.TestCase):
    def test_overlap_uses_timer_window_and_preserves_payload_boundaries(self):
        held=[0,1,0,0,0,1,1,1,23,0];active=held.copy();active[1]=0
        s=dict(origin_tick=0,coverage_end_tick=592,states=[[0,[0]*10],[148,held],[296,active],[444,[0]*10]],
               client_payload=[[148,6,0,8],[296,1,8,0],[444,1,8,0]],
               native_model32=[[185,0,4,3],[259,4,0,0],[481,0,4,4]])
        r=interval_activity(s,148,444)
        self.assertEqual(r['cycles'],2);self.assertEqual(r['gpu_busy_cycles'],2)
        self.assertEqual(r['cpu_runnable_gpu_busy_cycles'],1);self.assertEqual(r['display_cpu_gpu_pending_cycles'],2)
        self.assertEqual(r['client64_payload_bytes'][1],[8,0]);self.assertEqual(r['native32_bytes'],[4,4,3])
        s['states']=s['states'][2:];self.assertFalse(interval_activity(s,148,444)['available'])
