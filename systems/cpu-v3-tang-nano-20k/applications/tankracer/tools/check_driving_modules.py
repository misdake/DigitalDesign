"""Check that consecutive application profiles select their own RCC sources."""
from pathlib import Path
import tempfile
import unittest

from driving_assets import module_directory


class GeneratedModules(unittest.TestCase):
    def test_profile_switch_replaces_the_complete_generated_module_set(self):
        target = Path(__file__).resolve().parents[5] / 'target'
        with tempfile.TemporaryDirectory(dir=target) as temporary:
            output = Path(temporary)
            modules = module_directory(output)
            (modules / 'drive.rs').write_text('#![link_cseg="frame"]\n')
            (modules / 'frame_output.rs').write_text('mod range_map;\n')
            payload = output / 'scene.bin'
            payload.write_bytes(b'authored scene')
            self.assertEqual(module_directory(output), modules)
            self.assertEqual(list(modules.glob('*.rs')), [])
            (modules / 'frame_output.rs').write_text('pub struct Renderer {}\n')
            self.assertFalse((modules / 'drive.rs').exists())
            self.assertEqual(payload.read_bytes(), b'authored scene')

    def test_old_root_modules_do_not_enter_the_current_module_directory(self):
        target = Path(__file__).resolve().parents[5] / 'target'
        with tempfile.TemporaryDirectory(dir=target) as temporary:
            output = Path(temporary)
            (output / 'drive.rs').write_text('old profile')
            modules = module_directory(output)
            self.assertEqual(modules.parent, output)
            self.assertFalse((modules / 'drive.rs').exists())
            self.assertEqual((output / 'drive.rs').read_text(), 'old profile')

    def test_directory_named_as_a_module_is_preserved(self):
        target = Path(__file__).resolve().parents[5] / 'target'
        with tempfile.TemporaryDirectory(dir=target) as temporary:
            output = Path(temporary)
            modules = module_directory(output)
            (modules / 'foreign.rs').mkdir()
            with self.assertRaisesRegex(ValueError, 'unexpected'):
                module_directory(output)
            self.assertTrue((modules / 'foreign.rs').is_dir())


if __name__ == '__main__':
    unittest.main()
