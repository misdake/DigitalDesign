"""Topology, source support and quantization checks for the tree silhouette."""
from collections import Counter
import unittest

from build_tree_silhouette import fit_silhouette, triangulate
from evaluate_tree_outline import area, cross, evaluate


class TreeSilhouetteChecks(unittest.TestCase):
    def test_shared_seams_and_winding(self):
        polygon = [[0, 0], [64, 0], [64, 16], [40, 16],
                   [40, 64], [24, 64], [24, 16], [0, 16]]
        for points in (polygon, list(reversed(polygon))):
            triangles = triangulate(points)
            self.assertEqual(len(triangles), 6)
            edges = Counter(tuple(sorted((a, b))) for t in triangles
                            for a, b in zip(t, t[1:] + t[:1]))
            self.assertEqual(sum(n == 1 for n in edges.values()), 8)
            self.assertEqual(sum(n == 2 for n in edges.values()), 5)
            self.assertTrue(all(n in (1, 2) for n in edges.values()))
            signs = [cross(*[points[i] for i in t]) for t in triangles]
            self.assertTrue(all(v > 0 for v in signs) or all(v < 0 for v in signs))
            self.assertEqual(sum(abs(v) for v in signs) / 2, area(points))

    def test_source_support_and_quantized_uv_mapping(self):
        points = [[-96, 0], [96, 192], [-96, 192], [96, 0]]
        uvs = [[0, 16320], [16320, 0], [0, 0], [16320, 16320]]
        alpha = [(x, y) for y in range(64)
                 for x in (range(8, 56) if y < 12 else range(26, 38))]
        fitted = fit_silhouette(points, uvs, alpha, 256, {'x': 0, 'y': 0})
        self.assertEqual(len(fitted['points']), 8)
        self.assertEqual(fitted['support_checks'], len(alpha) * 4)
        self.assertLess(area(fitted['points']), 0.6 * 192 * 192)
        for point, uv in zip(fitted['points'], fitted['attributes']):
            self.assertEqual(uv, [round(v) for v in evaluate(fitted['mapping'], point)])
        self.assertLessEqual(fitted['uv_error_source_texels'], 0.5 * 256 / 65536)

    def test_a_full_square_does_not_claim_a_trunk_fit(self):
        points = [[0, 0], [256, 0], [0, 256], [256, 256]]
        alpha = [(x, y) for y in range(64) for x in range(64)]
        with self.assertRaisesRegex(ValueError, 'cannot fit'):
            fit_silhouette(points, [[0, 0], [16384, 0], [0, 16384], [16384, 16384]],
                           alpha, 256, {'x': 0, 'y': 0})

    def test_rotated_uv_domain_requires_its_own_fitting_policy(self):
        with self.assertRaisesRegex(ValueError, 'axis-aligned'):
            fit_silhouette([[0, 0], [100, 0], [0, 100]],
                           [[1000, 1000], [2000, 1100], [1100, 2000]],
                           [(32, 32)], 256, {'x': 0, 'y': 0})


if __name__ == '__main__':
    unittest.main()
