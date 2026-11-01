"""Import-time hold configuration and scoped capture regression tests."""
from pathlib import Path
import importlib.util
import os
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import uuid

sys.dont_write_bytecode = True
SPEC = importlib.util.spec_from_file_location('join_capture_tested',
                                             Path(__file__).with_name('join_q_policy.py'))
JOIN = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(JOIN)


class CaptureEnvironment(unittest.TestCase):
    def setUp(self):
        self.root = JOIN.ROOT/'target/join-capture-unit'/uuid.uuid4().hex
        self.root.mkdir(parents=True)
        self.base = self.root/'exports'
        self.base.mkdir()
        for mode in ['native', 'portable']:
            (self.base/f'generic-{mode}.v').write_text('frozen test input')

    def exercise(self, prior_hold, fail_import=False):
        saved_argv = sys.argv
        sentinel = SimpleNamespace(name='other fixture')
        loaded, calls = [], []
        with patch.dict(os.environ), patch.dict(sys.modules,
                {'stage1_frozen_replay': sentinel}):
            if prior_hold is None:
                os.environ.pop('VLIW_SOURCE_HOLD_CYCLES', None)
            else:
                os.environ['VLIW_SOURCE_HOLD_CYCLES'] = prior_hold
            os.environ['VLIW_BACKEND_EXPORT'] = 'caller-owned-export'

            def load(fixture):
                self.assertEqual(os.environ['VLIW_SOURCE_HOLD_CYCLES'], '17')
                loaded.append(fixture)
                sys.modules['stage1_frozen_replay'] = fixture.m
                sys.argv = ['fixture import changed argv']
                if fail_import:
                    raise RuntimeError('fixture import failed')

            def create(_):
                fixture = SimpleNamespace(m=SimpleNamespace())
                fixture.m.main = lambda: calls.append((fixture, list(sys.argv)))
                return fixture

            spec = SimpleNamespace(loader=SimpleNamespace(exec_module=load))
            with patch.object(JOIN.importlib.util, 'spec_from_file_location', return_value=spec), \
                 patch.object(JOIN.importlib.util, 'module_from_spec', side_effect=create):
                for n in range(2):
                    out = self.root/f'capture-{prior_hold}-{fail_import}-{n}'
                    if fail_import:
                        with self.assertRaisesRegex(RuntimeError, 'fixture import failed'):
                            JOIN.capture(self.base, out)
                    else:
                        self.assertEqual(JOIN.capture(self.base, out), out)
                    self.assertIs(sys.argv, saved_argv)
                    self.assertIs(sys.modules['stage1_frozen_replay'], sentinel)
                    self.assertEqual(os.environ.get('VLIW_SOURCE_HOLD_CYCLES'), prior_hold)
                    self.assertEqual(os.environ['VLIW_BACKEND_EXPORT'], 'caller-owned-export')
            self.assertEqual(len(loaded), 2)
            self.assertIsNot(loaded[0], loaded[1])
            self.assertEqual(len(calls), 0 if fail_import else 4)
            if not fail_import:
                self.assertNotEqual(loaded[0].m.O, loaded[1].m.O)

    def test_import_observes_hold_and_calls_are_independent(self):
        for value in [None, '3']:
            with self.subTest(prior_hold=value):
                self.exercise(value)

    def test_import_failure_restores_caller(self):
        for value in [None, '3']:
            with self.subTest(prior_hold=value):
                self.exercise(value, fail_import=True)

    def test_hold_zero_cannot_qualify(self):
        out = self.root/'hold-zero'
        out.mkdir()
        (out/'run.log').write_text('SOURCE reads=32 skips=0 holdCE0=0\n')
        with self.assertRaisesRegex(AssertionError, '17-edge hold not exercised'):
            JOIN.held_source_window(out)


if __name__ == '__main__':
    unittest.main()
