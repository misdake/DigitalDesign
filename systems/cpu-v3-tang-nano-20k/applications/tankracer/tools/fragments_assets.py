"""Pack representative authored fragment and failure cases for normal loading."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess
import sys


def binary(path, values):
    path.write_bytes(struct.pack('<' + 'H' * len(values), *values))


def words(values):
    return [word for value in values for word in [value & 65535, (value >> 16) & 65535]]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    out = parser.parse_args().output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    owner = Path(__file__).resolve().parent
    reference = Path(os.environ['TANKRACER_REFERENCE']).resolve()
    subprocess.run([sys.executable, str(owner / 'mesh_break_assets.py'), '--reference', str(reference),
                    '--output', str(out)], check=True, timeout=400)
    data = json.loads((out / 'mesh-break.json').read_text())
    by_tag = {c['tag']: c for c in data['cases']}
    tags = ['activate-0-1', 'activate-7-100000', 'failure-5', 'failure-8']
    for count in [36, 8, 16, 12]:
        item = next(i for i in data['authored'] if i['faces'] == count)
        index = item['object']
        tags += [f'authored-activate-{index}', f'authored-tick-{index}-0', f'authored-tick-{index}-15']
    cases = [by_tag[tag] for tag in tags]
    assert len(cases) == 16
    cases = [dict(input=c['input'], frames=[c['expected']], tag=c['tag'], kind=0) for c in cases]
    subprocess.run([sys.executable, str(owner / 'scene_fragment_assets.py'), '--reference', str(reference),
                    '--output', str(out)], check=True, timeout=400)
    scene_data = json.loads((out / 'scene-fragments.json').read_text())
    first = scene_data['objects'][0]
    for variant in [0, 1, 4, 5, 7, 11]:
        c = next(c for c in scene_data['cases'] if c['object'] == first and c['variant'] == variant and c['limit'] == 100000)
        steps = c['steps'][:4]
        control_offset = len(c['input']) - 1 - len(c['steps']) * 2
        inp = c['input'][:control_offset] + [len(steps)] + [v for pair in steps for v in pair]
        cases.append(dict(input=inp, frames=c['frames'][:len(steps)], tag=f'scene-{first}-{variant}', kind=1))
    subprocess.run([sys.executable, str(owner / 'mesh_store_assets.py'), '--reference', str(reference),
                    '--output', str(out)], check=True, timeout=400)
    store_data = json.loads((out / 'mesh-store.json').read_text())
    for index, count in [(0, 16), (len(store_data['cases']) - 1, 8)]:
        c = store_data['cases'][index]
        inp = c['input'][:c['input'][6] + count * 4]
        inp[3] = count
        cases.append(dict(input=inp, frames=c['frames'][:count], tag=f'store-{index}', kind=2))
    subprocess.run([sys.executable, str(owner / 'auxiliary_hit_assets.py'), '--reference', str(reference),
                    '--output', str(out)], check=True, timeout=400)
    hit_data = json.loads((out / 'auxiliary-hit.json').read_text())
    for model, variant in [(0, 0), (3, 0), (0, 3), (0, 15)]:
        c = next(c for c in hit_data['cases'] if c['model'] == model and c['variant'] == variant and c['limit'] > 1)
        cases.append(dict(input=c['input'], frames=c['frames'], tag=f'auxiliary-{model}-{variant}', kind=3))
    subprocess.run([sys.executable, str(owner / 'parent_auxiliary_assets.py'), '--reference', str(reference),
                    '--output', str(out)], check=True, timeout=400)
    parent_data = json.loads((out / 'parent-auxiliary.json').read_text())
    parents = [next(c for c in parent_data['cases'] if c['object'] == obj and c['variant'] == variant)
               for obj, variant in [(69, 0), (69, 3), (69, 5), (69, 9), (172, 0), (172, 6), (172, 7), (172, 8)]]
    parent_inputs, parent_expected, parent_directory = [], [], []
    for c in parents:
        parent_directory.extend([len(parent_inputs) & 65535, len(parent_inputs) >> 16, len(c['input']),
                                 len(parent_expected) & 65535, len(parent_expected) >> 16])
        parent_inputs.extend(c['input'])
        for frame in c['frames']:
            parent_expected.extend([len(frame), *frame])
    binary(out / 'parent-inputs.bin', parent_inputs)
    binary(out / 'parent-expected.bin', parent_expected)
    binary(out / 'parent-directory.bin', parent_directory)
    inputs, expected, directory = [], [], []
    maximum_scratch = 1536
    for c in cases:
        inp = c['input'] if c['kind'] >= 2 else words(c['input'])
        directory.extend([len(inputs) & 65535, len(inputs) >> 16, len(inp), len(expected) & 65535, len(expected) >> 16, c['kind']])
        inputs.extend(inp)
        for frame in c['frames']:
            exp = frame if c['kind'] >= 2 else words(frame)
            expected.append(len(exp))
            expected.extend(exp)
    binary(out / 'inputs.bin', inputs)
    binary(out / 'expected.bin', expected)
    binary(out / 'directory.bin', directory)
    resources = []
    declarations = ['use crate::dsl_rt::*;mod static_assets;use static_assets::AssetBytes;\n']
    for symbol, name in [('INPUTS', 'inputs.bin'), ('EXPECTED', 'expected.bin'), ('DIRECTORY', 'directory.bin'), ('SINE', 'sine.i16.bin'), ('SCENE', 'scene-fragments.bin'), ('CELLS', 'auxiliary-cells.bin'), ('CATALOG', 'mesh-catalog.bin'), ('CATALOG_EXPECTED', 'mesh-catalog-expected.bin'), ('CATALOG_CONTROLS', 'mesh-catalog-controls.bin'), ('PARENT_INPUTS', 'parent-inputs.bin'), ('PARENT_EXPECTED', 'parent-expected.bin'), ('PARENT_DIRECTORY', 'parent-directory.bin'), ('ANGLE', 'angle.i16.bin'), ('INVERSE_COSINE', 'cosine-angle.bin')]:
        path = out / name
        alignment = 32768 if symbol == 'CELLS' else 128
        declarations.append(f'#[asset(align={alignment})] pub static FRAGMENTS_{symbol}:AssetBytes=AssetBytes::new(include_bytes!({json.dumps(path.as_posix())}));\n')
        resources.append(dict(file=name, placement='source', symbol='FRAGMENTS_' + symbol,
                              bytes=path.stat().st_size, sha256=hashlib.sha256(path.read_bytes()).hexdigest()))
    input_words = max(len(c['input']) * (1 if c['kind'] >= 2 else 2) for c in cases)
    output_words = max(len(frame) * (1 if c['kind'] >= 2 else 2) for c in cases for frame in c['frames'])
    assert max(input_words, output_words, maximum_scratch) < 8192
    catalog = json.loads((out / 'mesh-catalog.json').read_text())
    restart_ids = ','.join(str(i) for i in catalog['restart_resources'])
    declarations.append(f"pub static FRAGMENTS_CATALOG_RESTART_IDS:Buf<u16,3>=Buf::new([{restart_ids}]);\n")
    catalog_words = 2560 + catalog['source_metadata_bytes'] // 2 + catalog['compact_index_bytes'] // 2
    declarations.append(f"pub const FRAGMENTS_CATALOG_WORDS:u16={catalog_words};\n")
    declarations.append(f"pub const FRAGMENTS_CATALOG_OPERATIONS:u16={catalog['operations']};\n")
    declarations.append(f"pub const FRAGMENTS_CATALOG_INITIAL_PAYLOAD:u16={catalog['initial_payload_bytes'] // 2};\n")
    declarations.append(f"pub const FRAGMENTS_CATALOG_FINAL_PAYLOAD:u16={catalog['final_payload_bytes'] // 2};\n")
    declarations.append(f"pub const FRAGMENTS_CATALOG_ARENA_USED:u16={catalog['external_payload_bytes'] // 2 + catalog['external_blocks']};\n")
    layout = [word for row in catalog['motion_layout'] for word in row]
    assert catalog['motion_words'] + 8 + 36 * 16 <= 16384
    declarations.append(f"pub static FRAGMENTS_MOTION_LAYOUT:Buf<u16,{len(layout)}>=Buf::new([{','.join(map(str, layout))}]);\n")
    declarations.append(f"pub const FRAGMENTS_MOTION_OWNERS:u16={len(catalog['motion_layout'])};\n")
    declarations.append(f"pub const FRAGMENTS_MOTION_WORDS:u16={catalog['motion_words']};\n")
    declarations.append(f"pub struct CatalogStorage{{pub memory:Buf<u16,{catalog_words}>,pub output:Buf<u16,{catalog['maximum_part_words']}>,pub scratch:Buf<u16,8>,pub comparison:Buf<u16,256>}}\n")
    declarations.append(f"pub fn catalog_storage()->CatalogStorage{{CatalogStorage{{memory:Buf::new([0;{catalog_words}]),output:Buf::new([0;{catalog['maximum_part_words']}]),scratch:Buf::new([0;8]),comparison:Buf::new([0;256])}}}}\n")
    declarations.append(f'pub const FRAGMENTS_CASES:u16={len(cases)};\n')
    declarations.append(f'pub struct FragmentsStorage{{pub input:Buf<u16,{input_words}>,pub output:Buf<u16,{output_words}>,pub scratch:Buf<u16,{maximum_scratch}>,pub lifetime:Buf<u16,11988>,pub comparison:Buf<u16,256>}}\n')
    declarations.append(f'pub fn fragments_storage()->FragmentsStorage{{FragmentsStorage{{input:Buf::new([0;{input_words}]),output:Buf::new([0;{output_words}]),scratch:Buf::new([0;{maximum_scratch}]),lifetime:Buf::new([0;11988]),comparison:Buf::new([0;256])}}}}\n')
    parent_input_words = max(len(c['input']) for c in parents)
    parent_output_words = max(len(frame) for c in parents for frame in c['frames'])
    declarations.append(f'pub const PARENT_CASES:u16={len(parents)};\n')
    declarations.append(f'pub struct ParentStorage{{pub input:Buf<u16,{parent_input_words}>,pub output:Buf<u16,{parent_output_words}>,pub scratch:Buf<u16,1536>,pub lifetime:Buf<u16,11988>,pub resources:Buf<u16,76>,pub comparison:Buf<u16,256>}}\n')
    declarations.append(f'pub fn parent_storage()->ParentStorage{{ParentStorage{{input:Buf::new([0;{parent_input_words}]),output:Buf::new([0;{parent_output_words}]),scratch:Buf::new([0;1536]),lifetime:Buf::new([0;11988]),resources:Buf::new([0;76]),comparison:Buf::new([0;256])}}}}\n')
    (out / 'fragments_assets.rs').write_text(''.join(declarations))
    imports = 'use crate::dsl_rt::*;use crate::{attitude,integer,mesh_break,entities,identity_capture,lifecycle,ordered,scene_lifecycle,scene_query,scene_world,spatial,static_assets,scene_fragments,scene_meshes,mesh_catalog,ground,terrain,scene_parent,mesh_storage};\n'
    helpers = ['scene_lifecycle_case.rs', 'mesh_break_case.rs', 'scene_fragment_case.rs', 'mesh_store_case.rs', 'auxiliary_hit_case.rs', 'mesh_catalog_case.rs', 'fragment_motion_case.rs', 'parent_auxiliary_case.rs', 'mesh_storage_case.rs']
    (out / 'fragment_case.rs').write_text(imports + '\n'.join((owner / name).read_text() for name in helpers))
    (out / 'trig_lookup.rs').write_bytes((owner.parent / 'rcc/trig_asset_lookup.rs').read_bytes())
    sources = json.loads((out / 'mesh-break-sources.json').read_text())['sources']
    sources += json.loads((out / 'scene-fragment-sources.json').read_text())['sources']
    sources += json.loads((out / 'mesh-store-sources.json').read_text())['sources']
    sources += json.loads((out / 'auxiliary-hit-sources.json').read_text())['sources']
    sources += json.loads((out / 'parent-sources.json').read_text())['sources']
    for p in [Path(__file__).resolve(), *[owner / name for name in helpers], owner.parent / 'rcc/trig_asset_lookup.rs']:
        sources.append(dict(file=str(p), sha256=hashlib.sha256(p.read_bytes()).hexdigest()))
    metadata = dict(residency=dict(bank_bytes=32768, motion_bytes=catalog['motion_words']*2,
                    final_external_payload_bytes=catalog['external_payload_bytes'],
                    final_external_blocks=catalog['external_blocks'],
                    final_combined_bytes=catalog['motion_words']*2+catalog['external_payload_bytes']+catalog['external_blocks']*2+2,
                    cpu_payload_bytes=catalog['initial_payload_bytes'],cpu_heap_bytes=8192),
                    catalog=catalog, parents=dict(cases=len(parents), frames=sum(len(c['frames']) for c in parents), checked_words=sum(len(frame) for c in parents for frame in c['frames'])), cases=[dict(tag=c['tag'], kind=c['kind'], frames=len(c['frames'])) for c in cases],
                    frames=sum(len(c['frames']) for c in cases),
                    checked_i32_values=sum(len(frame) for c in cases if c['kind'] < 2 for frame in c['frames']),
                    checked_resource_words=sum(len(frame) for c in cases if c['kind'] == 2 for frame in c['frames']),
                    checked_auxiliary_words=sum(len(frame) for c in cases if c['kind'] == 3 for frame in c['frames']),
                    scope='Complete ENGLAND resource import and all authored auxiliary copies; original fragment kernels, auxiliary-contact execution with real ENGLAND terrain, persistent auxiliary-update class calls and live mesh ownership; complete geometry, revisions, RNG and committed failures through normal asset loading')
    sources = list({s['file']: s for s in sources}.values())
    (out / 'assets.json').write_text(json.dumps(dict(resources=resources, sector_relocations=[], metadata=metadata, sources=sources), indent=2) + '\n')


if __name__ == '__main__':
    main()
