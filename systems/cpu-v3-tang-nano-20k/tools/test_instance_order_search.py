"""Fast boundary/fitness/checkpoint tests; no synthesis or simulator subprocesses."""
import json
import io
import os
import threading
from pathlib import Path
import random
import tempfile
import unittest
from unittest import mock

import instance_order_search as search


def template():
    return ("module CpuV3System;\r\n/*\r\nendmodule\r\n*/\r\nwire [3:0] x;\r\n"
            + "".join('child #(.P((3)), .TEXT(";)")) ' + n + '( .a(x) /* ) */ );\r\n'
                      for n in search.NAMES)
            + "assign x=4'b0;\r\nendmodule\r\nmodule fixed_helper;\r\nendmodule\r\n")


class InstanceOrderTests(unittest.TestCase):
    def test_complete_blocks_only_and_byte_preservation(self):
        source = template()
        order = list(reversed(search.NAMES))
        changed = search.permute(source, order)
        self.assertEqual(search.structure(source, changed)["order"], order)
        self.assertIn("assign x=4'b0;\r\nchild", changed)
        self.assertTrue(changed.endswith("module fixed_helper;\r\nendmodule\r\n"))
        self.assertEqual(search.permute(changed, order), changed)
        with self.assertRaises(ValueError):
            search.structure(source, changed.replace(".a(x)", ".a(0)", 1))
        with self.assertRaises(ValueError):
            search.structure(source, changed.replace("wire [3:0]", "wire [2:0]"))
        with self.assertRaises(ValueError):
            search.permute(source, order[:-1] + [order[0]])

    def test_elaboration_aliases_widths_and_json_roundtrip(self):
        headers = "\n".join("tang_nano_20k_ds2_top.u_logic." + n + " module <child> instance 0 children, 0 classes" for n in search.NAMES)
        body = ("    wire: a unpacked dims=0 pin_count=1 input netvector_t:logic unsigned[3:0] (eref=0, lref=0) scope=top #(0,0,0) vector_width=4 pin_count=1\n"
                "        [0]: 0x123 top.a\n"
                "    wire: b unpacked dims=0 pin_count=1 output netvector_t:logic unsigned[3:0] (eref=0, lref=0) scope=top #(0,0,0) vector_width=4 pin_count=1\n"
                "        [0]: 0x123 top.a\n")
        first = search.net_manifest(headers + "\n" + body)
        self.assertEqual(first, json.loads(json.dumps(first)))
        self.assertEqual(first, search.net_manifest(headers + "\n" + body.replace("0x123", "0x456")))
        self.assertNotEqual(first, search.net_manifest(headers + "\n" + body.replace("[0]: 0x123 top.a", "[0]: 0x456 top.a", 1)))
        self.assertNotEqual(first, search.net_manifest(headers + "\n" + body.replace("vector_width=4", "vector_width=3")))

    def test_fitness_prefers_valid_timing_then_area(self):
        base = dict(valid=True, qualified=False, tool_exit=0, setup_violations=1,
                    unrouted_nets=0, setup_wns_ns=-0.2, hold_wns_ns=0.1,
                    setup_reported_negative_slack_sum_ns=-0.2,
                    hold_reported_negative_slack_sum_ns=0, area={"Logic": 19000})
        self.assertIsNone(search.score(dict(base, valid=False)))
        # Fewer violations cannot outweigh worse actual slack.
        self.assertLess(search.score(dict(base, setup_violations=20, setup_wns_ns=-0.1)), search.score(base))
        qualified = dict(base, qualified=True, setup_wns_ns=0.01, area={"Logic": 21000})
        self.assertLess(search.score(qualified), search.score(base))
        self.assertLess(search.score(dict(qualified, area={"Logic": 20999})), search.score(qualified))

    def test_clock_tns_does_not_hide_cross_clock_negative_path(self):
        text = ("<Numbers of Setup Violated Endpoints>:1\n<Numbers of Hold Violated Endpoints>:0\n"
                "2.4 Total Negative Slack Summary\n  cpu_clk setup 0.000 0\n  cpu_clk hold 0.000 0\n"
                "3. Timing Details\n3.1.1 Setup Paths Table\n  1 -0.280 from/Q to/D controller_clk:[F] cpu_clk:[R] 4.630 0.003 4.837\n"
                "3.1.2 Hold Paths Table\n  1 0.100 from/Q to/D cpu_clk:[R] cpu_clk:[R] 0 0 0\n3.1.3 Recovery Paths Table\n")
        # Temporary files stay inside target, as do all live experiment outputs.
        target = Path(__file__).resolve().parents[3] / "target"
        target.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=target, prefix="instance-order-unit-") as directory:
            p = Path(directory) / "impl/pnr"
            p.mkdir(parents=True)
            (p / "cpu_v3_system.tr").write_text(text)
            result = search.metrics(Path(directory))
        self.assertEqual(result["setup_tns_ns"], 0)
        self.assertEqual(result["setup_reported_negative_slack_sum_ns"], -0.28)
        self.assertTrue(result["setup_negative_endpoints_complete"])

    def test_resource_report_preserves_fractional_dsp_usage(self):
        target = Path(__file__).resolve().parents[3] / "target"
        target.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=target, prefix="instance-order-resource-unit-") as directory:
            p = Path(directory) / "impl/pnr"
            p.mkdir(parents=True)
            for dsp in ("22.25", "22"):
                (p / "cpu_v3_system.rpt.txt").write_text(
                    "  Logic | 20069/20736\n  Register | 7440/15915\n"
                    "  BSRAM | 46/46\n  DSP | " + dsp + "/24\n"
                    "    --LUT,ALU,ROM16 | 18000(15000 LUT, 3000 ALU, 0 ROM16)\n")
                area = search.metrics(Path(directory))["area"]
                self.assertEqual(area["DSP"], float(dsp))
                self.assertEqual(area["Logic"], 20069)
                self.assertIsInstance(area["Logic"], int)
                self.assertEqual(area["BSRAM"], 46)

    def test_checkpoint_rng_and_no_repeated_neighbours(self):
        rng = random.Random(20261029)
        seen, results = set(), []
        for i in range(20):
            state = json.loads(json.dumps(rng.getstate()))
            restored = random.Random()
            restored.setstate(search.tuple_tree(state))
            order = search.propose(rng, results, seen, i // 2)
            self.assertEqual(order, search.propose(restored, results, seen, i // 2))
            self.assertNotIn(tuple(order), seen)
            seen.add(tuple(order))
            results.append({"order": order, "score": [1, 0.2, 0.4, 20000], "qualified": False,
                            "setup_wns_ns": -0.2, "hold_wns_ns": 0.1, "area": {"Logic": 20000}})
        self.assertNotEqual(search.cache_key("a", order), search.cache_key("b", order))

    def test_scheduler_migration_cannot_change_fixed_candidate_flow(self):
        source = 'NAMES=["a"]\ndef score(x):\n return x\ndef main():\n return 1\n'
        signature = search.pipeline_signature(source)
        self.assertEqual(signature, search.pipeline_signature(source.replace("return 1", "return 2")))
        self.assertNotEqual(signature, search.pipeline_signature(source.replace("return x", "return x+1")))
        self.assertNotEqual(signature, search.pipeline_signature(source.replace('["a"]', '["b"]')))

    def test_frontier_pool_keeps_low_area_near_feasible_and_high_margin(self):
        rows = []
        for i, (area, slack) in enumerate([(20000, 0.02), (19800, -0.02), (20200, 0.2), (19900, -0.01)]):
            order = search.NAMES[i:] + search.NAMES[:i]
            row = dict(order=order, valid=True, qualified=slack >= 0, tool_exit=0,
                       setup_violations=int(slack < 0), unrouted_nets=0, setup_wns_ns=slack,
                       hold_wns_ns=0.3, setup_reported_negative_slack_sum_ns=min(0, slack),
                       hold_reported_negative_slack_sum_ns=0, area={"Logic": area})
            row['score'] = search.score(row)
            rows.append(row)
        pool = search.portfolio(rows)
        self.assertEqual(pool[0], rows[0])
        self.assertTrue(all(row in pool for row in rows))


class FourWorkerSchedulerTests(unittest.TestCase):
    """Run the real scheduler with finite synthetic jobs and no external tools."""

    def run_scheduler(self, candidates, stagnation=8, qualified=False, batch_completion=False):
        target = Path(__file__).resolve().parents[3] / "target"
        target.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=target, prefix="instance-order-scheduler-unit-") as directory:
            root = Path(directory)
            base, output = root / "base", root / "round"
            top = base / search.TOP
            top.parent.mkdir(parents=True)
            original = template()
            top.write_bytes(original.encode())
            (base / "build.tcl").write_text("# unit-test-only; never executed\nrun all\n")
            normalized = dict(search.inputs(base))
            normalized[search.TOP] = search.digest(search.permute(original, search.NAMES).encode())
            connections = {"unit_test_only": True}
            gates = root / "gates.json"
            gates.write_text(json.dumps(dict(
                passed=True, source_commit="unit-test-only", original_inputs=search.inputs(base),
                normalized_inputs=normalized,
                connection_sha256=search.digest(json.dumps(connections, sort_keys=True).encode()),
            )))
            sdk = root / "fake-sdk"
            vendor, gowin = sdk / "IDE/simlib/gw2a/prim_sim.v", sdk / "IDE/bin/gw_sh.exe"
            iverilog = root / "never-execute-iverilog.exe"
            for path in [vendor, gowin, iverilog]:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("unit-test-only; never execute")
            barrier = threading.Barrier(4)
            lock = threading.Lock()
            active, peak = 0, 0
            starts, finishes, deadlines = [], [], []

            def physical_job(_base, job, order, _vendor, _expected, deadline, *_args):
                nonlocal active, peak
                reproduction = job.name == "reproduction"
                with lock:
                    if reproduction:
                        self.assertEqual(active, 0, "reproduction overlapped candidate jobs")
                        self.assertEqual(len(starts), len(finishes), "candidate futures not drained")
                    active += 1
                    peak = max(peak, active)
                    starts.append(job.name)
                    deadlines.append((reproduction, deadline))
                try:
                    if not reproduction:
                        # Four actual executor threads must enter before any job
                        # can finish. A two-slot or serial regression times out.
                        barrier.wait(timeout=5)
                    job.mkdir(parents=True, exist_ok=False)
                    result = dict(unit_test_only=True, order=list(order), directory=str(job),
                                  valid=True, qualified=qualified, tool_exit=0, elapsed_seconds=0,
                                  area={"Logic": 20000}, unrouted_nets=0,
                                  setup_violations=0 if qualified else 1, hold_violations=0,
                                  setup_wns_ns=0.1 if qualified else -0.1, hold_wns_ns=0.1,
                                  setup_tns_ns=0 if qualified else -0.1, hold_tns_ns=0,
                                  setup_reported_negative_slack_sum_ns=0 if qualified else -0.1,
                                  hold_reported_negative_slack_sum_ns=0,
                                  inputs={"unit_test_order": list(order)})
                    result["score"] = search.score(result)
                    search.save(job / "result.json", result)
                    return result
                finally:
                    with lock:
                        active -= 1
                        finishes.append(job.name)

            real_wait = search.concurrent.futures.wait

            def complete_batch(futures, **_kwargs):
                # A controlled simultaneous-completion history tests the stop
                # boundary without relying on OS thread completion ordering.
                return real_wait(futures, timeout=10,
                                 return_when=search.concurrent.futures.ALL_COMPLETED)

            # main is a CLI entry point. Retain its real lock handle through
            # error tracebacks, then emulate process-exit closure before the
            # temporary project is removed (including deliberate DUT failures).
            real_open = Path.open
            stage_locks = []

            def record_open(path, *args, **kwargs):
                stream = real_open(path, *args, **kwargs)
                if path == output / "stage.lock":
                    stage_locks.append(stream)
                return stream

            arguments = [str(search.__file__), "--base", str(base), "--output", str(output),
                         "--gates", str(gates), "--workers", "4", "--max-candidates", str(candidates),
                         "--minutes", "90", "--stagnation", str(stagnation), "--seed", "20261101"]
            with mock.patch.dict(os.environ, GOWIN_HOME=str(sdk), IVERILOG_EXE=str(iverilog)), \
                    mock.patch("sys.argv", arguments), mock.patch("sys.stdout", new=io.StringIO()), \
                    mock.patch.object(search, "elaborate", return_value=connections), \
                    mock.patch.object(search, "run_job", side_effect=physical_job), \
                    mock.patch.object(Path, "open", new=record_open), \
                    mock.patch.object(search.subprocess, "Popen", side_effect=AssertionError("external tool forbidden")) as popen, \
                    mock.patch.object(search.subprocess, "run", side_effect=AssertionError("external tool forbidden")) as run, \
                    mock.patch.object(search.concurrent.futures, "wait",
                                      side_effect=complete_batch if batch_completion else real_wait):
                try:
                    search.main()
                    self.assertTrue(stage_locks and all(stream.closed for stream in stage_locks))
                    popen.assert_not_called()
                    run.assert_not_called()
                finally:
                    for stream in stage_locks:
                        stream.close()
            state = json.loads((output / "state.json").read_text())
            self.assertEqual(active, 0)
            self.assertEqual(peak, 4)
            self.assertFalse(state["pending"])
            self.assertEqual(state["context"]["workers"], 4)
            self.assertEqual(state["context"]["max_candidates"], candidates)
            self.assertEqual(len({tuple(r["order"]) for r in state["results"]}), len(state["results"]))
            for reproduction, deadline in deadlines:
                self.assertEqual(deadline, state["deadline"] - (0 if reproduction else 1200))
            return state, starts, finishes

    def test_four_real_slots_obey_the_shared_candidate_budget(self):
        state, starts, finishes = self.run_scheduler(8)
        self.assertEqual(len(state["results"]), 8)
        self.assertEqual(len(starts), 8)
        self.assertEqual(len(finishes), 8)
        self.assertEqual(state["stop_reason"], "candidate_budget")
        self.assertNotIn("reproduction", state)

    def test_stagnation_drains_the_four_inflight_jobs_without_refill(self):
        state, starts, finishes = self.run_scheduler(16, stagnation=1, batch_completion=True)
        self.assertEqual(state["stop_reason"], "stagnation")
        self.assertEqual(len(state["results"]), 4)
        self.assertEqual(len(starts), 4)
        self.assertEqual(len(finishes), 4)

    def test_fresh_reproduction_is_serial_and_uses_the_same_reserved_deadline(self):
        state, starts, finishes = self.run_scheduler(4, qualified=True)
        self.assertEqual(len(state["results"]), 4)
        self.assertEqual(starts[-1], "reproduction")
        self.assertEqual(finishes[-1], "reproduction")
        self.assertEqual(len(starts), 5)
        self.assertTrue(state["reproduced"])


if __name__ == "__main__":
    unittest.main()
