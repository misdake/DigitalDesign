// Inspect original SUR alpha support and existing atlas packing; no geometry/UV edits.
import assert from 'node:assert/strict';
import {readFile, writeFile, mkdir, rename, open} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
import path from 'node:path';
import {pathToFileURL} from 'node:url';

const options = new Map();
for (let i = 2; i < process.argv.length; i += 2) {
  assert.ok(
    process.argv[i].startsWith('--') && process.argv[i + 1],
    'Named arguments require values'
  );
  options.set(process.argv[i].slice(2), path.resolve(process.argv[i + 1]));
}
for (const name of ['source-root', 'decoded', 'scene', 'inventory', 'archive', 'output'])
  assert.ok(options.has(name), '--' + name);
const source = options.get('source-root'),
  output = options.get('output');
const {decode} = await import(pathToFileURL(path.join(source, 'tools/map_format.mjs')));
const {alphaAtlas, nativeTexture} = await import(
  pathToFileURL(path.join(source, 'tools/platform_texture.mjs'))
);
const hash = (bytes) => createHash('sha256').update(bytes).digest('hex');
const archiveBytes = await readFile(options.get('archive'));
const inventoryBytes = await readFile(options.get('inventory'));
const inventory = JSON.parse(inventoryBytes);
assert.equal(hash(archiveBytes), inventory.source.archive.sha256, 'Original FASTFILE identity');
assert.equal(archiveBytes.subarray(0, 8).toString(), 'FASTF101');
const resourceHashes = new Map();
for (let i = 0; i < archiveBytes.readUInt32LE(24); i++) {
  const at = 28 + i * 24;
  const name = archiveBytes
    .subarray(at, at + 12)
    .toString('ascii')
    .split('\0')[0];
  const offset = archiveBytes.readUInt32LE(at + 12),
    bytes = archiveBytes.readUInt32LE(at + 16);
  resourceHashes.set(name, hash(archiveBytes.subarray(offset, offset + bytes)));
}
const unpack = spawnSync(
  'python',
  [
    '-B',
    '-c',
    'import json,sys,zipfile,hashlib; z=zipfile.ZipFile(sys.argv[1]); print(json.dumps(dict(scene=json.loads(z.read("scene.json")), hashes={n:hashlib.sha256(z.read(n)).hexdigest() for n in z.namelist() if n.endswith(".bin")})))',
    options.get('scene')
  ],
  {encoding: 'utf8', maxBuffer: 32 * 1024 * 1024, windowsHide: true}
);
assert.equal(unpack.status, 0, unpack.stderr);
const input = JSON.parse(unpack.stdout),
  scene = input.scene;
const alpha = inventory.textures.filter((t) => t.region);
const images = new Map(),
  rows = [],
  uses = new Map();
for (const texture of alpha) {
  const bytes = await readFile(path.join(options.get('decoded'), texture.name + '.trrgba'));
  const image = decode('image', bytes).mips[0];
  assert.ok((image.flags & 3) !== 0, 'Raw alpha flags');
  assert.equal(
    texture.source_sha256,
    resourceHashes.get(texture.name + '.SUR'),
    'Original SUR hash'
  );
  images.set(texture.name, image);
  let x0 = image.width,
    y0 = image.height,
    x1 = 0,
    y1 = 0,
    nonzero = 0,
    partial = 0;
  for (let y = 0; y < image.height; y++)
    for (let x = 0; x < image.width; x++) {
      const a = image.rgba[(y * image.width + x) * 4 + 3];
      if (a === 0) continue;
      nonzero++;
      if (a !== 255) partial++;
      x0 = Math.min(x0, x);
      y0 = Math.min(y0, y);
      x1 = Math.max(x1, x + 1);
      y1 = Math.max(y1, y + 1);
    }
  const crop = nonzero
    ? [
        Math.max(0, x0 - 2),
        Math.max(0, y0 - 2),
        Math.min(image.width, x1 + 2),
        Math.min(image.height, y1 + 2)
      ]
    : [0, 0, image.width, image.height];
  rows.push({
    name: texture.name,
    width: image.width,
    height: image.height,
    flags: image.flags,
    raw_surface_sha256: texture.source_sha256,
    decoded_sha256: hash(bytes),
    nonzero_alpha: nonzero,
    partial_alpha: partial,
    alpha_bbox: nonzero ? [x0, y0, x1, y1] : null,
    crop_with_two_texel_guard: crop,
    crop_width: crop[2] - crop[0],
    crop_height: crop[3] - crop[1],
    fully_transparent: !nonzero,
    fully_transparent_action: !nonzero ? 'retain original; never emit count zero' : null
  });
  uses.set(texture.name, new Map());
}
const baseline = alphaAtlas(images),
  side = baseline.image.width;
