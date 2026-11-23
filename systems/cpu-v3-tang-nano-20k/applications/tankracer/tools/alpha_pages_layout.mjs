// Validate reproducible source-image placement without producing runtime commands.
import assert from 'node:assert/strict';

export function alphaPagesLayout(config, {sceneSha256, archiveSha256, images}) {
  assert.ok(config && typeof config === 'object', 'Layout must be an object');
  let layout;
  let reference;
  if (config.format === 'tankracer.alpha-pages.v1') {
    layout = config;
    reference = config.source;
    assert.equal(layout.standalone, 'IGATE', 'IGATE must remain standalone');
  } else {
    assert.equal(
      config.format,
      'tankracer.alpha-atlas-audit.v1',
      'Unsupported alpha layout format'
    );
    layout = config.source_co_location_layout;
    reference = config;
    assert.equal(layout?.standalone, 'IGATE-128', 'IGATE must remain standalone');
  }
  assert.ok(reference && typeof reference === 'object', 'Missing source reference');
  for (const [key, actual] of [
    ['scene_sha256', sceneSha256],
    ['archive_sha256', archiveSha256]
  ]) {
    assert.match(reference[key] ?? '', /^[0-9a-f]{64}$/, 'Invalid source SHA: ' + key);
    assert.equal(reference[key], actual, 'Source SHA mismatch: ' + key);
  }
  assert.ok(Array.isArray(layout.pages) && layout.pages.length === 2, 'Expected two alpha pages');
  const assigned = new Set(['IGATE']);
  for (const page of layout.pages) {
    assert.ok(
      Array.isArray(page) && page.length > 0 && page.length <= 9,
      'Page requires 1..9 sources'
    );
    for (const name of page) {
      assert.equal(typeof name, 'string', 'Source name must be a string');
      assert.ok(!assigned.has(name), 'Duplicate or standalone source in pages: ' + name);
      assigned.add(name);
      const image = images.get(name);
      assert.ok(image && (image.flags & 3) !== 0, 'Unknown or opaque alpha source: ' + name);
      assert.ok(image.width === 64 && image.height === 64, 'Page source must be 64x64: ' + name);
    }
  }
  const igate = images.get('IGATE');
  assert.ok(
    igate && (igate.flags & 3) !== 0 && igate.width === 128 && igate.height === 128,
    'Standalone IGATE must be alpha 128x128'
  );
  const expected = [...images]
    .filter(([, image]) => (image.flags & 3) !== 0)
    .map(([name]) => name)
    .sort();
  assert.deepEqual([...assigned].sort(), expected, 'All alpha sources assigned once');
  return {pages: layout.pages.map((page) => [...page]), standalone: 'IGATE'};
}
