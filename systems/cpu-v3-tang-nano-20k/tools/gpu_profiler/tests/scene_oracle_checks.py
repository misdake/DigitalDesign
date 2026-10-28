"""Opt-in NumPy full-scene reference checks; synthetic cases, not board proof."""
from pathlib import Path
import struct
import sys
import tempfile
import unittest

import numpy as np

sys.dont_write_bytecode = True
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from draw_oracle import evaluate
from scene_oracle import render,lighting,sample,expand,texture_levels
from layout import ROW,COLUMN
from test_draw_oracle import SyntheticCapture
from full_scene_replay import validate_scene,runtime_activity


class SceneReferenceChecks(unittest.TestCase):
    def test_direct_ray_matches_exact_rational_small_triangle(self):
        capture=SyntheticCapture(tint=0xff4080ff)
        small=evaluate(capture); scene=render(capture)
        self.assertEqual(scene.depth.ravel().tolist(),small.planes['depth'])
        self.assertEqual(scene.color.ravel().tolist(),small.planes['color'])

    def test_small_scene_cannot_satisfy_complete_scene_contract(self):
        with self.assertRaisesRegex(ValueError,'complete'):
            validate_scene(render(SyntheticCapture()))
        with tempfile.TemporaryDirectory() as temp:
            path=Path(temp)/'events.txt';path.write_text('DONE 10 1\n')
            with self.assertRaisesRegex(AssertionError,'complete nonempty'):
                runtime_activity(path,105)

    def test_actual_command_words_are_compared_not_just_counted(self):
        words=[0x30000000]*24+[0xb0000000]+[0]*79+[0xf0000000]
        lines=[f'CMD_EXEC {i} {4*i} {word:08x}' for i,word in enumerate(words)]
        lines+=['DRAW 0']*24+['VT_END 0']*24+['T_PUBLISH 0']*300+['Q_ALLOC 0']*10000+['DONE 0']
        commands=struct.pack('<105I',*words)
        with tempfile.TemporaryDirectory() as temp:
            path=Path(temp)/'events.txt';path.write_text('\n'.join(lines)+'\n')
            self.assertTrue(runtime_activity(path,105,commands)['frozen_command_words_verified'])
            path.write_text(path.read_text().replace('CMD_EXEC 0 0 30000000','CMD_EXEC 0 0 30000001',1))
            with self.assertRaisesRegex(AssertionError,'differs from frozen'):
                runtime_activity(path,105,commands)

    def test_integer_bilinear_taps_have_documented_rounding(self):
        tex=expand(np.array([[0xf800,0x07e0],[0x001f,0xffff]]))
        bilinear=sample({1:tex},(1 << 16)|(1 << 21),np.array([[.5,.5]]),np.array([0.]))
        self.assertEqual(bilinear.tolist(),[[127,127,127]])
        nearest=sample({1:tex},1 << 16,np.array([[.25,.25]]),np.array([0.]))
        self.assertEqual(nearest.tolist(),[[255,0,0]])

    def test_row_column_texture_addressing_is_not_a_transpose(self):
        expected=np.array([[(x*37+y*101)&65535 for x in range(32)] for y in range(32)])
        for layout in (ROW,COLUMN):
            capture=SyntheticCapture(layout);tile=8 if layout==ROW else 16
            raw=bytearray()
            blocks=[(tx,ty) for ty in range(32//tile) for tx in range(32//tile)]
            if layout==COLUMN:blocks.sort()
            for tx,ty in blocks:
                for y in range(tile):
                    for x in range(tile):raw+=struct.pack('<H',int(expected[ty*tile+y,tx*tile+x]))
            capture.put(0x4f0000,raw)
            got=texture_levels(capture,(0x4f0000//128)|(5 << 16))[5]
            np.testing.assert_array_equal(got,expand(expected))

    def test_ideal_light_uses_current_global_context(self):
        context=[0]*16;context[2]=16384;context[3]=16384 << 16
        context[4]=64|(192 << 16);context[5]=3
        g,h=lighting(np.array([[0,0,1024.]]),np.array([199.5]),np.array([119.5]),context,False)
        self.assertAlmostEqual(g[0],256)
        self.assertAlmostEqual(h[0],192)
        unlit,_=lighting(np.array([[0,0,-1024.]]),np.array([199.5]),np.array([119.5]),context,True)
        self.assertEqual(unlit[0],256)


if __name__=='__main__':
    unittest.main()
