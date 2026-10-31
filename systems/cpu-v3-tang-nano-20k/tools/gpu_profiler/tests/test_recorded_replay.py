"""Opt-in actual C inputs through bounded production GPU/stream-MC replay.

The CPU producer uses contract/control peers. These are not A ownership,
CPU/Display contention, board, PVT, or whole API-combination qualifications.
"""
import json
import os
from pathlib import Path
import sys
import unittest
import uuid

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from layout import COLUMN
from snapshot import Abi, encode_reference, decode_stream
from replay import run


@unittest.skipUnless(os.environ.get('GPU_API_NATIVE_REPLAY_DIR'), 'explicit bounded native replay tier')
class RecordedNativeReplay(unittest.TestCase):
    def setUp(self):
        self.root=Path(os.environ['GPU_API_NATIVE_REPLAY_DIR']).resolve()
        self.rtl=Path(os.environ['GPU_API_CAPTURE_RTL_DIR']).resolve()
        self.inputs=Path(os.environ['GPU_API_CAPTURE_FIXTURE_DIR']).resolve()
        self.out=self.root/uuid.uuid4().hex
        self.out.mkdir(parents=True,exist_ok=False)

    def replay(self,name):
        abi=Abi()
        source=json.loads((self.inputs/f'{name}-false.json').read_text())
        rows=[source['ranges'][i:i+8] for i in range(0,len(source['ranges']),8)]
        captures,report=decode_stream(encode_reference(source['header'],rows,[bytes(b) for b in source['blobs']],abi),abi)
        self.assertFalse(report['failures']);self.assertEqual(len(captures),1)
        capture=captures[0]
        capture.save(self.out/'frozen')
        result=run(self.out/'frozen/snapshot.json',self.rtl,self.out/'replay',200_000,60)
        self.assertTrue(result['complete'])
        return capture,result,self.out/'replay/capture'

    def test_no_target_memcpy_matches_all_source_bytes(self):
        capture,result,out=self.replay('memcpy')
        self.assertFalse(result['has_target'])
        self.assertEqual(len(result['raw_buffers']),1)
        self.assertEqual((out/result['raw_buffers'][0]['file']).read_bytes(),capture.read(0x300000,1024))

    def test_load_preserves_both_physical_planes_and_padding(self):
        capture,_,out=self.replay('ui_load')
        for role,base in [('color',0x200000),('depth',0x280000)]:
            self.assertEqual((out/f'{role}.bin').read_bytes(),capture.read(base,204800))

    def expected_ui(self,before,color=0xf800):
        expected=bytearray(before)
        for y in range(32,48):
            for x in range(224,232):
                at=COLUMN.offset(x,y)
                expected[at:at+2]=color.to_bytes(2,'little')
        return expected

    def test_dynamic_ui_writes_exactly_one_cell(self):
        capture,_,out=self.replay('ui_load_visible')
        self.assertEqual((out/'color.bin').read_bytes(),self.expected_ui(capture.read(0x200000,204800)))
        self.assertEqual((out/'depth.bin').read_bytes(),capture.read(0x280000,204800))

    def test_short_atlas_last_glyph_ignores_masked_neighbor(self):
        capture,_,out=self.replay('ui_short')
        atlas,=[r for r in capture.ranges if r.tag==capture.abi['TAG_ATLAS']]
        self.assertEqual(atlas.length,768)
        self.assertEqual((out/'color.bin').read_bytes(),self.expected_ui(capture.read(0x200000,204800),0x07e0))
        self.assertEqual((out/'depth.bin').read_bytes(),capture.read(0x280000,204800))

    def test_multiple_targets_keep_all_outputs_not_only_the_header_target(self):
        capture,result,out=self.replay('ui_load_multi_target')
        self.assertEqual(len(result['write_ranges']),4)
        writes={r['address']:out/r['file'] for r in result['write_ranges']}
        for base in (0x180000,0x200000):
            self.assertEqual(writes[base].read_bytes(),self.expected_ui(capture.read(base,204800)))
        for base in (0x340000,0x280000):
            self.assertEqual(writes[base].read_bytes(),capture.read(base,204800))


if __name__=='__main__':
    unittest.main()
