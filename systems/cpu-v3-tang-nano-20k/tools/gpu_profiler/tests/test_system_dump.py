"""Shared transport contract checks; synthetic bytes are not a GPU run."""
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import snapshot
import system_dump
from test_snapshot import fixture


class SystemDumpTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.raw = self.root / "raw"
        self.raw.mkdir()
        self.abi = snapshot.Abi()
        self.header, self.rows, self.blobs = fixture(self.abi)
        (self.raw / "header.bin").write_bytes(snapshot.words_to_bytes(self.header))
        (self.raw / "ranges.bin").write_bytes(snapshot.words_to_bytes([v for row in self.rows for v in row]))
        for i, data in enumerate(self.blobs):
            (self.raw / f"range-{i:03}.bin").write_bytes(data)
        self.origin = dict(schema=1, source="system-simulation", boundary="before-profiler-gate",
                           gpu_consumed=False, range_count=len(self.rows))
        self.save_origin()

    def save_origin(self):
        (self.raw / "origin.json").write_text(json.dumps(self.origin))

    def test_materialized_content_matches_uart_decoder(self):
        system_dump.materialize(self.raw, self.root / "snapshot", {"compiled_program_sha256": "test"}, self.abi)
        actual = snapshot.load_snapshot(self.root / "snapshot", self.abi)
        wire = snapshot.encode_reference(self.header, self.rows, self.blobs, self.abi)
        uart, errors = snapshot.decode_stream(wire, self.abi)
        self.assertFalse(errors["failures"])
        self.assertEqual(actual.manifest()["input_sha256"], uart[0].manifest()["input_sha256"])
        self.assertEqual(actual.descriptor, uart[0].descriptor)
        self.assertEqual(actual.data, uart[0].data)

    def test_consumed_submission_is_refused(self):
        self.origin["gpu_consumed"] = True
        self.save_origin()
        with self.assertRaisesRegex(snapshot.SnapshotError, "pre-consumption"):
            system_dump.materialize(self.raw, self.root / "snapshot", {}, self.abi)
        self.assertFalse((self.root / "snapshot").exists())

    def test_short_payload_cannot_create_complete_snapshot(self):
        (self.raw / "range-000.bin").write_bytes(b"\x00\x00")
        with self.assertRaisesRegex(snapshot.SnapshotError, "length"):
            system_dump.materialize(self.raw, self.root / "snapshot", {}, self.abi)
        self.assertFalse((self.root / "snapshot").exists())

    def test_changed_program_is_refused_before_execution(self):
        (self.root / "program.bin").write_bytes(b"changed")
        recipe = self.root / "recipe.json"
        recipe.write_text(json.dumps(dict(schema=1, format="cb06", program=dict(file="program.bin", sha256="0" * 64))))
        with self.assertRaisesRegex(snapshot.SnapshotError, "identity"):
            system_dump.capture(recipe, self.root / "missing.exe", self.root / "output")
        self.assertFalse((self.root / "output").exists())


if __name__ == "__main__":
    unittest.main()
