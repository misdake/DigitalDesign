// Rebuild the original static scene, then produce an opt-in co-located alpha split.
import assert from 'node:assert/strict';
import {readFile, mkdir, open, rename, stat} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
import {alphaPagesLayout} from './alpha_pages_layout.mjs';

const args = new Map();
for (let i = 2; i < process.argv.length; i += 2) {
  assert.ok(process.argv[i].startsWith('--') && process.argv[i + 1], 'Expected named arguments');
  args.set(process.argv[i].slice(2), path.resolve(process.argv[i + 1]));
}
for (const key of ['source-root', 'decoded', 'archive', 'baseline', 'layout', 'output'])
  assert.ok(args.has(key), '--' + key);
const source = args.get('source-root'),
  output = args.get('output');
const {decode} = await import(pathToFileURL(path.join(source, 'tools/map_format.mjs')));
const {alphaAtlas, nativeTexture} = await import(
  pathToFileURL(path.join(source, 'tools/platform_texture.mjs'))
);
const {staticScene} = await import(pathToFileURL(path.join(source, 'tools/static_scene.mjs')));
const {terrainLods} = await import(pathToFileURL(path.join(source, 'tools/terrain_lod.mjs')));
const {zipArchive} = await import(pathToFileURL(path.join(source, 'tools/zip_archive.mjs')));
const hash = (bytes) => createHash('sha256').update(bytes).digest('hex');
const jsonBytes = (value, indent) => Buffer.from(JSON.stringify(value, null, indent) + '\n');
async function atomic(file, bytes) {
  await mkdir(path.dirname(file), {recursive: true});
  const temporary = file + '.tmp-' + process.pid;
  const stream = await open(temporary, 'wx');
  try {
    await stream.writeFile(bytes);
    await stream.sync();
  } finally {
    await stream.close();
  }
  await rename(temporary, file);
  const actual = await readFile(file);
  assert.equal(actual.length, bytes.length);
  assert.equal(hash(actual), hash(bytes));
}
const extracted = spawnSync(
  'python',
  [
    '-B',
    '-c',
    'import json,sys,zipfile,hashlib; z=zipfile.ZipFile(sys.argv[1]); print(json.dumps(dict(scene=json.loads(z.read("scene.json")), inventory=json.loads(z.read("inventory.json")), hashes={n:hashlib.sha256(z.read(n)).hexdigest() for n in z.namelist() if n.endswith(".bin")})))',
    args.get('baseline')
  ],
  {encoding: 'utf8', maxBuffer: 32 * 1024 * 1024, windowsHide: true}
);
assert.equal(extracted.status, 0, extracted.stderr);
const original = JSON.parse(extracted.stdout);
const manifest = JSON.parse(
  await readFile(path.join(source, 'assets/maps/ENGLAND/map.json'), 'utf8')
);
const worldBytes = await readFile(path.join(source, 'assets/maps/ENGLAND/world.json'));
const world = JSON.parse(worldBytes).data;
const archive = await readFile(args.get('archive'));
assert.equal(hash(archive), manifest.source.sha256, 'Original FASTFILE hash');
assert.equal(hash(worldBytes), original.inventory.source.world_sha256, 'Original world hash');
const surfaces = new Map();
for (let i = 0; i < archive.readUInt32LE(24); i++) {
  const at = 28 + i * 24,
    name = archive
      .subarray(at, at + 12)
      .toString('ascii')
      .split('\0')[0];
  surfaces.set(
    name,
    hash(
      archive.subarray(
        archive.readUInt32LE(at + 12),
        archive.readUInt32LE(at + 12) + archive.readUInt32LE(at + 16)
      )
    )
  );
}
const images = new Map(),
  decodedHashes = {};
