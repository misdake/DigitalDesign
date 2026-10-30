"""Fast boundary/fitness/checkpoint tests; no synthesis or simulator subprocesses."""
import json
from pathlib import Path
import random
import tempfile
import unittest

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


if __name__ == "__main__":
    unittest.main()
