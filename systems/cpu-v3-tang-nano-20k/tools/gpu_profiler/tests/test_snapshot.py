"""Synthetic transport/identity cases. These are not GPU rendering measurements."""
import hashlib
import json
import os
from pathlib import Path
import struct
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import snapshot as s


def fixture(abi):
    header = [0] * abi["WORDS"]

    def word(name, value):
        header[abi["H_" + name]] = value

    def pair(name, value):
        at = abi["H_" + name]
        header[at:at + 2] = [value & 65535, value >> 16]

    for name, value in dict(VERSION=1, WORDS=64, FLAGS=1, BINDINGS=1, FRAME=17,
                            SLOT=0, COUNT=3, RANGE_COUNT=2, RANGE_WORDS=8,
                            RANGE_VERSION=1, LAYOUT_ABI=1, COMMAND_ABI=1,
                            MANIFEST_WORDS=16, FRONT_FORMAT=1, TARGET_FORMAT=1).items():
        word(name, value)
    for name, value in dict(TOKEN=0x130004, SUBMISSION=11, COMMAND_BASE=0x490000,
                            COMMAND_BYTES=16, COMMAND_PADDED_BYTES=128,
                            COLOR_BASE=0x430000, DEPTH_BASE=0x460000,
                            FRONT_BASE=0x400000, PLANE_BYTES=192000).items():
        pair(name, value)
    at = abi["H_MANIFEST"]
    header[at:at + 16] = s.bytes_to_words(hashlib.sha256(b"synthetic-build-id").digest())

    def row(flags, tag, base, size):
        result = [0] * abi["RANGE_WORDS"]
        result[abi["R_FLAGS"]] = flags
        result[abi["R_TAG"]] = tag
        for field, value in [("BASE", base), ("BYTES", size), ("GENERATION", 1)]:
            i = abi["R_" + field]
            result[i:i + 2] = [value & 65535, value >> 16]
        return result

    rows = [row(abi["READ"], abi["TAG_COMMANDS"], 0x490000, 128),
            row(abi["READ"] | abi["IMMUTABLE"], abi["TAG_MESH"], 0x100000, 2048)]
    commands = struct.pack("<IIII", 0, 0x40000003, 0x70000000, 0xf0000000) + bytes(112)
    data = s.words_to_bytes([0x1234] * 500 + list(range(524)))
    return header, rows, [commands, data]


def split_frames(raw):
    frames, at = [], 0
    while at < len(raw):
        fields = s.FRAME.unpack_from(raw, at)
        end = at + s.FRAME.size + fields[-1] + 4
        frames.append(s.Frame(s.Kind(fields[2]), fields[4], fields[5], fields[6],
                              raw[at + s.FRAME.size:end - 4]))
        at = end
    return frames


class SnapshotTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.abi = s.Abi(os.environ.get("GPU_CAPTURE_ABI_SOURCE", s.DEFAULT_ABI))

    def setUp(self):
        self.header, self.rows, self.blobs = fixture(self.abi)
        self.raw = s.encode_reference(self.header, self.rows, self.blobs, self.abi)

    def decode(self, raw):
        return s.decode_stream(raw, self.abi)

    def test_round_trip_raw_and_rle(self):
        for compress in [False, True]:
            for chunk in [2, 512, 1024]:
                with self.subTest(compress=compress, chunk=chunk):
                    raw = s.encode_reference(self.header, self.rows, self.blobs, self.abi,
                                             chunk_bytes=chunk, compress=compress, elapsed_ms=500)
                    done, report = self.decode(b"console ready\r\n" + raw)
                    self.assertEqual(report["failures"], [])
                    self.assertEqual(len(done), 1)
                    self.assertEqual(done[0].elapsed_ms, 500)
                    self.assertEqual(done[0].read(0x100000, 2048), self.blobs[1])
                    self.assertEqual(done[0].header["count_minus_one"], 3)

    def test_retries_are_independent_and_identical(self):
        two = s.encode_reference(self.header, self.rows, self.blobs, self.abi, attempt=2)
        done, report = self.decode(self.raw[:-12] + two)
        self.assertTrue(report["failures"])
        self.assertEqual([d.attempt for d in done], [2])
        complete, _ = self.decode(self.raw + two)
        self.assertEqual(len(complete), 2)
        self.assertEqual(complete[0].manifest()["input_sha256"],
                         complete[1].manifest()["input_sha256"])

    def test_manual_gate_notices_do_not_modify_snapshot_abi(self):
        console=b'\nGPUPROF:R\n\nGPUPROF:P\n\nGPUPROF:D\n'
        done,report=self.decode(console+self.raw+b'\nGPUPROF:P\n')
        clean,_=self.decode(self.raw)
        self.assertEqual(report['failures'],[])
        self.assertEqual(len(done),1)
        self.assertEqual(done[0].manifest()['input_sha256'],clean[0].manifest()['input_sha256'])
        self.assertEqual(report['ignored_bytes'],len(console)+len(b'\nGPUPROF:P\n'))

    def test_corrupt_truncated_and_missing_end_never_complete(self):
        corrupt = bytearray(self.raw)
        corrupt[40] ^= 16
        for raw in [corrupt, self.raw[:-1], self.raw[:-48], self.raw[:20], self.raw[:128]]:
            with self.subTest(length=len(raw)):
                done, report = self.decode(raw)
                self.assertEqual(done, [])
                self.assertTrue(report["failures"])

    def test_order_duplicates_and_attempt_mix_rejected(self):
        frames = split_frames(self.raw)
        variants = [frames[:3] + [frames[2]] + frames[3:],
                    [frames[0], frames[2], frames[1]] + frames[3:],
                    [frames[0]] + [s.Frame(f.kind, f.snapshot, 2, f.sequence, f.payload)
                                   for f in frames[1:]]]
        for items in variants:
            done, report = self.decode(b"".join(f.encode() for f in items))
            self.assertFalse(done)
            self.assertTrue(report["failures"])

    def test_offset_codec_length_and_range_crc_rejected(self):
        frames = split_frames(self.raw)
        at = next(i for i, f in enumerate(frames) if f.kind == s.Kind.DATA)
        index, codec, offset, size = s.DATA_HEADER.unpack_from(frames[at].payload)
        for header in [(index, codec, offset + 2, size), (index, 9, offset, size),
                       (index, codec, offset, 1026)]:
            changed = list(frames)
            f = frames[at]
            changed[at] = s.Frame(f.kind, f.snapshot, f.attempt, f.sequence,
                                  s.DATA_HEADER.pack(*header) + f.payload[s.DATA_HEADER.size:])
            self.assertFalse(self.decode(b"".join(x.encode() for x in changed))[0])
        at = next(i for i, f in enumerate(frames) if f.kind == s.Kind.RANGE_END)
        f = frames[at]
        changed = list(frames)
        body = bytearray(f.payload)
        body[-1] ^= 1
        changed[at] = s.Frame(f.kind, f.snapshot, f.attempt, f.sequence, bytes(body))
        self.assertFalse(self.decode(b"".join(x.encode() for x in changed))[0])

    def test_padding_and_early_end_rejected(self):
        for blob in [self.blobs[0][:-1] + b"\1",
                     struct.pack("<IIII", 0xf0000000, 0, 0, 0xf0000000) + bytes(112)]:
            raw = s.encode_reference(self.header, self.rows, [blob, self.blobs[1]], self.abi)
            done, report = self.decode(raw)
            self.assertFalse(done)
            self.assertIn("END" if blob[-1] == 0 else "padding", report["failures"][-1]["error"])

    def test_aliases_must_agree(self):
        row = list(self.rows[0])
        row[self.abi["R_TAG"]] = self.abi["TAG_OBJECTS"]
        self.header[self.abi["H_RANGE_COUNT"]] = 3
        for last in [self.blobs[0], bytes(128)]:
            raw = s.encode_reference(self.header, self.rows + [row], self.blobs + [last], self.abi)
            done, _ = self.decode(raw)
            self.assertEqual(bool(done), last == self.blobs[0])

    def test_bounds_and_invalid_descriptors(self):
        for field, value in [("COUNT", 4), ("GPU_STATUS", 1), ("MANIFEST_WORDS", 15),
                             ("SLOT", 2), ("RANGE_COUNT", 65), ("WORDS", 63)]:
            header = list(self.header)
            header[self.abi["H_" + field]] = value
            with self.subTest(field=field), self.assertRaises(s.SnapshotError):
                self.abi.header(header)
        done, _ = self.decode(self.raw)
        with self.assertRaises(s.SnapshotError):
            done[0].read(0x600000, 2)

    def test_save_load_recheck_and_tamper(self):
        done, _ = self.decode(self.raw)
        with tempfile.TemporaryDirectory() as temp:
            done[0].save(temp)
            loaded = s.load_snapshot(temp, self.abi)
            self.assertEqual(loaded.manifest(), done[0].manifest())
            with self.assertRaises(s.SnapshotError):
                done[0].save(temp)
            path = Path(temp) / "range-001.bin"
            path.write_bytes(bytes(len(self.blobs[1])))
            with self.assertRaises(s.SnapshotError):
                s.load_snapshot(temp, self.abi)

    def test_saved_paths_and_identity_not_trusted(self):
        done, _ = self.decode(self.raw)
        for key, value in [("input_sha256", "0" * 64), ("abi_sha256", "0" * 64),
                           ("complete", False), ("replay_policy", "warm")]:
            with tempfile.TemporaryDirectory() as temp:
                metadata = done[0].save(temp)
                metadata[key] = value
                (Path(temp) / "snapshot.json").write_text(json.dumps(metadata), encoding="utf-8")
                with self.subTest(key=key), self.assertRaises(s.SnapshotError):
                    s.load_snapshot(temp, self.abi)
        with tempfile.TemporaryDirectory() as temp:
            metadata = done[0].save(temp)
            metadata["ranges"][0]["file"] = "../range-000.bin"
            (Path(temp) / "snapshot.json").write_text(json.dumps(metadata), encoding="utf-8")
            with self.assertRaises(s.SnapshotError):
                s.load_snapshot(temp, self.abi)

    def test_rle_decoder_bounded(self):
        for payload, expected in [(b"\xff\xff\0\0", 512), (b"\0\x80", 2),
                                  (b"\x02\0\0\0", 6), (b"\0", 2), (b"", 2)]:
            with self.assertRaises(s.SnapshotError):
                s.rle16_decode(payload, expected)
        for data in [bytes(1024), s.words_to_bytes(list(range(512))), b"\1\0" * 512]:
            self.assertEqual(s.rle16_decode(s.rle16_encode(data), len(data)), data)

    def test_false_magic_huge_length_and_unknown_kind(self):
        for field, value in [(1, 2), (2, 19), (3, 1), (7, 0xffffffff)]:
            fields = list(s.FRAME.unpack_from(self.raw))
            fields[field] = value
            body = s.FRAME.pack(*fields) + self.raw[s.FRAME.size:s.FRAME.size + 128]
            raw = body + s.CRC.pack(s.checksum(body))
            done, report = self.decode(raw)
            self.assertFalse(done)
            self.assertTrue(report["failures"])


if __name__ == "__main__":
    unittest.main()
