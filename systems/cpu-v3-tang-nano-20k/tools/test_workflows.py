"""Offline workflow contracts; mock only the independently audited hardware process."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from application_artifact import Artifact, canonical
import hardware

spec = importlib.util.spec_from_file_location('application_profile', Path(__file__).with_name('profile.py'))
profile = importlib.util.module_from_spec(spec)
spec.loader.exec_module(profile)


class Comparison(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.value = dict(schema='digitaldesign.profile-run', version=1, complete=True,
                          request=dict(scene='default', backend='system', frame=2, clock_hz=54000000,
                                       boundary='applied-to-applied', input='disconnected', schedule=None),
                          runtime={'sha256': 'model'}, tools={'tool': 'hash'}, package={'sha256': 'program'},
                          metrics={'frame_logic_cycles': 100}, files={})

    def run_at(self, name, value):
        folder = self.root/name
        folder.mkdir()
        value = copy.deepcopy(value)
        value['run_id'] = hashlib.sha256(canonical(value)).hexdigest()
        profile.write(folder/'run.json', value)
        return folder

    def test_difference_is_explicit_and_metrics_keep_machine_units(self):
        a = self.run_at('a', self.value)
        b = copy.deepcopy(self.value)
        b['package']['sha256'] = 'new-program'
        b['metrics']['frame_logic_cycles'] = 80
        b = self.run_at('b', b)
        with self.assertRaisesRegex(ValueError, 'differences require'):
            profile.compare(a, b)
        self.assertEqual(profile.compare(a, b, 'application optimization')['metrics']['frame_logic_cycles'],
                         dict(before=100, after=80, delta=-20, percent=-20))

    def test_partial_mixed_domain_model_tools_scene_and_state_are_rejected(self):
        a = self.run_at('a', self.value)
        for key in ('partial', 'backend', 'frame', 'boundary', 'runtime', 'tools', 'state'):
            b = copy.deepcopy(self.value)
            if key == 'partial':
                b['complete'] = False
            elif key in ('runtime', 'tools'):
                b[key] = {'different': True}
            elif key == 'state':
                b['selected_state'] = {'motion_age': 101}
            else:
                b['request'][key] = 'different'
            with self.subTest(key=key), self.assertRaises(ValueError):
                profile.compare(a, self.run_at(key, b), 'intentional program change')

    def test_changed_raw_evidence_and_escaping_path_are_rejected(self):
        (self.root/'raw').write_bytes(b'original')
        value = copy.deepcopy(self.value)
        value['files'] = {'../raw': profile.identity(self.root/'raw')}
        with self.assertRaisesRegex(ValueError, 'relative run file'):
            profile.verified_run(self.run_at('escape', value))
        a = self.run_at('a', self.value)
        (a/'raw').write_bytes(b'original')
        value['files'] = {'raw': profile.identity(a/'raw')}
        value['run_id'] = hashlib.sha256(canonical(value)).hexdigest()
        profile.write(a/'run.json', value)
        (a/'raw').write_bytes(b'changed')
        with self.assertRaisesRegex(ValueError, 'changed run'):
            profile.verified_run(a)

    def test_offline_reports_never_launch_processes(self):
        self.value['request']['backend'] = 'cpu'
        a = self.run_at('cpu', self.value)
        with patch.object(profile.subprocess, 'run', side_effect=AssertionError('process launched')):
            profile.report(a, self.root/'report')
        self.assertTrue((self.root/'report/report.json').is_file())

    def test_recovery_rejects_a_real_partial_capture_before_launching_any_process(self):
        value = copy.deepcopy(self.value)
        value['complete'] = False
        a = self.run_at('partial', value)
        (a/'capture').mkdir()
        profile.write(a/'capture/capture.json', dict(reached=False, truncated=True))
        value['files'] = {'capture/capture.json': profile.identity(a/'capture/capture.json')}
        value['run_id'] = hashlib.sha256(canonical(value)).hexdigest()
        profile.write(a/'run.json', value)
        with patch.object(profile.subprocess, 'run', side_effect=AssertionError('process launched')):
            with self.assertRaisesRegex(ValueError, 'incomplete capture'):
                profile.recover(a, self.root/'recovered', 60)
        self.assertFalse((self.root/'recovered').exists())


class Deployment(unittest.TestCase):
    def test_configuration_encoding_padding_and_capacity(self):
        with tempfile.TemporaryDirectory() as directory:
            fs = Path(directory)/'image.fs'
            fs.write_text('// header\n00000001\n11111111\n')
            config = hardware.configuration(fs)
            self.assertEqual(config, b'\x01\xff')
            image = hardware.compose(config, b'CB06', 8*1024*1024)
            self.assertEqual(image[:2], config)
            self.assertEqual(image[2:hardware.BOOT_OFFSET], b'\xff'*(hardware.BOOT_OFFSET-2))
            self.assertEqual(image[hardware.BOOT_OFFSET:], b'CB06')
            for config, package, capacity in [(bytes(hardware.BOOT_OFFSET+1), b'', 8*1024*1024),
                                               (b'config', b'CB06', hardware.BOOT_OFFSET+3)]:
                with self.subTest(capacity=capacity), self.assertRaises(ValueError):
                    hardware.compose(config, package, capacity)
            fs.write_text('not bit data')
            with self.assertRaises(ValueError):
                hardware.configuration(fs)

    def test_wrong_project_is_not_a_system_deployment(self):
        process = type('Process', (), {'stdout': json.dumps(dict(schema='digitaldesign.gowin-audit',
                              version=1, project='another', logic_top='CpuV3System'))})()
        with patch.object(hardware.subprocess, 'run', return_value=process), self.assertRaises(ValueError):
            hardware.audit('hardware', 'frozen-runtime')


class SourceBinding(unittest.TestCase):
    def test_statement_identity_is_line_ending_independent_and_rejects_ambiguity(self):
        artifact = Artifact.__new__(Artifact)
        artifact.globals, artifact.constants = {}, {}
        artifact.functions = {'run': dict(pc=0, end_pc=1, file='source.rs')}
        artifact.manifest = dict(artifact_id='fixture', target={'code_segment': 7},
                                source_map={'source.rs': {'file': 'source'}})
        for ending in ('\n', '\r\n'):
            artifact.files = {'source': ending.join(['fn run(){', 'capture();', 'let input=take();', '}']).encode()}
            self.assertEqual(artifact.source_line('run', 'capture();\nlet input=take();'), 2)
        artifact.files = {'source': b'fn run(){\ncapture();\ncapture();\n}'}
        with self.assertRaisesRegex(ValueError, 'unique source statement'):
            artifact.source_line('run', 'capture();')


if __name__ == '__main__':
    unittest.main()
