import assert from 'node:assert/strict';
import test from 'node:test';
import {alphaPagesLayout} from './alpha_pages_layout.mjs';

const scene = 'a'.repeat(64);
const archive = 'b'.repeat(64);
const names = [
  'BARK',
  'BRIDGE',
  'BRIDGE2',
  'CATTLEG',
  'FENHED',
  'SCARE',
  'SIGNS2',
  'START3',
  'CANNON',
  'HEDGE2',
  'HEDGE2B',
  'HEDGE2C',
  'NTREE1',
  'NTREE2',
  'TREE2',
  'TREE3',
  'TREE4'
];
const images = new Map(names.map((name) => [name, {flags: 1, width: 64, height: 64}]));
images.set('IGATE', {flags: 2, width: 128, height: 128});
const context = {sceneSha256: scene, archiveSha256: archive, images};
function config() {
  return {
    format: 'tankracer.alpha-pages.v1',
    source: {scene_sha256: scene, archive_sha256: archive},
    pages: [names.slice(0, 8), names.slice(8)],
    standalone: 'IGATE'
  };
}

test('versioned and legacy descriptions preserve exact page order', () => {
  const current = config();
  const expected = {pages: current.pages, standalone: 'IGATE'};
  assert.deepEqual(alphaPagesLayout(current, context), expected);
  assert.deepEqual(
    alphaPagesLayout(
      {
        format: 'tankracer.alpha-atlas-audit.v1',
        ...current.source,
        source_co_location_layout: {...expected, standalone: 'IGATE-128'}
      },
      context
    ),
    expected
  );
});
test('reject unsupported version and missing source reference', () => {
  const current = config();
  current.format = 'tankracer.alpha-pages.v2';
  assert.throws(() => alphaPagesLayout(current, context), /Unsupported alpha layout format/);
  current.format = 'tankracer.alpha-pages.v1';
  delete current.source;
  assert.throws(() => alphaPagesLayout(current, context), /Missing source reference/);
});
test('reject modified scene or original archive', () => {
  for (const field of ['scene_sha256', 'archive_sha256']) {
    const current = config();
    current.source[field] = 'c'.repeat(64);
    assert.throws(() => alphaPagesLayout(current, context), /Source SHA mismatch/);
    current.source[field] = 'not-a-hash';
    assert.throws(() => alphaPagesLayout(current, context), /Invalid source SHA/);
  }
});
test('reject missing, repeated, unknown, and standalone page sources', () => {
  const mutations = [
    (c) => c.pages[0].pop(),
    (c) => {
      c.pages[1][0] = c.pages[0][0];
    },
    (c) => {
      c.pages[0][0] = 'UNKNOWN';
    },
    (c) => {
      c.pages[0][0] = 'IGATE';
    }
  ];
  for (const mutate of mutations) {
    const current = config();
    mutate(current);
    assert.throws(() => alphaPagesLayout(current, context));
  }
});
test('reject malformed page counts and overfull page', () => {
  const current = config();
  current.pages.push([]);
  assert.throws(() => alphaPagesLayout(current, context), /Expected two alpha pages/);
  current.pages = [names.slice(0, 10), names.slice(10)];
  assert.throws(() => alphaPagesLayout(current, context), /Page requires/);
});
test('reject opaque source, source dimensions and standalone changes', () => {
  const changedImages = new Map(images);
  changedImages.set('BARK', {flags: 0, width: 64, height: 64});
  assert.throws(
    () => alphaPagesLayout(config(), {...context, images: changedImages}),
    /Unknown or opaque/
  );
  changedImages.set('BARK', {flags: 1, width: 128, height: 64});
  assert.throws(() => alphaPagesLayout(config(), {...context, images: changedImages}), /64x64/);
  changedImages.set('BARK', images.get('BARK'));
  changedImages.set('IGATE', {flags: 2, width: 64, height: 64});
  assert.throws(() => alphaPagesLayout(config(), {...context, images: changedImages}), /128x128/);
  const current = config();
  current.standalone = 'BARK';
  assert.throws(() => alphaPagesLayout(current, context), /IGATE must remain standalone/);
});