for (const texture of alpha) assert.deepEqual(baseline.regions.get(texture.name), texture.region);
const native = nativeTexture(baseline.image, false);
assert.equal(
  hash(native.bytes),
  input.hashes[alpha[0].file],
  'Forward raw SUR -> atlas -> RGBA4444 matches production'
);
let unattributed = 0;
for (const group of scene.groups) {
  if (scene.materials[group.material].texture !== alpha[0].file) continue;
  for (const triangle of group.triangles) {
    const uv = triangle.map((i) =>
      group.vertices[i].slice(group.billboard ? 4 : 3).map((v) => v * side)
    );
    const matches = alpha.filter((t) =>
      uv.every(
        (p) =>
          p[0] >= t.region.x - 1e-6 &&
          p[0] <= t.region.x + t.region.width + 1e-6 &&
          p[1] >= t.region.y - 1e-6 &&
          p[1] <= t.region.y + t.region.height + 1e-6
      )
    );
    if (matches.length !== 1) {
      unattributed++;
      continue;
    }
    const texture = matches[0],
      record = rows.find((r) => r.name === texture.name);
    if (group.billboard) assert.equal(group.source_texture, texture.name);
    const local = uv.map((p) => [p[0] - texture.region.x, p[1] - texture.region.y]);
    let usage = uses.get(texture.name).get(group.name);
    if (!usage) {
      usage = {
        billboard: !!group.billboard,
        vertices: group.vertices.length,
        triangles: group.triangles.length,
        lo: [Infinity, Infinity],
        hi: [-Infinity, -Infinity],
        area: 0,
        outside_crop: false
      };
      uses.get(texture.name).set(group.name, usage);
    }
    for (const p of local)
      for (let a = 0; a < 2; a++) {
        usage.lo[a] = Math.min(usage.lo[a], p[a]);
        usage.hi[a] = Math.max(usage.hi[a], p[a]);
        if (
          p[a] < record.crop_with_two_texel_guard[a] - 1e-6 ||
          p[a] > record.crop_with_two_texel_guard[a + 2] + 1e-6
        )
          usage.outside_crop = true;
      }
    usage.area +=
      Math.abs(
        (local[1][0] - local[0][0]) * (local[2][1] - local[0][1]) -
          (local[1][1] - local[0][1]) * (local[2][0] - local[0][0])
      ) / 2;
  }
}
assert.equal(unattributed, 0, 'Every alpha face identifies its original region');
for (const row of rows) {
  const groups = [...uses.get(row.name).entries()];
  const partial = groups.filter(
    ([, g]) =>
      g.lo[0] > 0.5 ||
      g.lo[1] > 0.5 ||
      g.hi[0] < row.width - 0.5 ||
      g.hi[1] < row.height - 0.5 ||
      g.area < row.width * row.height * 0.95
  );
  row.referenced_source_uv_bounds = {
    lo: [0, 1].map((a) => Math.min(...groups.map(([, g]) => g.lo[a]))),
    hi: [0, 1].map((a) => Math.max(...groups.map(([, g]) => g.hi[a])))
  };
  row.billboard_groups = groups.filter(([, g]) => g.billboard).length;
  row.non_billboard_groups = groups.filter(([, g]) => !g.billboard).length;
  row.billboard_only = row.billboard_groups > 0 && row.non_billboard_groups === 0;
  row.partial_uv_groups = partial.length;
  row.partial_uv_examples = partial
    .slice(0, 3)
    .map(([name, g]) => ({
      name,
      lo: g.lo,
      hi: g.hi,
      area_fraction: g.area / (row.width * row.height)
    }));
  row.uv_outside_crop_groups = groups.filter(([, g]) => g.outside_crop).length;
  row.non_billboard_outside_crop_groups = groups.filter(
    ([, g]) => !g.billboard && g.outside_crop
  ).length;
  row.safe_crop_without_geometry = !row.fully_transparent && row.uv_outside_crop_groups === 0;
  row.original_padded_tiles = Math.ceil((row.width + 4) / 16) * Math.ceil((row.height + 4) / 16);
  row.crop_padded_tiles =
    Math.ceil((row.crop_width + 4) / 16) * Math.ceil((row.crop_height + 4) / 16);
}
function estimate(predicate) {
  const fake = new Map(
    rows.map((r) => {
      const crop = predicate(r),
        width = crop ? r.crop_width : r.width,
        height = crop ? r.crop_height : r.height;
      return [
        r.name,
        {width, height, flags: 6, rgba: new Uint8Array(width * height * 4).fill(255)}
      ];
    })
  );
  const layout = alphaAtlas(fake);
  return {
    side: layout.image.width,
    rgba4444_bytes: layout.image.width ** 2 * 2,
    padded_16x16_tiles: [...fake.values()].reduce(
      (sum, i) => sum + Math.ceil((i.width + 4) / 16) * Math.ceil((i.height + 4) / 16),
      0
    ),
    regions: Object.fromEntries(layout.regions)
  };
}
const layouts = {
  baseline: estimate(() => false),
  all_crop_layout_only: estimate(() => true),
  billboard_only_crop_layout: estimate((r) => r.billboard_only),
  safe_without_geometry_crop_layout: estimate((r) => r.safe_crop_without_geometry)
};
// Reuse the existing shelf packer for page assignment; never emit a page texture.
function split256(names) {
  const pages = [];
  const ordered = [...names].sort(
    (a, b) => images.get(b).height - images.get(a).height || a.localeCompare(b)
  );
  const fit = (names) =>
    alphaAtlas(
      new Map(
        names.map((name) => {
          const i = images.get(name);
          return [
            name,
            {
              width: i.width,
              height: i.height,
              flags: 6,
              rgba: new Uint8Array(i.width * i.height * 4).fill(255)
            }
          ];
        })
      )
    ).image.width <= 256;
  for (const name of ordered) {
    let selected = pages.find((page) => fit([...page, name]));
    if (!selected) {
      selected = [];
      pages.push(selected);
    }
    selected.push(name);
  }
  return pages.map((names, page) => ({page, side: 256, rgba4444_bytes: 256 * 256 * 2, names}));
}
function mixedGroups(assignments) {
  const groups = new Map();
  for (const [name, references] of uses)
    for (const [group] of references) {
      if (!groups.has(group)) groups.set(group, new Set());
      groups.get(group).add(assignments.get(name));
    }
  return [...groups]
    .filter(([, pages]) => pages.size > 1)
    .map(([name, pages]) => ({name, pages: [...pages]}));
}
const allPages = split256(images.keys());
const gatePages = split256([...images.keys()].filter((name) => name !== 'IGATE'));
function splitSummary(pages, standaloneGate) {
  const assignment = new Map();
  for (const page of pages)
    for (const name of page.names) assignment.set(name, 'atlas-' + page.page);
  if (standaloneGate) assignment.set('IGATE', 'IGATE-128');
  const bytes = pages.length * 256 * 256 * 2 + (standaloneGate ? 128 * 128 * 2 : 0);
  return {
    pages,
    standalone_igate: standaloneGate ? {side: 128, rgba4444_bytes: 32768} : null,
    texture_count: pages.length + (standaloneGate ? 1 : 0),
    rgba4444_bytes: bytes,
    saved_bytes: 524288 - bytes,
    mixed_page_sort_groups: mixedGroups(assignment),
    extra_adjacent_binding_boundaries_118: null,
    binding_scope:
      'Not measured: audit inputs do not contain exact per-frame ordered visible group sets. Preserve transparent sort units; mixed-page groups may also require internal draw spans.',
    application_scope:
      'Layout estimate only. A real conversion needs static UV/texture-reference remapping; neither is applied here.'
  };
}
const splitLayouts = {
  all_256: splitSummary(allPages, false),
  igate_128_plus_256: splitSummary(gatePages, true)
};
// Co-locate every original sort group's source images before assigning pages.
const parent = new Map([...images.keys()].map((name) => [name, name]));
function find(name) {
  while (parent.get(name) !== name) name = parent.get(name);
  return name;
}
const groupSources = new Map();
for (const [name, references] of uses)
  for (const [group] of references) {
    if (!groupSources.has(group)) groupSources.set(group, []);
    groupSources.get(group).push(name);
  }