for (const material of world.materials) {
  const name = material.name.toUpperCase();
  const bytes = await readFile(path.join(args.get('decoded'), name + '.trrgba'));
  images.set(name, decode('image', bytes).mips[0]);
  decodedHashes[name] = hash(bytes);
  assert.equal(
    surfaces.get(name + '.SUR'),
    manifest.resources.find((r) => r.id === name + '.SUR').source_sha256
  );
}
const layoutBytes = await readFile(args.get('layout'));
const layoutConfig = JSON.parse(layoutBytes);
const layout = alphaPagesLayout(layoutConfig, {
  sceneSha256: hash(await readFile(args.get('baseline'))),
  archiveSha256: hash(archive),
  images
});
const alphaNames = [...images]
  .filter(([, image]) => (image.flags & 3) !== 0)
  .map(([name]) => name)
  .sort();

function build(split) {
  const files = [],
    textures = new Map(),
    provenance = [],
    pages = new Map();
  const alpha = new Map([...images].filter(([, i]) => (i.flags & 3) !== 0));
  if (!split) {
    const packed = alphaAtlas(alpha),
      native = nativeTexture(packed.image, false);
    files.push({path: 'textures/ALPHA.bin', bytes: native.bytes});
    for (const name of alpha.keys())
      pages.set(name, {...native, file: 'textures/ALPHA.bin', region: packed.regions.get(name)});
  } else {
    layout.pages.forEach((names, index) => {
      const packed = alphaAtlas(new Map(names.map((n) => [n, images.get(n)])));
      assert.equal(packed.image.width, 256);
      const native = nativeTexture(packed.image, false),
        file = 'textures/ALPHA_' + index + '.bin';
      files.push({path: file, bytes: native.bytes});
      for (const name of names)
        pages.set(name, {...native, file, region: packed.regions.get(name)});
    });
    const native = nativeTexture(images.get('IGATE'), false),
      file = 'textures/IGATE_ALPHA.bin';
    assert.equal(native.size, 128);
    assert.equal(native.bytes.length, 32768);
    files.push({path: file, bytes: native.bytes});
    pages.set('IGATE', {...native, file});
  }
  for (const material of world.materials) {
    const name = material.name.toUpperCase(),
      image = images.get(name);
    const native = pages.get(name) ?? {
      ...nativeTexture(image, (image.flags & 3) === 0),
      file: 'textures/' + name + '.bin'
    };
    const alphaMode = ['opaque', 'cutout', 'blend'][image.flags & 3];
    textures.set(name, {...native, image, alpha: alphaMode});
    if (!pages.has(name)) files.push({path: native.file, bytes: native.bytes});
    const resource = manifest.resources.find((r) => r.id === name + '.SUR');
    provenance.push({
      name,
      source_sha256: resource.source_sha256,
      file: native.file,
      bytes: native.bytes.length,
      sha256: hash(native.bytes),
      size: native.size,
      format: native.format,
      mipmaps: native.mipmaps,
      region: native.region,
      levels: native.levels
    });
  }
  const {scene, inventory} = staticScene(world, textures);
  scene.display_distance = original.scene.display_distance;
  inventory.terrain_lod = terrainLods(scene);
  const metadata = {
    format: 'tankracer.static-input.v1',
    map: 'ENGLAND',
    cup: 0,
    coordinates: {
      basis: 'source Y-up, Z unreflected',
      bsm_units_per_world: 128,
      simulation_units_per_world: 1024,
      render_scale: scene.scale
    },
    display_distance_world: scene.display_distance / scene.scale,
    source: {archive: manifest.source, world_sha256: hash(worldBytes)},
    content: ['terrain', 'roads', 'buildings', 'fixed props', 'fixed billboard anchors'],
    materials: scene.materials.length,
    groups: scene.groups.length,
    texture_bytes: files.reduce((n, f) => n + (f.path.endsWith('.bin') ? f.bytes.length : 0), 0),
    textures: provenance,
    ...inventory
  };
  return {scene, metadata, files, textures};
}
function packageOf(built) {
  return zipArchive([
    {path: 'scene.json', bytes: jsonBytes(built.scene)},
    ...built.files,
    {path: 'inventory.json', bytes: jsonBytes(built.metadata, 2)}
  ]);
}
const baseline = build(false);
assert.deepEqual(baseline.scene, original.scene, 'Pure source rebuild reproduces production scene');
assert.equal(
  hash(jsonBytes(baseline.metadata, 2)),
  hash(jsonBytes(original.inventory, 2)),
  'Pure source rebuild reproduces production inventory'
);
for (const file of baseline.files)
  assert.equal(hash(file.bytes), original.hashes[file.path], file.path);
