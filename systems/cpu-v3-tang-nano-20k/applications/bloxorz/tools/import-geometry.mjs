// Independent expected geometry: invoke the original renderer helpers and Three.js.
import { pathToFileURL, fileURLToPath } from 'node:url';
import { resolve, join } from 'node:path';
import { readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';

if (!process.argv[2]) throw Error('Usage: node tools/import-geometry.mjs <reference-directory>');
const root = resolve(process.argv[2]);
const THREE = await import(pathToFileURL(join(root, 'node_modules/three/build/three.module.js')));
const state = await import(pathToFileURL(join(root, 'src/core/state.js')));
const camera = await import(pathToFileURL(join(root, 'src/render/camera.js')));
const shadow = await import(pathToFileURL(join(root, 'src/render/ground-shadow.js')));
const rows = ['# Original Three.js geometry reference, schema 1.'];
for (const [w, h] of [[3, 3], [8, 6], [15, 10]]) {
  const c = new THREE.PerspectiveCamera(20, 5 / 3, 0.5, 260);
  const distance = camera.applyCamera(c, w, h);
  c.updateMatrixWorld(true);
  const v = c.matrixWorldInverse.elements;
  const k = c.projectionMatrix.elements[5] * 120 / 256;
  const view = [0, 1, 2].flatMap(r => [0, 4, 8, 12].map(col => v[col + r] * (r === 0 ? k : r === 1 ? -k : -1)));
  rows.push(`camera ${w} ${h} ${distance} ${view.join(' ')}`);
  for (const p of [[0, 0, 0], [-w / 2, 0, -h / 2], [w / 2, 2, h / 2]]) {
    const projected = new THREE.Vector3(...p).project(c);
    rows.push(`screen ${p.join(' ')} ${(projected.x + 1) * 200 - 0.5} ${(1 - projected.y) * 120 - 0.5}`);
  }
}
for (let orient = 0; orient < 3; orient++) for (let dir = 0; dir < 4; dir++) {
  for (const fraction of [0, 0.125, 0.5, 0.875, 1]) for (const drop of [0, -0.7, -3]) {
    const hint = state.rollAnimation(orient, 0, 0, dir);
    const center = state.blockCenter(orient, 0, 0);
    const size = state.blockSize(orient);
    const pivot = new THREE.Group();
    pivot.position.set(hint.px, drop, hint.pz);
    pivot.rotation[hint.axis] = hint.angle * fraction;
    const mesh = new THREE.Object3D();
    mesh.position.set(center.x - hint.px, center.y, center.z - hint.pz);
    mesh.scale.set(size.x, size.y, size.z);
    pivot.add(mesh);
    const poly = shadow.boxFloorShadow(mesh, 0.49);
    const e = mesh.matrixWorld.elements;
    const matrix = [0, 1, 2].flatMap(r => [e[r], e[r + 4], e[r + 8], e[r + 12]]);
    rows.push(`pose ${orient} ${dir} ${fraction} ${drop} ${matrix.join(' ')} ${poly.length} ${poly.flat().join(' ')}`.trim());
    const clipped = shadow.clipPolyToCell(poly, 0, 0);
    let area = 0;
    for (let i = 0; i < clipped.length; i++) {
      const a = clipped[i], b = clipped[(i + 1) % clipped.length];
      area += a[0] * b[1] - a[1] * b[0];
    }
    rows.push(`cell ${Math.abs(area) / 2}`);
  }
}
const out = fileURLToPath(new URL('../assets/', import.meta.url));
await writeFile(join(out, 'geometry-reference.txt'), rows.join('\n') + '\n');
const files = {};
for (const name of ['src/core/state.js', 'src/render/camera.js', 'src/render/ground-shadow.js', 'node_modules/three/package.json']) {
  files[name] = createHash('sha256').update(await readFile(join(root, name))).digest('hex');
}
await writeFile(join(out, 'geometry-provenance.json'), JSON.stringify({ schema: 1, poses: 180, cameras: 3, files }, null, 2) + '\n');
console.log('Imported 180 poses/shadows and three cameras from original helpers.');
