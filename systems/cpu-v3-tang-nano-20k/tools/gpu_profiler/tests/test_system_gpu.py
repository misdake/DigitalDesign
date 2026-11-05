"""Board descriptor-slot boundaries preserve ownership, not acceptance ticks."""
import copy
from pathlib import Path
import sys
import unittest
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
import system_gpu


class DescriptorSlots(unittest.TestCase):
    def test_fifo_fields_and_distinct_master_service_boundaries(self):
        masters=[[148,0,4096,3],[296,2,8192,0],[444,0,4096,3]]
        data=dict(partial=False,system=dict(transactions=[
            dict(id=1,port=4,start_tick=296,address_halfword=4096,requested_beats=16),
            dict(id=2,port=4,start_tick=888,address_halfword=4096,requested_beats=16),
            dict(id=3,port=6,start_tick=1184,address_halfword=8192,requested_beats=4)]))
        self.assertEqual(system_gpu.bind_services(data,masters),[[1,148],[2,444],[3,296]])
        for field,value in [('start_tick',0),('address_halfword',4097),('requested_beats',8)]:
            bad=copy.deepcopy(data);bad['system']['transactions'][0][field]=value
            with self.assertRaisesRegex(ValueError,'slot FIFO/field'):
                system_gpu.bind_services(bad,masters)
        data['system']['transactions'].pop()
        with self.assertRaisesRegex(ValueError,'unserved GPU descriptor'):
            system_gpu.bind_services(data,masters)
        data['partial']=True
        self.assertEqual(system_gpu.bind_services(data,masters),[[1,148],[2,444]])


if __name__=='__main__':unittest.main()
