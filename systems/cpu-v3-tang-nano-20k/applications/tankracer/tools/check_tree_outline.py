"""Small geometry invariants for the native-affine outline evaluator."""
import unittest
import contextlib
import io
import json
import os
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch
import tempfile
import zipfile
import evaluate_tree_outline
from evaluate_tree_outline import affine, area, evaluate, intersect, ordered, packed_point, simplify


class TreeOutlineChecks(unittest.TestCase):
    def test_native_geometry_uses_ties_even(self):
        scene = {'scale': 0.125, 'origin': [0, 0, 0], 'position_shift': 10}
        self.assertEqual(packed_point([0, 0.0625, 0, 0.0078125, 0, 0], scene), [0, 0])
        self.assertEqual(packed_point([0, 0.1875, 0, 0.0234375, 0, 0], scene), [2, 2])

    def test_quantized_geometry_drives_uv(self):
        points = [[-96, 39], [96, 87], [-96, 87]]
        uv = [[41471, 37311], [57791, 20991], [41471, 20991]]
        mapping = affine(points, uv)
        for point, expected in zip(points, uv):
            self.assertEqual(evaluate(mapping, point), expected)
        for point in [[-73, 42], [0, 63], [66, 84]]:
            expected = evaluate(mapping, point)
            rounded = [round(v) for v in expected]
            self.assertTrue(all(abs(a-b) <= 0.5 for a, b in zip(expected, rounded)))

    def test_remove_duplicate_and_collinear_vertices(self):
        self.assertEqual(simplify([[0, 0], [0, 0], [1, 0], [2, 0], [2, 2], [0, 2], [0, 0]]),
                         [[0, 0], [2, 0], [2, 2], [0, 2]])

    def test_support_is_intersected_with_original_native_domain(self):
        original = ordered([[0, 0], [255/256, 0], [255/256, 255/256], [0, 255/256]])
        clipped = intersect([[0.8, 0.8], [1.1, 0.8], [1.1, 1.1], [0.8, 1.1]], original)
        self.assertTrue(clipped)
        self.assertTrue(all(-1e-12 <= x <= 255/256 + 1e-12 and -1e-12 <= y <= 255/256 + 1e-12 for x, y in clipped))
        self.assertGreater(area(clipped), 0)
        self.assertEqual(intersect([[2, 2], [3, 2], [3, 3], [2, 3]], original), [])

    @unittest.skipUnless(os.environ.get('TANKRACER_TREE_OUTLINE_FIXTURE'),
                         'Set TANKRACER_TREE_OUTLINE_FIXTURE for the real-asset fallback check')
    def test_collapsed_outline_retains_original_quad_through_native_validation(self):
        # The qualified map has no natural collapsed card at shift 8. Inject one
        # two-point quantized result, then exercise the complete real-asset path.
        fixture = json.loads(Path(os.environ['TANKRACER_TREE_OUTLINE_FIXTURE']).read_bytes())
        original_simplify = evaluate_tree_outline.simplify
        calls = 0

        def collapse_first(points):
            nonlocal calls
            calls += 1
            return points[:2] if calls == 1 else original_simplify(points)

        with tempfile.TemporaryDirectory(dir=fixture['output_root']) as folder:
            output = Path(folder) / 'candidate'
            args = SimpleNamespace(source_root=Path(fixture['source_root']),
                                   decoded=Path(fixture['decoded']), input=Path(fixture['input']),
                                   output=output, precision=8, exact_anchor=True)
            with patch.object(evaluate_tree_outline, 'simplify', side_effect=collapse_first):
                with contextlib.redirect_stdout(io.StringIO()):
                    evaluate_tree_outline.run(args)
            report = json.loads((output / 'result.json').read_bytes())
            self.assertEqual(report['fallback_quads'], 1)
            self.assertEqual(report['triangles_after'], 2 + 99 * 5)
            self.assertEqual(report['native_record_bytes_before'], report['native_record_bytes_after'])
            self.assertGreater(report['alpha_support_corner_checks'], 0)
            retained = report['rows'][0]['source_group_ordinal']
            with zipfile.ZipFile(args.input) as archive:
                original = json.loads(archive.read('scene.json'))
            with zipfile.ZipFile(output / 'scene.zip') as archive:
                candidate = json.loads(archive.read('scene.json'))
            self.assertNotIn('bounds_geometry', original['groups'][retained])
            self.assertEqual(candidate['groups'][retained], original['groups'][retained])
            self.assertEqual(sum(a != b for a, b in zip(original['groups'], candidate['groups'])), 99)

    def test_degenerate_original_mapping_is_rejected(self):
        with self.assertRaisesRegex(AssertionError, 'nondegenerate'):
            affine([[0, 0], [1, 0], [2, 0]], [[0, 0], [1, 1], [2, 2]])


if __name__ == '__main__':
    unittest.main()
