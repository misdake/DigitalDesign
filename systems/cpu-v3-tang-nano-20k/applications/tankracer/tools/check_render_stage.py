"""Check selection page contents, capacity boundaries and ordinary storage."""
import json
from pathlib import Path
import tempfile
import unittest

from render_stage import TABLES, prepare, read_table


class Assets:
    def __init__(self, folder):
        self.output = folder
        self.payloads = []

    def bytes(self, name, payload):
        self.payloads.append(payload)
        (self.output / (name.lower() + '.bin')).write_bytes(payload)


def source():
    return ''.join(f'pub static {name}:Buf<u16,2>=Buf::new([{i},65535]);\n'
                   for i, name in enumerate(TABLES))


class RenderStage(unittest.TestCase):
    def folder(self):
        root = Path(__file__).resolve().parents[5] / 'target/tmp'
        root.mkdir(parents=True, exist_ok=True)
        temporary = tempfile.TemporaryDirectory(dir=root)
        self.addCleanup(temporary.cleanup)
        return Path(temporary.name)

    def test_metadata_and_mutable_ranges_have_separate_owners(self):
        assets = Assets(self.folder())
        code, layout = prepare(assets, source(), True, True, 2, 3)
        self.assertEqual(layout['metadata_bytes'], 36)
        self.assertEqual(layout['prefix_bytes'], 128)
        self.assertEqual(layout['visibility_bytes'], 28)
        self.assertEqual(layout['ranges_bytes'], 30)
        self.assertEqual(layout['used_bytes'], 186)
        self.assertEqual(layout['spare_bytes'] + layout['used_bytes'], 32768)
        (assets.output / 'assets.json').write_text(json.dumps({'render_stage': layout}))
        for i, name in enumerate(TABLES):
            self.assertEqual(read_table(assets.output, name), [i, 65535])
            self.assertNotIn('pub static ' + name + ':', code)
        layout['tables'][TABLES[0]]['first_word'] = 19
        (assets.output / 'assets.json').write_text(json.dumps({'render_stage': layout}))
        with self.assertRaisesRegex(ValueError, 'exceeds metadata'):
            read_table(assets.output, TABLES[0])

    def test_full_page_is_accepted_and_another_range_is_rejected(self):
        assets = Assets(self.folder())
        _, layout = prepare(assets, source(), True, False, 0, 3264)
        self.assertEqual(layout['used_bytes'], 32768)
        with self.assertRaisesRegex(ValueError, 'limit is 32768'):
            prepare(assets, source(), True, False, 0, 3265)

    def test_ordinary_recording_keeps_its_tables_and_allocates_no_page(self):
        assets = Assets(self.folder())
        code, layout = prepare(assets, source(), False, False, 2, 3)
        self.assertEqual(layout, {'bytes': 0})
        self.assertEqual(assets.payloads, [])
        self.assertIn(source(), code)
        self.assertNotIn('StaticBuffer', (assets.output / 'map_stage.rs').read_text())

    def test_invalid_word_and_declared_length_are_rejected(self):
        assets = Assets(self.folder())
        for malformed in (source().replace('[0,65535]', '[-1,65535]'),
                          source().replace('[0,65535]', '[0]')):
            with self.assertRaisesRegex(ValueError, 'invalid selection table'):
                prepare(assets, malformed, True, True, 2, 3)


if __name__ == '__main__':
    unittest.main()
