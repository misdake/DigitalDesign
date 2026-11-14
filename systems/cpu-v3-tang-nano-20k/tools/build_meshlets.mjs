// Shared meshoptimizer entry, promoted from the local GPU meshpack experiment.
import {readFileSync} from 'node:fs';
import {pathToFileURL} from 'node:url';
import {MeshoptClusterizer} from './vendor/meshoptimizer/meshopt_clusterizer.mjs';

export async function buildMeshlets(positions, indices, maxVertices = 64, maxTriangles = 96) {
  await MeshoptClusterizer.ready;
  const buffers = MeshoptClusterizer.buildMeshlets(indices, positions, 3, maxVertices, maxTriangles, 0);
  return Array.from({length: buffers.meshletCount}, (_, index) => {
    const part = MeshoptClusterizer.extractMeshlet(buffers, index);
    return {vertices: Uint32Array.from(part.vertices), triangles: Uint8Array.from(part.triangles)};
  });
}

if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) {
  const input = JSON.parse(readFileSync(0, 'utf8'));
  const parts = await buildMeshlets(Float32Array.from(input.positions), Uint32Array.from(input.indices));
  process.stdout.write(JSON.stringify(parts.map(part => ({
    vertices: Array.from(part.vertices),
    triangles: Array.from({length: part.triangles.length / 3}, (_, i) => Array.from(part.triangles.slice(i * 3, i * 3 + 3))),
  }))));
}
