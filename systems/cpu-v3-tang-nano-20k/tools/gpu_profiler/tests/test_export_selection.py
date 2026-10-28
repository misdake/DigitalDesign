"""Current exports must match the frozen input without silent RTL replacement."""
import json
from pathlib import Path
import tempfile
import unittest
import sys
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from layout import ROW, COLUMN
from producer_handoff import select_replay_export


class ExportSelectionTests(unittest.TestCase):
    def export(self, root, layout):
        root.mkdir()
        (root / 'rtl.json').write_text(json.dumps(['gpu.v']))
        (root / 'layout.json').write_text(json.dumps(dict(
            schema=1, layout_abi=layout.abi,
            gpu_profile='texture16x8-column16' if layout == COLUMN else 'production')))
        source = ('function[8:0]column_ui_tile; clear_sector==1599; '
                  'wire[11:0]macro_column=0;' if layout == COLUMN else '// row GPU')
        (root / 'gpu.v').write_text(source)
        return root

    def test_current_row_and_column_exports_are_used_directly(self):
        for layout in (ROW, COLUMN):
            with self.subTest(layout=layout.abi), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                rtl = self.export(root / 'current', layout)
                output = root / 'unused'
                with patch('producer_handoff.stage_column') as stage:
                    self.assertEqual(select_replay_export(layout, rtl, output), rtl)
                    stage.assert_not_called()
                self.assertFalse(output.exists())

    def test_layout_mismatch_is_rejected_without_replacement(self):
        for expected, actual in ((ROW, COLUMN), (COLUMN, ROW)):
            with tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                rtl = self.export(root / 'current', actual)
                with self.assertRaisesRegex(ValueError, 'does not match input layout'):
                    select_replay_export(expected, rtl, root / 'unused')

    def test_legacy_replacement_requires_both_identities_and_row_peer(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            row = self.export(root / 'row', ROW)
            column = self.export(root / 'column', COLUMN)
            frozen, contract, output = root / 'frozen.v', root / 'contract.json', root / 'staged'
            for expected, rtl, f, c in ((COLUMN, row, frozen, None),
                                        (COLUMN, row, None, contract),
                                        (COLUMN, column, frozen, contract),
                                        (ROW, row, frozen, contract)):
                with self.assertRaisesRegex(ValueError, 'Legacy frozen replacement'):
                    select_replay_export(expected, rtl, output, f, c)
            with patch('producer_handoff.stage_column') as stage:
                self.assertEqual(select_replay_export(COLUMN, row, output, frozen, contract), output)
                stage.assert_called_once_with(row, frozen, contract, output)
