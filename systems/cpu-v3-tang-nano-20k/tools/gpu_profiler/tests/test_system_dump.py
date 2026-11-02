"""Shared transport contract checks; synthetic bytes are not a GPU run."""
import json
import hashlib
import os
import shutil
import subprocess
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

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

    def test_prefix_mode_reaches_frontend_and_preserves_origin(self):
        for mode in (None, "reject", "functional"):
            with self.subTest(mode=mode):
                name = str(mode)
                program = self.root / "program.bin"
                program.write_bytes(b"identity-only: frontend is mocked")
                recipe = dict(schema=1, format="cb06", program=dict(file=program.name,
                              sha256=hashlib.sha256(program.read_bytes()).hexdigest()),
                              capture_point=dict(gate_pc=10, header_word=100, ranges_word=200),
                              memory_seed=0, max_steps=256, timeout_seconds=5)
                if mode is not None:
                    recipe["prefix_gpu"] = mode
                path = self.root / (name + ".json")
                path.write_text(json.dumps(recipe))
                actual_mode = mode or "reject"
                def frontend(args, **kwargs):
                    self.assertEqual("--prefix-gpu" in args, actual_mode == "functional")
                    if actual_mode == "functional":
                        self.assertEqual(args[-2:], ["--prefix-gpu", "functional"])
                    raw = Path(args[args.index("--out") + 1])
                    shutil.copytree(self.raw, raw)
                    if actual_mode == "functional":
                        origin = dict(self.origin, prefix_gpu="functional")
                        (raw / "origin.json").write_text(json.dumps(origin))
                    return subprocess.CompletedProcess(args, 0)
                out = self.root / ("out-" + name)
                with patch.object(system_dump.subprocess, "run", side_effect=frontend):
                    system_dump.capture(path, self.root / "frontend.exe", out)
                source = json.loads((out / "snapshot/source.json").read_text())
                self.assertEqual(source.get("prefix_gpu", "reject"), actual_mode)
                self.assertFalse(source["gpu_consumed"])

    def test_unknown_or_mismatched_prefix_mode_cannot_publish_snapshot(self):
        program = self.root / "program.bin"
        program.write_bytes(b"identity-only")
        recipe = dict(schema=1, format="cb06", program=dict(file=program.name,
                      sha256=hashlib.sha256(program.read_bytes()).hexdigest()),
                      prefix_gpu="guess", capture_point=dict(gate_pc=1, header_word=2, ranges_word=3),
                      memory_seed=0, max_steps=256, timeout_seconds=5)
        path = self.root / "recipe.json"
        path.write_text(json.dumps(recipe))
        with self.assertRaisesRegex(snapshot.SnapshotError, "prefix_gpu"):
            system_dump.capture(path, self.root / "missing.exe", self.root / "bad-mode")
        self.assertFalse((self.root / "bad-mode").exists())
        recipe["prefix_gpu"] = "functional"
        path.write_text(json.dumps(recipe))
        def stale_frontend(args, **kwargs):
            shutil.copytree(self.raw, Path(args[args.index("--out") + 1]))
            return subprocess.CompletedProcess(args, 0)
        out = self.root / "stale-mode"
        with patch.object(system_dump.subprocess, "run", side_effect=stale_frontend):
            with self.assertRaisesRegex(snapshot.SnapshotError, "differs"):
                system_dump.capture(path, self.root / "frontend.exe", out)
        self.assertFalse((out / "snapshot").exists())

    def test_native_recipe_selects_clock_budget_and_preserves_backend(self):
        program = self.root / "native-program.bin"
        program.write_bytes(b"mocked native frontend identity")
        recipe = dict(schema=1, format="cb06", cpu_mode="native", prefix_gpu="functional",
                      program=dict(file=program.name, sha256=hashlib.sha256(program.read_bytes()).hexdigest()),
                      capture_point=dict(gate_pc=10, header_word=100, ranges_word=200),
                      memory_seed=0, max_cycles=20000, timeout_seconds=5)
        path = self.root / "native.json"
        path.write_text(json.dumps(recipe))
        def frontend(args, **kwargs):
            self.assertNotIn("--max-steps", args)
            self.assertEqual(args[args.index("--max-cycles")+1], "20000")
            self.assertEqual(args[args.index("--cpu-mode")+1], "native")
            raw = Path(args[args.index("--out")+1])
            shutil.copytree(self.raw, raw)
            origin = dict(self.origin, cpu_mode="native", cpu_steps=None,
                          cpu_logic_cycles=12000, native_cycles=24000, prefix_gpu="functional")
            (raw / "origin.json").write_text(json.dumps(origin))
            return subprocess.CompletedProcess(args, 0)
        out = self.root / "native-output"
        with patch.object(system_dump.subprocess, "run", side_effect=frontend):
            system_dump.capture(path, self.root / "frontend.exe", out)
        origin = json.loads((out / "snapshot/source.json").read_text())
        self.assertEqual(origin["cpu_mode"], "native")
        self.assertIsNone(origin["cpu_steps"])
        self.assertEqual(origin["cpu_logic_cycles"], 12000)

    def test_unknown_backend_wrong_budget_and_stale_native_origin_are_refused(self):
        program = self.root / "program.bin"
        program.write_bytes(b"identity-only")
        base = dict(schema=1, format="cb06", cpu_mode="native",
                    program=dict(file=program.name, sha256=hashlib.sha256(program.read_bytes()).hexdigest()),
                    capture_point=dict(gate_pc=1, header_word=2, ranges_word=3),
                    memory_seed=0, max_cycles=20000, timeout_seconds=5)
        path = self.root / "backend.json"
        for change in [dict(cpu_mode="guess"), dict(max_steps=100), dict(max_cycles=0), dict(max_cycles=True)]:
            with self.subTest(change=change):
                path.write_text(json.dumps(dict(base, **change)))
                with self.assertRaises(snapshot.SnapshotError):
                    system_dump.capture(path, self.root / "missing.exe", self.root / "bad-backend")
                self.assertFalse((self.root / "bad-backend").exists())
        path.write_text(json.dumps(base))
        def stale(args, **kwargs):
            shutil.copytree(self.raw, Path(args[args.index("--out")+1]))
            return subprocess.CompletedProcess(args, 0)
        with patch.object(system_dump.subprocess, "run", side_effect=stale):
            with self.assertRaisesRegex(snapshot.SnapshotError, "backend differs"):
                system_dump.capture(path, self.root / "frontend.exe", self.root / "stale-backend")
        self.assertFalse((self.root / "stale-backend/snapshot").exists())
        for origin in [dict(cpu_mode="native", cpu_steps=1, cpu_logic_cycles=10, native_cycles=20),
                       dict(cpu_mode="native", cpu_steps=None, cpu_logic_cycles=0, native_cycles=20)]:
            self.origin.update(origin)
            self.save_origin()
            with self.assertRaisesRegex(snapshot.SnapshotError, "modeled clocks"):
                system_dump.materialize(self.raw, self.root / "bad-native-origin", {}, self.abi)

    @unittest.skipUnless(os.environ.get("GPU_DUMP_FRONTEND") and os.environ.get("CPU_NATIVE_PREPARED_RECIPE"),
                         "requires built frontend and actual compiled Prepared recipe")
    def test_actual_prepared_recipe_uses_both_backends_and_same_input(self):
        path = Path(os.environ["CPU_NATIVE_PREPARED_RECIPE"]).resolve()
        original = json.loads(path.read_text())
        manifests = []
        for mode in ("instruction", "native"):
            recipe = dict(original)
            recipe["program"] = dict(original["program"], file=str((path.parent/original["program"]["file"]).resolve()))
            recipe["cpu_mode"] = mode
            recipe.pop("max_steps", None)
            recipe.pop("max_cycles", None)
            recipe["max_cycles" if mode == "native" else "max_steps"] = 4_000_000
            recipe_path = self.root / (mode + "-recipe.json")
            recipe_path.write_text(json.dumps(recipe))
            out = self.root / mode
            manifests.append(system_dump.capture(recipe_path, Path(os.environ["GPU_DUMP_FRONTEND"]), out))
            origin = json.loads((out/"snapshot/source.json").read_text())
            self.assertEqual(origin["cpu_mode"], mode)
            self.assertFalse(origin["gpu_consumed"])
            if mode == "native":
                self.assertIsNone(origin["cpu_steps"])
                self.assertGreater(origin["cpu_logic_cycles"], 0)
                self.assertEqual((out/"raw/physical.bin").stat().st_size, 0x800000)
        self.assertEqual(manifests[0]["input_sha256"], manifests[1]["input_sha256"])

    @unittest.skipUnless(os.environ.get("GPU_DUMP_FRONTEND") and os.environ.get("CPU_PREFIX_CAPTURE_TEST_OUT"),
                         "requires built dump CLI and current Rust-generated prefix fixture")
    def test_actual_cli_keeps_prefix_writes_and_freezes_before_target(self):
        fixture = Path(os.environ["CPU_PREFIX_CAPTURE_TEST_OUT"])
        point = json.loads((fixture / "point.json").read_text())
        base = [os.environ["GPU_DUMP_FRONTEND"], "--flash", str(fixture / "prefix.bin"),
                "--memory-seed", str(0xa55a)]
        for key in ("gate_pc", "header_word", "ranges_word"):
            base += ["--" + key.replace("_", "-"), str(point[key])]
        for mode, steps in (("reject", 256), ("functional", 256), ("functional", 1), ("guess", 256)):
            with self.subTest(mode=mode, steps=steps):
                out = self.root / f"actual-{mode}-{steps}"
                args = base + ["--out", str(out), "--max-steps", str(steps)]
                # Exercise the legacy CLI with its mode flag absent.
                if mode != "reject":
                    args += ["--prefix-gpu", mode]
                result = subprocess.run(args, capture_output=True, text=True, timeout=5)
                if mode != "functional" or steps == 1:
                    self.assertNotEqual(result.returncode, 0)
                    self.assertFalse(out.exists(), "failed capture must not create a dump")
                    continue
                self.assertEqual(result.returncode, 0, result.stderr)
                origin = json.loads((out / "origin.json").read_text())
                self.assertEqual(origin["prefix_gpu"], "functional")
                self.assertFalse(origin["gpu_consumed"])
                self.assertEqual(origin["gate_pc"], point["gate_pc"])
                self.assertEqual((out / "range-000.bin").read_bytes(), b"\x57\x13" * (400 * 256))
                self.assertEqual((out / "range-001.bin").read_bytes(), b"\x5a\xa5" * (400 * 256))


if __name__ == "__main__":
    unittest.main()
