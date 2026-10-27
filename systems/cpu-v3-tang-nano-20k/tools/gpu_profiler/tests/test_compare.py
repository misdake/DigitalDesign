"""Adjacent-lease association and exact output checks, using synthetic planes."""
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import snapshot as s
from compare import compare
import test_identity as fixture_helpers


class CompareTests(unittest.TestCase):
    create = fixture_helpers.IdentityTests.create

    @classmethod
    def setUpClass(cls):
        cls.abi = s.Abi(os.environ.get("GPU_CAPTURE_ABI_SOURCE", s.DEFAULT_ABI))

    def cases(self, folder):
        previous, _, _ = self.create(folder)
        header = list(previous.descriptor)
        for field, value in [("TOKEN", previous.header["token"] + 1),
                             ("SUBMISSION", previous.header["submission"] + 1),
                             ("FRONT_BASE", previous.header["color_base"]),
                             ("COLOR_BASE", previous.header["front_base"])]:
            at = self.abi["H_" + field]
            header[at:at + 2] = [value & 65535, value >> 16]
        raw = s.encode_reference(header, [r.words for r in previous.ranges],
                                 [bytes(previous.data[r.index]) for r in previous.ranges], self.abi)
        following = s.decode_stream(raw, self.abi)[0][0]
        replay = Path(folder) / "replay"
        (replay / "capture").mkdir(parents=True)
        metadata = dict(complete=True, input_sha256=previous.manifest()["input_sha256"])
        for role in ["color", "depth"]:
            raw = bytes(192000)
            (replay / "capture" / (role + ".bin")).write_bytes(raw)
            metadata[role + "_sha256"] = hashlib.sha256(raw).hexdigest()
        (replay / "replay.json").write_text(json.dumps(metadata), encoding="utf-8")
        return previous, following, replay

    def test_adjacent_exact_outputs(self):
        with tempfile.TemporaryDirectory() as temp:
            a, b, replay = self.cases(temp)
            report = compare(a, b, replay, Path(temp) / "result.json")
            self.assertTrue(report["equal"])
            self.assertEqual(report["planes"]["color"]["different_pixels"], 0)

    def test_wrong_lease_front_and_input_refused(self):
        for key in ["front_base", "submission", "manifest_id"]:
            with self.subTest(key=key), tempfile.TemporaryDirectory() as temp:
                a, b, replay = self.cases(temp)
                b.header[key] = "0" * 64 if key == "manifest_id" else 0
                with self.assertRaises(s.SnapshotError):
                    compare(a, b, replay, Path(temp) / "result.json")

    def test_output_corruption_and_pixel_location(self):
        with tempfile.TemporaryDirectory() as temp:
            a, b, replay = self.cases(temp)
            path = replay / "capture/color.bin"
            raw = bytearray(path.read_bytes())
            raw[14:16] = b"\xff\xff"
            path.write_bytes(raw)
            with self.assertRaises(s.SnapshotError):
                compare(a, b, replay, Path(temp) / "corrupt.json")
            metadata = json.loads((replay / "replay.json").read_text())
            metadata["color_sha256"] = hashlib.sha256(raw).hexdigest()
            (replay / "replay.json").write_text(json.dumps(metadata), encoding="utf-8")
            report = compare(a, b, replay, Path(temp) / "result.json")
            self.assertFalse(report["equal"])
            plane = report["planes"]["color"]
            self.assertEqual(plane["different_pixels"], 1)
            self.assertEqual((plane["examples"][0]["x"], plane["examples"][0]["y"]), (7, 0))


if __name__ == "__main__":
    unittest.main()
