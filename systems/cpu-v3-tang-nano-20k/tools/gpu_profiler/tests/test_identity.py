"""Identity/closure negative cases with synthetic artifacts, never board receipts."""
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import identity as i
import replay
import snapshot as s
from test_snapshot import fixture
from layout import COLUMN, COMMAND_ABI


class IdentityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.abi = s.Abi(os.environ.get("GPU_CAPTURE_ABI_SOURCE", s.DEFAULT_ABI))

    def create(self, folder, planes=True, layout=COLUMN, guards=True):
        root = Path(folder)
        header, rows, blobs = fixture(self.abi)
        mesh_address = 0x100000
        color, depth = 0x430000, 0x460000
        if layout == COLUMN:
            mesh_address, color, depth = 0x4e0000, 0x440000, 0x480000
            header[self.abi['H_LAYOUT_ABI']] = 2
            for name, value in [('COMMAND_BASE', 0x4c0000), ('COLOR_BASE', color),
                                ('DEPTH_BASE', depth), ('PLANE_BYTES', layout.plane_bytes)]:
                at = self.abi['H_' + name]
                header[at:at + 2] = [value & 65535, value >> 16]
            for row, address in [(rows[0], 0x4c0000), (rows[1], mesh_address)]:
                at = self.abi['R_BASE']
                row[at:at + 2] = [address & 65535, address >> 16]
        for tag, address in [("COLOR", color), ("DEPTH", depth), ("FRONT", 0x400000)]:
            if not planes:
                break
            row = [0] * 8
            row[self.abi["R_FLAGS"]] = self.abi["READ"] | self.abi["PREIMAGE"]
            row[self.abi["R_TAG"]] = self.abi["TAG_" + tag]
            row[self.abi["R_BASE"]:self.abi["R_BASE"] + 2] = [address & 65535, address >> 16]
            span=layout.allocation_bytes if guards else layout.plane_bytes
            row[self.abi["R_BYTES"]:self.abi["R_BYTES"] + 2] = [span & 65535, span >> 16]
            rows.append(row)
            blobs.append(bytes(span))
        header[self.abi["H_RANGE_COUNT"]] = len(rows)
        spec = dict(files=[dict(name="synthetic-source", path="source.txt")],
                    resources=[dict(name="synthetic-mesh", address=mesh_address, bytes=2048,
                                    sha256=hashlib.sha256(blobs[1]).hexdigest())],
                    options={"test_only": True}, toolchain={"test_only": True}, source_commit="test-only")
        if layout == COLUMN:
            spec['layout'] = layout.description()
            spec['options']['capture_allocation_guards']=guards
            spec['resources'][0]['kind'] = 'mesh'
        (root / "source.txt").write_bytes(b"synthetic-code-not-hardware")
        (root / "spec.json").write_text(json.dumps(spec), encoding="utf-8")
        build = i.create_build(root / "spec.json", root / "build.json")
        at = self.abi["H_MANIFEST"]
        header[at:at + 16] = s.bytes_to_words(bytes.fromhex(build["manifest_id"]))
        (root / "firmware.bin").write_bytes(b"synthetic-firmware")
        (root / "bitstream.fs").write_bytes(b"synthetic-bitstream")
        receipt = dict(schema="digitaldesign.gpu-programming-receipt", version=1,
                       manifest_id=build["manifest_id"], board_id="UNIT-TEST-NOT-A-BOARD",
                       programmed_at="synthetic-test-only", verified=True)
        for kind, name in [("firmware", "firmware.bin"), ("bitstream", "bitstream.fs")]:
            receipt[kind] = dict(path=name, bytes=(root / name).stat().st_size, sha256=i.digest(root / name))
        (root / "receipt.json").write_text(json.dumps(receipt), encoding="utf-8")
        captures, report = s.decode_stream(s.encode_reference(header, rows, blobs, self.abi), self.abi)
        self.assertFalse(report["failures"])
        return captures[0], root / "build.json", root / "receipt.json"

    def test_complete_extents_and_bound_receipt(self):
        with tempfile.TemporaryDirectory() as temp:
            capture, build, receipt = self.create(temp)
            result = i.preflight(capture, build, receipt)
            self.assertEqual(len(result["extents"]), 5)
            self.assertIn("dynamic RTL", result["dependency_closure"])

    def test_input_only_is_explicitly_unverified_and_still_checks_preimages(self):
        with tempfile.TemporaryDirectory() as temp:
            capture, build, receipt = self.create(temp)
            result = i.input_preflight(capture)
            self.assertFalse(result['build_identity_verified'])
            self.assertFalse(result['verified_board_identity'])
            self.assertIn('firmware/bitstream unverified', result['policy'])
            with patch.object(capture, 'read', side_effect=s.SnapshotError('missing preimage')):
                with self.assertRaises(s.SnapshotError):
                    i.input_preflight(capture)
            # The explicit qualified route never falls back after a bad receipt.
            Path(receipt).write_text('{}')
            with self.assertRaises(s.SnapshotError):
                i.preflight(capture, build, receipt)

    def test_system_simulation_uses_bound_source_without_board_receipt(self):
        with tempfile.TemporaryDirectory() as temp:
            capture, build, receipt = self.create(temp)
            source = Path(temp) / 'source.json'
            record = dict(schema=1, source='system-simulation', boundary='before-profiler-gate',
                          gpu_consumed=False, input_sha256=capture.manifest()['input_sha256'],
                          compiled_program_sha256='a'*64, recipe_sha256='b'*64)
            source.write_text(json.dumps(record))
            result = i.preflight(capture, build, simulation_source=source)
            self.assertEqual(result['source_kind'], 'system-simulation')
            self.assertNotIn('board_id', result)
            with self.assertRaises(s.SnapshotError):
                i.preflight(capture, build, receipt, simulation_source=source)
            record['input_sha256'] = 'c'*64
            source.write_text(json.dumps(record))
            with self.assertRaises(s.SnapshotError):
                i.preflight(capture, build, simulation_source=source)

    def test_interrupted_backend_writes_incomplete_receipt(self):
        with tempfile.TemporaryDirectory() as temp:
            capture, _, _ = self.create(temp)
            manifest = Path(temp) / 'manifest.json'
            manifest.write_text(json.dumps(dict(command_abi=COMMAND_ABI)))
            with patch('replay.capture_run', side_effect=RuntimeError('bounded stop')):
                with self.assertRaisesRegex(RuntimeError, 'bounded stop'):
                    replay.execute(capture, manifest)
            self.assertFalse(json.loads((Path(temp) / 'replay.json').read_text())['complete'])

    def test_missing_plane_cannot_be_assumed_cleared(self):
        with tempfile.TemporaryDirectory() as temp:
            capture, build, receipt = self.create(temp, planes=False)
            with self.assertRaises(s.SnapshotError):
                i.preflight(capture, build, receipt)

    def test_source_and_programmed_bytes_must_match(self):
        for filename in ["source.txt", "firmware.bin", "bitstream.fs"]:
            with self.subTest(file=filename), tempfile.TemporaryDirectory() as temp:
                capture, build, receipt = self.create(temp)
                (Path(temp) / filename).write_bytes(b"changed")
                with self.assertRaises(s.SnapshotError):
                    i.preflight(capture, build, receipt)

    def test_header_build_and_receipt_mismatch(self):
        for target in ["header", "receipt", "catalog"]:
            with self.subTest(target=target), tempfile.TemporaryDirectory() as temp:
                capture, build, receipt = self.create(temp)
                if target == "header":
                    capture.header["manifest_id"] = "0" * 64
                elif target == "receipt":
                    value = json.loads(receipt.read_text())
                    value["verified"] = False
                    receipt.write_text(json.dumps(value), encoding="utf-8")
                else:
                    capture.data[1][3] ^= 1
                with self.assertRaises(s.SnapshotError):
                    i.preflight(capture, build, receipt)

    def test_physical_bank_layout(self):
        for word in [0, 1, 31, 32, 63, 64, 127, 128, 1023, 100_000, 2_097_151]:
            index = replay.physical(word)
            bank, remaining = index >> 19, index & ((1 << 19) - 1)
            restored = (remaining & 31) | (bank << 5) | ((remaining >> 5) << 7)
            self.assertEqual(restored, word)
        self.assertEqual(replay.physical(32), 1 << 19)
        self.assertEqual(replay.physical(128), 32)

    def test_preview_keeps_native_pixel_dimensions(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "image.png"
            replay.preview(b"\xff\xff" * (204800//2), path, linear=True)
            self.assertEqual(path.read_bytes()[:8], b"\x89PNG\r\n\x1a\n")
            self.assertEqual(int.from_bytes(path.read_bytes()[16:20], "big"), 400)
            self.assertEqual(int.from_bytes(path.read_bytes()[20:24], "big"), 240)


if __name__ == "__main__":
    unittest.main()
