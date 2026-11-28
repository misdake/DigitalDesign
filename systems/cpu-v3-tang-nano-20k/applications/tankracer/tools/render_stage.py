"""Lay out static selection tables, visibility cache and range offsets in one page."""
import json
import re
import struct


TABLES = ('MAP_RANGE_SELECT_INDEX', 'MAP_CLUSTERS', 'MAP_RING_CHUNKS',
          'MAP_RING_SEGMENTS', 'MAP_ROUTE_LOCATION_CELLS', 'MAP_ROUTE_LOCATION_LISTS',
          'MAP_MATERIALS', 'MAP_PVS_QUADRANT', 'MAP_ALPHA_PITCH_THRESHOLDS')


def prepare(assets, source, hybrid, incremental, groups, units):
    words = []
    getters = []
    tables = {}
    for name in TABLES:
        pattern = r'pub static ' + name + r':\s*Buf<u16,\s*(\d+)>\s*=\s*Buf::new\(\[([^]]*)\]\);'
        match = re.search(pattern, source)
        if match is None:
            raise ValueError(f'missing selection table {name}')
        values = [int(v) for v in match[2].split(',') if v.strip()]
        if len(values) != int(match[1]) or any(not 0 <= v <= 65535 for v in values):
            raise ValueError(f'invalid selection table {name}')
        if hybrid:
            tables[name] = dict(first_word=len(words), words=len(values))
            getters.append(f'pub fn {name.lower()}()->Array<u16> {{ unsafe {{ '
                           f'Ptr::from_addr({32768 + len(words)}).as_u16_array() }} }}\n')
            words.extend(values)
            source = source[:match.start()] + source[match.end():]
        else:
            getters.append(f'pub fn {name.lower()}()->Array<u16> {{ {name}.as_array() }}\n')

    if not hybrid:
        module = ('use crate::dsl_rt::*;\n'
                  'pub fn init() {}\npub fn begin()->u16 {0}\npub fn end(saved:u16) {}\n'
                  'pub fn all()->Ptr {Ptr::from_addr(0)}\n'
                  'pub fn visible()->Ptr {Ptr::from_addr(0)}\n'
                  'pub fn fringe()->Ptr {Ptr::from_addr(0)}\n'
                  'pub fn ranges()->Ptr {Ptr::from_addr(0)}\n')
        layout = dict(bytes=0)
    else:
        prefix = (len(words) + 63) & ~63
        cache_words = groups * 7 if incremental else 0
        ranges = prefix + cache_words
        end = ranges + units * 5
        if end > 16384:
            raise ValueError(f'static selection page needs {end * 2} bytes; limit is 32768')
        assets.bytes('MAP_STAGE_METADATA', struct.pack(f'<{len(words)}H', *words))
        layout = dict(bytes=32768, metadata_bytes=len(words) * 2,
                      prefix_bytes=prefix * 2, visibility_bytes=cache_words * 2,
                      ranges_bytes=units * 10, used_bytes=end * 2,
                      spare_bytes=32768-end * 2, tables=tables,
                      metadata_file='map_stage_metadata.bin')
        module = f'''//! CPU-owned selection data. The caller restores DSEG2 at each phase boundary.
use crate::dsl_rt::*;
mod boot_resources;
mod static_buffer;
mod gpu_window;
use boot_resources::*;
use static_buffer::StaticBuffer;
use gpu_window::gpu_window_asset_read;
#[static_buffer]
const STORAGE:StaticBuffer=StaticBuffer::new(32768,32768);
pub fn begin()->u16 {{
    let saved=read_dseg_page(2);
    unsafe {{write_dseg_page(2,STORAGE.first_sector()>>8);}}
    saved
}}
pub fn end(saved:u16) {{unsafe {{write_dseg_page(2,saved);}}}}
pub fn init() {{
    let saved=begin();
    if !gpu_window_asset_read(unsafe {{Ptr::from_addr(32768).as_u16_array()}},
        MAP_STAGE_METADATA.first_sector,0,{len(words)}) {{halt(0x7b60);}}
    end(saved);
}}
pub fn all()->Ptr {{Ptr::from_addr({32768+prefix})}}
pub fn visible()->Ptr {{Ptr::from_addr({32768+prefix+groups})}}
pub fn fringe()->Ptr {{Ptr::from_addr({32768+prefix+groups*2})}}
pub fn ranges()->Ptr {{Ptr::from_addr({32768+ranges})}}
'''
    (assets.output / 'map_stage.rs').write_text(module)
    return source + ''.join(getters), layout


def read_table(folder, name):
    """Read one immutable table from either exported storage layout."""
    if name not in TABLES:
        raise ValueError(f'unknown selection table {name}')
    manifest = json.loads((folder / 'assets.json').read_text())
    stage = manifest.get('render_stage', {})
    if stage.get('bytes', 0):
        row = stage['tables'][name]
        payload = (folder / 'map_stage_metadata.bin').read_bytes()
        first, count = row['first_word'], row['words']
        if first < 0 or count < 0 or (first + count) * 2 > len(payload):
            raise ValueError(f'selection table exceeds metadata: {name}')
        return list(struct.unpack_from(f'<{count}H', payload, first * 2))
    source = (folder / 'tankracer_static.rs').read_text()
    match = re.search(r'pub static ' + name + r':.*?Buf::new\(\[(.*?)\]\);', source, re.S)
    if match is None:
        raise ValueError(f'missing selection table {name}')
    return [int(v) for v in match[1].split(',') if v.strip()]
