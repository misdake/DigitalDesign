"""Small deterministic capture contract; no RTL, scene assets or frozen baseline."""
import copy
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from model import build_capture, load_profile, validate, write_report
from capture import run as capture_run


class Profiles(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture = ROOT / "tests/fixtures/pingpong"
        cls.data = build_capture(cls.fixture)

    def test_aba_occurrences_and_owners(self):
        d = self.data
        self.assertEqual([s["base"] for s in d["submissions"]], [4096, 8192, 4096])
        self.assertEqual([c["pc"] for c in d["commands"]], [0, 4, 8] * 3)
        self.assertEqual(len({c["id"] for c in d["commands"]}), 9)
        self.assertEqual([m["command"] for m in d["meshlets"]], [2, 5, 8])
        self.assertEqual([j["command"] for j in d["jobs"]], [1, 4, 7])
        self.assertEqual([s["quad_counts"]["allocated"] for s in d["submissions"]], [1] * 3)

    def test_cpu_overlap_and_crop(self):
        self.assertLess(self.data["cpu"][1]["start"], self.data["submissions"][0]["end"])
        self.assertEqual(self.data["cpu"][-1]["frame_id"], 4)
        self.assertTrue(self.data["cpu"][-1]["partial"])
        self.assertFalse(self.data["partial"])

    def test_backend_commit_tail(self):
        for m in self.data["meshlets"]:
            self.assertGreater(m["last_quad"]["end"], m["last_quad"]["pool_retire"])
            self.assertGreater(m["last_quad"]["end"], m["vt_end"])
            self.assertEqual(m["p"][-1][1], m["last_quad"]["end"])

    def test_schema_and_identity_rejections(self):
        for mutate in [
            lambda d: d.update(version=999),
            lambda d: d.update(clock_hz=0),
            lambda d: d["commands"][3].update(id=1),
            lambda d: d["commands"][3].update(submission=1),
            lambda d: d["meshlets"][1].update(command=2),
            lambda d: d["commands"][0].update(pc=1),
            lambda d: d["tracks"]["r_busy"].append([1, 1]),
        ]:
            d = copy.deepcopy(self.data)
            mutate(d)
            with self.assertRaises(ValueError):
                validate(d)

    def test_truncated_capture_explicit_partial(self):
        with tempfile.TemporaryDirectory() as temp:
            folder = Path(temp)
            for p in self.fixture.iterdir():
                shutil.copyfile(p, folder / p.name)
            path = folder / "events.txt"
            path.write_text("\n".join(line for line in path.read_text().splitlines()
                                     if not line.startswith(("DONE 121", "CAPTURE_END"))) + "\nCMD_END 12")
            with self.assertRaises(ValueError):
                build_capture(folder)
            d = build_capture(folder, True)
            self.assertTrue(d["partial"])
            self.assertIsNone(d["submissions"][-1]["end"])

    def test_report_roundtrip_and_safe_embedding(self):
        with tempfile.TemporaryDirectory() as temp:
            d = copy.deepcopy(self.data)
            d["title"] = "</script><script>bad()</script>"
            write_report(d, temp)
            html = (Path(temp) / "profiler.html").read_text(encoding="utf-8")
            self.assertEqual(html.count("<script>"), 1)
            self.assertNotIn(d["title"], html)
            loaded = load_profile(Path(temp) / "profile.json")
            self.assertEqual(loaded, d)
            self.assertIn("submission", (Path(temp) / "commands.csv").read_text())

    def test_cli_without_external_capture_dependencies(self):
        with tempfile.TemporaryDirectory() as temp:
            subprocess.run([sys.executable, str(ROOT / "profiler.py"), "build",
                            str(self.fixture), "--out", temp], check=True, capture_output=True,
                           timeout=15)
            self.assertEqual(json.loads((Path(temp) / "profile.json").read_text())["version"], 2)

    def test_legacy_v1_without_frozen_capture(self):
        d = copy.deepcopy(self.data)
        d.pop("schema"); d.pop("submissions"); d["version"] = 1
        for index, command in enumerate(d["commands"]):
            command["pc"] = index * 4
        pc = {c["id"]: c["pc"] for c in d["commands"]}
        for item in [*d["meshlets"], *d["jobs"]]:
            item["pc"] = pc[item["command"]]
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "legacy.json"
            path.write_text(json.dumps(d))
            loaded = load_profile(path)
            self.assertEqual(loaded["version"], 2)
            self.assertEqual(len(loaded["submissions"]), 1)
            self.assertEqual([m["command"] for m in loaded["meshlets"]], [2, 5, 8])
            self.assertIn("imported_v1_sha256", loaded["provenance"])

    def test_capture_input_identity(self):
        with tempfile.TemporaryDirectory() as temp:
            folder = Path(temp)
            for p in self.fixture.iterdir():
                shutil.copyfile(p, folder / p.name)
            config = folder / "capture.json"
            metadata = json.loads(config.read_text())
            metadata["inputs_sha256"] = {"load.csv": hashlib.sha256((folder / "load.csv").read_bytes()).hexdigest()}
            config.write_text(json.dumps(metadata))
            with (folder / "load.csv").open("a") as stream:
                stream.write("tampered")
            with self.assertRaisesRegex(ValueError, "evidence changed"):
                build_capture(folder)

    def test_capture_output_names_cannot_overwrite_sources_or_inputs(self):
        for kind in ("source", "input"):
            for name in ("profiler_tb.v", "CAPTURE.JSON", "events.txt", "load.csv"):
                with self.subTest(kind=kind, name=name), tempfile.TemporaryDirectory() as temp:
                    folder = Path(temp)
                    source = folder / (name if kind == "source" else "dut.v")
                    source.write_text("module dut; endmodule\n", encoding="utf-8")
                    original = source.read_bytes()
                    config = dict(version=1, max_cycles=10, timeout_seconds=1,
                                  submissions=1, clock_hz=54000000, sources=[source.name])
                    if kind == "input":
                        config["inputs"] = {name: source.name}
                    manifest = folder / "manifest.json"
                    manifest.write_text(json.dumps(config), encoding="utf-8")
                    with self.assertRaisesRegex(ValueError, "reserved|unique flat"):
                        capture_run(manifest, folder / "out", compile_only=True)
                    self.assertEqual(source.read_bytes(), original)


if __name__ == "__main__":
    unittest.main()