for (const names of groupSources.values())
  for (const name of names.slice(1)) parent.set(find(name), find(names[0]));
const componentMap = new Map();
for (const name of images.keys()) {
  const root = find(name);
  if (!componentMap.has(root)) componentMap.set(root, []);
  componentMap.get(root).push(name);
}
const components = [...componentMap.values()]
  .map((c) => c.sort())
  .sort((a, b) => b.length - a.length || (a[0] < b[0] ? -1 : 1));
assert.deepEqual(
  components.find((c) => c.includes('IGATE')),
  ['IGATE']
);
const smallComponents = components.filter((c) => !c.includes('IGATE'));
assert.ok(
  smallComponents.flat().every((n) => images.get(n).width === 64 && images.get(n).height === 64)
);
let coLocatedPages = null;
for (let mask = 0; mask < 2 ** smallComponents.length; mask++) {
  const a = [],
    b = [];
  smallComponents.forEach((c, i) => ((mask >>> i) & 1 ? a : b).push(...c));
  if (a.length <= 9 && b.length <= 9) {
    coLocatedPages = [a, b];
    break;
  }
}
assert.ok(coLocatedPages, 'Two pages fit whole source co-location components');
for (const page of coLocatedPages) {
  const layout = alphaAtlas(
    new Map(
      page.map((name) => [
        name,
        {width: 64, height: 64, flags: 6, rgba: new Uint8Array(64 * 64 * 4).fill(255)}
      ])
    )
  );
  assert.ok(layout.image.width <= 256, 'Existing packer verifies each co-located page');
}
const coLocatedAssignment = new Map(
  coLocatedPages.flatMap((names, i) => names.map((n) => [n, 'atlas-' + i]))
);
coLocatedAssignment.set('IGATE', 'IGATE-128');
const coLocatedMixed = mixedGroups(coLocatedAssignment);
assert.equal(coLocatedMixed.length, 0, 'All current alpha sort groups stay within a texture');
const coLocation = {
  components,
  pages: coLocatedPages,
  standalone: 'IGATE-128',
  texture_count: 3,
  rgba4444_bytes: 294912,
  saved_bytes: 229376,
  mixed_page_sort_groups: coLocatedMixed,
  extra_adjacent_binding_boundaries_118: null,
  scope:
    'Pure source co-location and existing shelf-packer validation. No actual geometry, UV or texture-reference conversion.'
};
const guardedContentTexels = rows.reduce((sum, r) => sum + r.crop_width * r.crop_height, 0);
const priorBytes = options.has('prior-report') ? await readFile(options.get('prior-report')) : null;
const prior = priorBytes ? JSON.parse(priorBytes) : null;
const ntree2 = rows.find((r) => r.name === 'NTREE2');
const ntree2Region = layouts.billboard_only_crop_layout.regions.NTREE2;
const ntree2MinimumU =
  (ntree2Region.x +
    ntree2.referenced_source_uv_bounds.lo[0] -
    ntree2.crop_with_two_texel_guard[0]) /
  side;
