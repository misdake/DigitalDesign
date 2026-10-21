"""Small deterministic checks for scene assets and the independent reference."""
from pathlib import Path
import unittest

import numpy as np

import float_reference as reference
import full_bunny
import material_scene
import verify_stage2


class MaterialSceneTests(unittest.TestCase):
    def test_opposite_faces_do_not_normalize_cancellation_residue(self):
        world = [(0, 0, 0), (1, 0, 0), (0, 1, 0), (1, 1, 0)]
        faces = [(0, 1, 2), (0, 2, 1), (1, 3, 2)]
        normals, repaired = full_bunny.smooth_normals(world, faces)
        self.assertEqual(repaired, [0])
        self.assertEqual(normals, [[0., 0., 1.]]*4)
        reversed_order, _ = full_bunny.smooth_normals(world, faces[::-1])
        self.assertEqual(normals, reversed_order)

    def test_supplied_bunny_retains_all_source_faces(self):
        ply = full_bunny.ROOT/'target/prepare-operands/fixtures/bun_zipper_res4.ply'
        if not ply.exists():
            self.skipTest('Supply the documented audited bunny PLY fixture')
        batches, info = full_bunny.mesh(ply)
        self.assertEqual(info['normal_fallback_vertices'], [140, 245, 330, 380, 382, 408])
        self.assertEqual(sorted(i for b in batches for i in b['source_face_ids']), list(range(948)))
        self.assertEqual(info['meshlets'], 17)

    def test_texture_seed_and_sampler_use_the_same_asset(self):
        levels = material_scene.texture_levels()
        memory = verify_stage2.initial_memory(levels)
        offset = 0
        for n in range(7):
            side = 1 << n
            for ty in range((side+7)//8):
                for tx in range((side+7)//8):
                    for y in range(8):
                        for x in range(8):
                            value = int.from_bytes(memory[offset:offset+2], 'little')
                            self.assertEqual(value, levels[n][((8*ty+y) % side)*side+(8*tx+x) % side])
                            offset += 2
        self.assertEqual(len(memory), 400384)
        self.assertNotEqual(verify_stage2.sample([10000, 30000], 0, levels),
                            verify_stage2.sample([10000, 30000], 0))

    def test_float_lighting_uses_public_view_ray_sign(self):
        # L=V=N implies full diffuse and full specular, including an off-axis ray.
        shading = dict(light=[0, 0, 16384], ray_scale=[-8192, -4915], ray_k=8192,
                       ambient=64, directional=192, shininess=12)
        g, h = reference.shade(np.array([[0., 0., 1.]]), np.array([199.5]), np.array([119.5]), shading)
        np.testing.assert_allclose([g[0], h[0]], [256, 192], atol=1e-10)
        x, y = 80., 35.
        v = reference.normalize(np.array([(2*x-399)/400*-.5, (239-2*y)/240*(-4915/16384), .5]))
        n = reference.normalize(v+np.array([0., 0., 1.]))
        _, h = reference.shade(n[None], np.array([x]), np.array([y]), shading)
        self.assertAlmostEqual(h[0], 192, places=9)


if __name__ == '__main__':
    unittest.main()