assert.equal(baseline.files.length, Object.keys(original.hashes).length);
const baselineZip = packageOf(baseline),
  originalZip = await readFile(args.get('baseline'));
assert.equal(hash(baselineZip), hash(originalZip), 'Complete baseline ZIP rebuild');

function regionAt(scene, group, vertex, textures) {
  const uv = vertex.slice(group.billboard ? 4 : 3),
    material = scene.materials[group.material];
  const matches = [...textures].filter(
    ([, t]) =>
      t.file === material.texture &&
      (!t.region ||
        (uv[0] * t.size >= t.region.x - 1e-9 &&
          uv[0] * t.size <= t.region.x + t.region.width + 1e-9 &&
          uv[1] * t.size >= t.region.y - 1e-9 &&
          uv[1] * t.size <= t.region.y + t.region.height + 1e-9))
  );
  assert.equal(matches.length, 1, 'Unambiguous original source image');
  return matches[0];
}
function roundEven(x) {
  const n = Math.floor(x),
    f = x - n;
  return f < 0.5 ? n : f > 0.5 ? n + 1 : n + (n & 1);
}
function sourceTexel(uv, texture, quantized) {
  return uv.map(
    (v, a) =>
      (quantized ? roundEven(v * 65535) / 65536 : v) * texture.size -
      (texture.region ? (a ? texture.region.y : texture.region.x) : 0)
  );
}
function convert() {
  const candidate = build(true),
    mapping = [],
    newNames = new Map(),
    materialPairs = new Map();
  assert.equal(
    candidate.scene.groups.length,
    baseline.scene.groups.length,
    'No sort-group split or merge'
  );
  const representativeAlphaChanges = [];
  let uvVertices = 0,
    maxQuantizedTexelDelta = 0;
  for (let index = 0; index < baseline.scene.groups.length; index++) {
    const a = baseline.scene.groups[index],
      b = candidate.scene.groups[index];
    assert.deepEqual(b.triangles, a.triangles, 'Triangle sequence and indices');
    assert.equal(b.vertices.length, a.vertices.length);
    assert.equal(b.billboard, a.billboard);
    assert.equal(b.source_texture, a.source_texture);
    assert.deepEqual(b.lod, a.lod, 'Opaque LOD unchanged');
    const ma = baseline.scene.materials[a.material],
      mb = candidate.scene.materials[b.material];
    for (const key of ['tint', 'rgba', 'mipmaps', 'clamp', 'blend', 'encoding'])
      assert.deepEqual(mb[key], ma[key], 'Preserved material state ' + key);
    const names = new Set();
    for (let v = 0; v < a.vertices.length; v++) {
      const width = a.billboard ? 4 : 3;
      assert.deepEqual(
        b.vertices[v].slice(0, width),
        a.vertices[v].slice(0, width),
        'Position and BB anchor/offset'
      );
      if (!ma.texture) {
        assert.deepEqual(b.vertices[v], a.vertices[v]);
        continue;
      }
      const [name, oldTexture] = regionAt(baseline.scene, a, a.vertices[v], baseline.textures);
      const nextTexture = candidate.textures.get(name);
      assert.equal(nextTexture.alpha, oldTexture.alpha, 'Original SUR alpha mode');
      names.add(name);
      assert.equal(
        mb.texture,
        nextTexture.file,
        'All original images in this group share one page'
      );
      const oldUv = a.vertices[v].slice(width),
        nextUv = b.vertices[v].slice(width);
      const oldSource = sourceTexel(oldUv, oldTexture, false),
        nextSource = sourceTexel(nextUv, nextTexture, false);
      oldSource.forEach((x, i) =>
        assert.ok(Math.abs(x - nextSource[i]) < 1e-9, 'Original source UV semantics')
      );
      const oldNative = sourceTexel(oldUv, oldTexture, true),
        nextNative = sourceTexel(nextUv, nextTexture, true);
      oldNative.forEach(
        (x, i) =>
          (maxQuantizedTexelDelta = Math.max(maxQuantizedTexelDelta, Math.abs(x - nextNative[i])))
      );
      uvVertices++;
    }
    // staticScene keys materials by [texture file, tint]. source_alpha names
    // the first SUR encountered for that merged material, not a runtime state.
    if (ma.source_alpha !== mb.source_alpha) {
      representativeAlphaChanges.push({
        source_group_ordinal: index,
        name: a.name,
        baseline_material: a.material,
        candidate_material: b.material,
        before: ma.source_alpha,
        after: mb.source_alpha,
        source_images: [...names].sort()
      });
    }
    const generated = b.name;
    newNames.set(generated, a.name);
    b.name = a.name;
    b.source_group_ordinal = index;
    mapping.push({
      source_group_ordinal: index,
      name: a.name,
      generated_name: generated,
      baseline_material: a.material,
      candidate_material: b.material,
      source_images: [...names].sort(),
      source_alpha_modes: Object.fromEntries(
        [...names].sort().map((name) => [name, baseline.textures.get(name).alpha])
      ),
      representative_source_alpha_before: ma.source_alpha,
      representative_source_alpha_after: mb.source_alpha,
      texture: mb.texture ?? null,
      billboard: !!b.billboard
    });
    materialPairs.set(a.material + ':' + b.material, {baseline: a.material, candidate: b.material});
  }
  for (const mesh of candidate.metadata.meshes)
    mesh.group_names = mesh.group_names.map((n) => newNames.get(n));
  assert.deepEqual(
    candidate.metadata.meshes,
    baseline.metadata.meshes,
    'Original mesh-to-sort-group sequence'
  );
  candidate.metadata.alpha_split = {
    format: 'tankracer.alpha-split.v1',
    opt_in: true,
    source_group_ordinals: true,
    pages: layout.pages,
    standalone: 'IGATE',
    original_atlas_bytes: 524288,
    alpha_bytes: 294912
  };
  const bytes = packageOf(candidate);
  return {
    ...candidate,
    bytes,
    mapping,
    materialPairs: [...materialPairs.values()],
    representativeAlphaChanges,
    uvVertices,
    maxQuantizedTexelDelta
  };
}
const candidate = convert(),
  repeat = convert();
