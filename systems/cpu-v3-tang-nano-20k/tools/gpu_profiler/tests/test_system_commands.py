"""Committed instruction facts: no static package decoding or functional replay."""
import copy
import sys
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from system_commands import normalize


class Commands(unittest.TestCase):
    def setUp(self):
        self.system = dict(axis_origin_tick=0, origin_tick=0, coverage_end_tick=2960,
                           capabilities=dict(command_control_lineage=True),
                           submissions=[dict(id=1, start_tick=0, end_tick=2812)],
                           command_events=[])
        # Controlled real protocol shape; WAIT/FLUSH and END have distinct issue,
        # CP release and drain boundaries. Tick 1332 could be a frame boundary.
        rows = [(0,'submit',[1,4096,12]), (148,'latch',[1,0,0x40000003]),
                (296,'execute',[1,0,0x40000003]), (296,'issue',[1,0,0x40000003]),
                (740,'release',[1,0,0x40000003]), (1036,'latch',[1,4,0x70000000]),
                (1184,'execute',[1,4,0x70000000]), (1184,'issue',[1,4,0x70000000]),
                (1776,'release',[1,4,0x70000000]), (2072,'latch',[1,8,0xf0000000]),
                (2220,'execute',[1,8,0xf0000000]), (2220,'issue',[1,8,0xf0000000]),
                (2220,'release',[1,8,0xf0000000]), (2664,'drain',[1])]
        self.system['command_events'] = [[t,'cb_'+k,v] for t,k,v in rows]

    def test_actual_descriptor_word_and_distinct_completion_boundaries(self):
        subs, commands = normalize(self.system, False)
        self.assertEqual(subs[0]['bytes'], 12)
        self.assertEqual([c['address_bytes'] for c in commands], [4096,4100,4104])
        self.assertEqual(commands[1]['start'], 6)
        self.assertEqual(commands[1]['issue_tick'], 1184)
        self.assertEqual(commands[1]['end'], 13)
        self.assertEqual(commands[2]['release_tick'], 2220)
        self.assertEqual(commands[2]['end'], 19)

    def test_truncated_abort_and_reset_keep_open_without_fake_end(self):
        self.system['submissions'][0]['end_tick'] = None
        self.system['command_events'] = self.system['command_events'][:8]
        _, commands = normalize(self.system, True)
        self.assertIsNone(commands[-1]['end'])
        self.system['command_events'] += [[1332,'cb_abort',[1]], [1480,'cb_reset',[1]]]
        subs, commands = normalize(self.system, True)
        self.assertEqual(subs[0]['reset_tick'], 1480)
        self.assertIsNone(commands[-1]['end'])
        with self.assertRaisesRegex(ValueError, 'missing command drain'):
            normalize(self.system, False)

    def test_duplicate_reorder_prefetch_word_and_false_completion_rejected(self):
        mutations = [
            lambda s: s['command_events'].insert(3,copy.deepcopy(s['command_events'][2])),
            lambda s: s['command_events'][3][2].__setitem__(2,0x40000000),
            lambda s: s['command_events'][5][2].__setitem__(1,8),
            lambda s: s['command_events'][2].__setitem__(1,'cb_issue'),
            lambda s: s['command_events'][1].__setitem__(1,'cb_prefetch'),
            lambda s: s['submissions'][0].__setitem__('end_tick',2664),
            lambda s: s['command_events'][0][2].__setitem__(2,16),
        ]
        for mutate in mutations:
            source = copy.deepcopy(self.system); mutate(source)
            with self.assertRaises(ValueError): normalize(source, False)

    def test_legacy_cannot_invent_command_lineage(self):
        self.system['capabilities'] = {}
        with self.assertRaisesRegex(ValueError, 'capability'): normalize(self.system, True)
        self.system.pop('command_events')
        self.assertEqual(normalize(self.system, True), ([],[]))


if __name__ == '__main__': unittest.main()
