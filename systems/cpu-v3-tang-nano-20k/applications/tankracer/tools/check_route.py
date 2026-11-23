"""Bounded route checks; --map-directory additionally checks real source ENGB."""
import argparse
import json
from pathlib import Path
import tempfile
import unittest

from export_route import export_route, main_loop, pack_route, read_json, validate_route, write_json_atomic

MAP_DIRECTORY = None


def sample():
    points = []
    for index, (name, edges) in enumerate((("wptstart", ["two"]), ("two", ["three", "side"]),
                                          ("three", ["wptstart"]), ("side", ["three"]))):
        points.append(dict(name=name, object_index=index + 2, position_q10=[index * 1025, -1, 9],
                           position=[index * 1025 / 1024, -1 / 1024, 9 / 1024], next=edges,
                           parameters="(:next " + " ".join(edges) + ")"))
    return dict(format="tankracer.route.v1", map="ENGLAND", scene="ENGB.DAT", start="wptstart",
                branch=0, source=dict(object_count=6), points=points,
                main_loop=["wptstart", "two", "three"])


class RouteChecks(unittest.TestCase):
    def test_closed_primary_route_preserves_branch_and_ids(self):
        route = validate_route(sample())
        self.assertEqual(main_loop(route["points"]), route["main_loop"])
        self.assertEqual(route["points"][1]["next"], ["three", "side"])
        packed = pack_route(route, [1, 2, 3], 0.125)
        self.assertEqual(packed["render_positions"][0], [-0.125, (-1 / 1024 - 2) / 8,
                                                       (9 / 1024 - 3) / 8])
        self.assertEqual(packed["route"]["points"][3]["object_index"], 5)

    def test_missing_node(self):
        route = sample()
        route["points"].pop()
        with self.assertRaisesRegex(ValueError, "missing node: side"):
            validate_route(route)

    def test_duplicate_edge(self):
        route = sample()
        route["points"][1].update(next=["three", "three"], parameters="(:next three three)")
        with self.assertRaisesRegex(ValueError, "duplicate edge"):
            validate_route(route)

    def test_wrong_start_and_branch(self):
        for patch in (dict(start="two"), dict(branch=1), dict(main_loop=["wptstart", "two", "side", "three"])):
            route = sample()
            route.update(patch)
            with self.assertRaises(ValueError):
                validate_route(route)

    def test_subcycle_and_coordinate_mismatch(self):
        route = sample()
        route["points"][2].update(next=["two"], parameters="(:next two)")
        with self.assertRaisesRegex(ValueError, "does not close"):
            validate_route(route)
        route = sample()
        route["points"][0]["position"][0] = 1
        with self.assertRaisesRegex(ValueError, "Q10"):
            validate_route(route)

    def test_atomic_json_round_trip(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "route.json"
            result = write_json_atomic(path, sample())
            self.assertEqual(result["bytes"], path.stat().st_size)
            self.assertEqual(read_json(path)[0], sample())
            path.write_bytes(b"{\0}")
            with self.assertRaisesRegex(ValueError, "NUL"):
                read_json(path)

    def test_real_england_source(self):
        if MAP_DIRECTORY is None:
            self.skipTest("supply --map-directory for real ENGLAND source check")
        route = export_route(MAP_DIRECTORY)
        source = json.loads((MAP_DIRECTORY / "scenes/ENGB.json").read_bytes())["data"]["objects"]
        self.assertEqual((len(source), len(route["points"]), len(route["main_loop"])), (240, 36, 33))
        for point in route["points"]:
            original = source[point["object_index"]]
            self.assertEqual(original["class"], "WayPoint")
            self.assertEqual(original["name"], point["name"])
            self.assertEqual(original["position"], point["position_q10"])
            self.assertEqual(original["parameters"], point["parameters"])
        self.assertEqual(route["main_loop"][0], "wptstart")
        self.assertEqual(route["main_loop"][-1], "wpt29")
        self.assertEqual(route["points"][0]["position_q10"], [-235, 122, 9072])
        self.assertEqual(route["source"]["resource_sha256"],
                         "4939e9cbfb9743feb7d9da79e768b5cf8c1c674f897495dbbf4da279e5fff012")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--map-directory", type=Path)
    args, remaining = parser.parse_known_args()
    MAP_DIRECTORY = args.map_directory
    unittest.main(argv=[__file__, *remaining])