assert.equal(
  hash(candidate.bytes),
  hash(repeat.bytes),
  'Independent repeated construction is deterministic'
);
function word(bytes, side, x, y) {
  const tiles = side / 16;
  const at =
    ((Math.floor(x / 16) * tiles + Math.floor(y / 16)) * 256 + (y % 16) * 16 + (x % 16)) * 2;
  return bytes.readUInt16LE(at);
}
let pixelChecks = 0,
  guardChecks = 0;
for (const name of alphaNames) {
  const old = baseline.textures.get(name),
    next = candidate.textures.get(name),
    image = images.get(name);
  const forward = nativeTexture(image, false);
  for (let y = -2; y < image.height + 2; y++)
    for (let x = -2; x < image.width + 2; x++) {
      const sx = Math.max(0, Math.min(image.width - 1, x)),
        sy = Math.max(0, Math.min(image.height - 1, y));
      const expected = word(forward.bytes, forward.size, sx, sy);
      const oldWord = word(old.bytes, old.size, old.region.x + x, old.region.y + y);
      const nextWord = next.region
        ? word(next.bytes, next.size, next.region.x + x, next.region.y + y)
        : word(next.bytes, next.size, sx, sy);
      assert.equal(oldWord, expected, 'Original quantized pixel and gutter ' + name);
      assert.equal(nextWord, expected, 'Candidate quantized pixel and clamp/gutter ' + name);
      pixelChecks++;
      if (x < 0 || y < 0 || x >= image.width || y >= image.height) guardChecks++;
    }
}
assert.ok(candidate.maxQuantizedTexelDelta <= 1 / 128, 'Documented u16 UV coordinate bound');
await mkdir(path.dirname(output), {recursive: true});
await mkdir(output);
async function emit(folder, built, zip) {
  for (const file of built.files) await atomic(path.join(folder, file.path), file.bytes);
  await atomic(path.join(folder, 'scene.json'), jsonBytes(built.scene));
  await atomic(path.join(folder, 'inventory.json'), jsonBytes(built.metadata, 2));
  await atomic(path.join(folder, 'scene.zip'), zip);
}
await emit(path.join(output, 'baseline'), baseline, baselineZip);
await emit(path.join(output, 'candidate'), candidate, candidate.bytes);
await atomic(
  path.join(output, 'group-material-map.json'),
  jsonBytes({groups: candidate.mapping, materials: candidate.materialPairs}, 2)
);
const helperHashes = {};
for (const file of [
  'map_format.mjs',
  'platform_texture.mjs',
  'static_scene.mjs',
  'terrain_lod.mjs',
  'zip_archive.mjs'
])
  helperHashes[file] = hash(await readFile(path.join(source, 'tools', file)));