const result = {
  format: 'tankracer.alpha-atlas-audit.v1',
  scene_sha256: hash(await readFile(options.get('scene'))),
  archive_sha256: hash(archiveBytes),
  atlas_sha256: hash(native.bytes),
  alpha_images: rows.length,
  original_decoder: 'tankracer-assets decode-surfaces, direct original FASTFILE SUR entries',
  geometry_changed: false,
  uv_changed: false,
  texture_changed: false,
  bbox_convention: 'Half-open source-texel [x0,y0,x1,y1]',
  bilinear_guard_texels: 2,
  existing_atlas_gutter_texels_per_side: 2,
  tile_alignment_texels: 16,
  layout_scope:
    'Existing alphaAtlas shelf packing with synthetic opaque pixels; dimensions only, no candidate texture emitted',
  working_set_scope:
    'Padded 16x16 allocation tiles only; not measured texture fetches or frame time',
  crop_semantics:
    'UVs outside cropped content cannot simply be remapped inside a shared atlas. They may enter neighboring regions; global clamp does not protect per-region bounds.',
  rectangular_color_extension:
    'Existing extendColors assumes square source images; rectangular crops need a separate implementation before emitting textures.',
  guarded_crop_content_texels: guardedContentTexels,
  atlas_256_capacity_texels: 256 * 256,
  atlas_256_impossible_even_without_gutter: guardedContentTexels > 256 * 256,
  ntree2_crop_minimum_normalized_u: ntree2MinimumU,
  ntree2_existing_layout_rejects_negative_uv: ntree2MinimumU < 0,
  partial_uv_scope:
    'Per-group bounding extent or summed UV face area below 95%; summed areas are not a union when faces overlap.',
  prior_eight_plane: prior
    ? {
        report_sha256: hash(priorBytes),
        decision: prior.decision,
        gpu_cycles_saved: prior.gpu_sample.before.gpu_cycles - prior.gpu_sample.after.gpu_cycles,
        gpu_ms_saved: prior.gpu_sample.before.gpu_ms_54mhz - prior.gpu_sample.after.gpu_ms_54mhz,
        fragments_saved:
          prior.gpu_sample.before.fragments_final - prior.gpu_sample.after.fragments_final,
        cpu_ms: prior.cpu_ms,
        source_matches_current: false
      }
    : null,
  decision: 'Keep the existing atlas and cards; no geometry, UV, texture or default changes.',
  source_co_location_layout: coLocation,
  split_layouts: splitLayouts,
  layouts,
  images: rows
};
await mkdir(output, {recursive: true});
async function atomic(file, bytes) {
  const temporary = file + '.tmp-' + process.pid;
  const stream = await open(temporary, 'wx');
  try {
    await stream.writeFile(bytes);
    await stream.sync();
  } finally {
    await stream.close();
  }
  await rename(temporary, file);
  const readback = await readFile(file);
  assert.equal(readback.length, bytes.length);
  assert.equal(hash(readback), hash(bytes));
}
const json = Buffer.from(JSON.stringify(result, null, 2) + '\n');
await atomic(path.join(output, 'result.json'), json);
const columns = [
  'name',
  'width',
  'height',
  'nonzero_alpha',
  'crop_width',
  'crop_height',
  'billboard_groups',
  'non_billboard_groups',
  'partial_uv_groups',
  'uv_outside_crop_groups',
  'billboard_only',
  'safe_crop_without_geometry',
  'original_padded_tiles',
  'crop_padded_tiles'
];
await atomic(
  path.join(output, 'images.csv'),
  Buffer.from(
    columns.join(',') + '\n' + rows.map((r) => columns.map((c) => r[c]).join(',')).join('\n') + '\n'
  )
);
const report =
  [
    'Keep the existing atlas and cards. This audit changes no geometry, UV, texture, SDK or defaults.',
    `The production atlas has ${rows.length} original alpha SUR sources: ${rows.filter((r) => r.width === 64 && r.height === 64).length} at 64x64 and IGATE at 128x128. Direct source decoding followed by the existing atlas and RGBA4444 encoder reproduces the production atlas SHA256 ${hash(native.bytes)}.`,
    `After a two-texel alpha-bbox guard, content alone occupies ${guardedContentTexels} texels, exceeding a 256x256 atlas (${256 * 256}) before gutter or alignment. Existing shelf packing stays 512x512 / 524288 bytes in every tested scenario. Padded 16x16 allocation tiles: original ${layouts.baseline.padded_16x16_tiles}, all-crop hypothetical ${layouts.all_crop_layout_only.padded_16x16_tiles}, billboard-only hypothetical ${layouts.billboard_only_crop_layout.padded_16x16_tiles}, crops safe without geometry ${layouts.safe_without_geometry_crop_layout.padded_16x16_tiles}. These are layout bounds, not measured texture fetches.`,
    `Only NTREE1, NTREE2 and SCARE are exclusively billboard sources. NTREE1 and SCARE have no guarded bbox crop. NTREE2 could reduce its region from 64x64 to 52x64, but all 104 referencing groups extend outside the crop; existing packing would produce minimum normalized U ${ntree2MinimumU}, rejected by the native UV input contract. TREE2 is shared by 17 billboard and 3 non-billboard groups; its crop would require accounting for all 20 references. TREE3 and BARK are also shared. Per-image partial-reference examples and guarded rectangles are in result.json and images.csv.`,
    'Existing atlas padding is two texels per side, rounded to 16-texel allocation tiles. A remapped UV outside cropped content may reach a neighbor; global ClampToEdge does not clamp a region. Preserving the old full UV domain through virtual transparent padding can remove the apparent allocation saving. Rectangular source color extension also needs explicit support before a real crop producer.',
    prior
      ? `The prior eight-plane candidate remains rejected: ${result.prior_eight_plane.gpu_cycles_saved} GPU cycles (${result.prior_eight_plane.gpu_ms_saved.toFixed(8)} ms) saved in its old measured frame, with ${result.prior_eight_plane.fragments_saved} fewer fragments. CPU timing was unavailable, and this audit makes no new GPU/CPU timing claim.`
      : 'No GPU/CPU timing was run for this audit.',
    `Multiple-atlas estimate with the existing shelf packer: ${allPages.length} 256x256 atlases need ${splitLayouts.all_256.rgba4444_bytes} bytes / ${splitLayouts.all_256.texture_count} textures (save ${splitLayouts.all_256.saved_bytes} bytes). Keeping IGATE as a standalone clamped 128x128 texture plus ${gatePages.length} 256x256 atlases needs ${splitLayouts.igate_128_plus_256.rgba4444_bytes} bytes / ${splitLayouts.igate_128_plus_256.texture_count} textures (save ${splitLayouts.igate_128_plus_256.saved_bytes} bytes). Standalone IGATE needs no atlas gutter; its texture must keep ClampToEdge. Original dimensions are retained in both split estimates.`,
    `Splitting remains a structural option, not a drop-in replacement: it needs static UV and texture-reference remapping. ${splitLayouts.all_256.mixed_page_sort_groups.length} existing alpha sort groups span multiple pages in the all-256 assignment; ${splitLayouts.igate_128_plus_256.mixed_page_sort_groups.length} do in the IGATE-separate assignment. Exact extra binding boundaries across the 118 views are not measured; the source audit has no frozen ordered per-frame visibility lists. Preserve current transparent ordering and account for internal draw splits before adopting it.`,
    `Source co-location can eliminate those five internal group splits without extra bytes: keep {HEDGE2,HEDGE2B,TREE3,TREE4} and {BARK,START3} together. The existing packer accepts pages of ${coLocatedPages[0].length} and ${coLocatedPages[1].length} original 64x64 images; IGATE stays standalone128. Every existing alpha sort group's sources remain in one texture, with 294912 total bytes / 3 textures. Page membership and all connected components are in source_co_location_layout. Adjacent inter-group texture switches remain unmeasured.`,
    'No fully transparent source was found. The audit retains original dimensions for that case and never creates zero-count geometry.'
  ].join('\n\n') + '\n';
await atomic(path.join(output, 'report.txt'), Buffer.from(report));
console.log(
  JSON.stringify({
    alpha_images: rows.length,
    original_atlas_side: side,
    layouts: Object.fromEntries(
      Object.entries(layouts).map(([k, v]) => [k, {...v, regions: undefined}])
    ),
    billboard_only: rows.filter((r) => r.billboard_only).map((r) => r.name),
    split_layouts: Object.fromEntries(
      Object.entries(splitLayouts).map(([k, v]) => [
        k,
        {
          bytes: v.rgba4444_bytes,
          textures: v.texture_count,
          pages: v.pages.map((p) => p.names.length),
          mixed_page_groups: v.mixed_page_sort_groups.length
        }
      ])
    )
  })
);
