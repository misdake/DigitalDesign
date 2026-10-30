"""Production exports require explicit, matching column storage identities."""
import json
from pathlib import Path
import tempfile
import unittest
import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from layout import COLUMN
from producer_handoff import select_replay_export
from snapshot import SnapshotError

class ExportSelectionTests(unittest.TestCase):
    def export(self, root):
        root.mkdir()
        (root/'rtl.json').write_text(json.dumps(['gpu.v']))
        (root/'layout.json').write_text(json.dumps(dict(schema=1, layout_abi=2, gpu_profile='production')))
        (root/'gpu.v').write_text('function[8:0]column_ui_tile; clear_sector==1599; wire[11:0]macro_column=0;')
        return root

    def test_matching_export_is_used_without_copy_or_replacement(self):
        with tempfile.TemporaryDirectory() as temp:
            root = self.export(Path(temp)/'current')
            self.assertEqual(select_replay_export(COLUMN, root), root)
            self.assertEqual(sorted(p.name for p in root.iterdir()), ['gpu.v', 'layout.json', 'rtl.json'])

    def test_legacy_missing_and_false_layout_declarations_are_rejected(self):
        for mode in ['row', 'missing', 'old_profile', 'row_rtl']:
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temp:
                root = self.export(Path(temp)/'current')
                if mode == 'missing': (root/'layout.json').unlink()
                elif mode == 'row_rtl': (root/'gpu.v').write_text('// row framebuffer')
                else:
                    metadata = json.loads((root/'layout.json').read_text())
                    if mode == 'row': metadata['layout_abi'] = 1
                    else: metadata['gpu_profile'] = 'texture16x8-column16'
                    (root/'layout.json').write_text(json.dumps(metadata))
                with self.assertRaises((ValueError, SnapshotError)):
                    select_replay_export(COLUMN, root)

if __name__ == '__main__': unittest.main()