const report = {
  format: 'tankracer.alpha-split-check.v1',
  archive_sha256: hash(archive),
  layout_format: layoutConfig.format,
  layout_sha256: hash(layoutBytes),
  source_world_sha256: hash(worldBytes),
  source_helpers: helperHashes,
  decoded_surfaces: decodedHashes,
  baseline_zip_sha256: hash(baselineZip),
  candidate_zip_sha256: hash(candidate.bytes),
  baseline_zip_identical: true,
  baseline_scene_and_inventory_identical: true,
  all_original_native_textures_identical: true,
  groups: baseline.scene.groups.length,
  materials_before: baseline.scene.materials.length,
  materials_after: candidate.scene.materials.length,
  alpha_texture_bytes_before: 524288,
  alpha_texture_bytes_after: 294912,
  alpha_texture_bytes_saved: 229376,
  texture_bytes_before: baseline.metadata.texture_bytes,
  texture_bytes_after: candidate.metadata.texture_bytes,
  positions_indices_source_order_lod_states_preserved: true,
  per_source_alpha_modes_preserved: true,
  runtime_material_state_fields: ['tint', 'rgba', 'mipmaps', 'clamp', 'blend', 'encoding'],
  source_alpha_metadata_scope:
    'staticScene materials merge by [texture file,tint]; source_alpha is the first encountered SUR label. Actual per-SUR modes and runtime state are checked independently.',
  representative_source_alpha_label_changes: candidate.representativeAlphaChanges,
  sort_groups_split: 0,
  group_names_preserved: true,
  original_source_uv_semantics_preserved: true,
  uv_vertices_checked: candidate.uvVertices,
  maximum_native_u16_source_texel_delta: candidate.maxQuantizedTexelDelta,
  native_pixel_and_virtual_guard_checks: pixelChecks,
  guard_checks: guardChecks,
  igate_clamp_virtual_guard_texels: 2,
  deterministic_candidate_hash: true,
  geometry_triangles_unchanged: true,
  gpu_and_cpu_runtime_not_measured: true,
  quantization_scope:
    'Native u16 UV quantization can change sampling slightly. Do not assume exact RGB equality; geometry/depth/order/state must remain strict.'
};
await atomic(path.join(output, 'result.json'), jsonBytes(report, 2));
console.log(JSON.stringify(report));
