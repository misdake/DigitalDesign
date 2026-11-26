"""Check alpha-derived cutout classification against source data and mixed inputs."""
import io
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import tempfile
import unittest
import zipfile

from PIL import Image
from static_assets import read_materials


class Assets:
    def __init__(self):
        self.textures = []

    def texture(self, *args):
        self.textures.append(args)

    def texture_pixels(self, *args, **kwargs):
        self.textures.append(args)


def packed(materials, payloads):
    stream = io.BytesIO()
    with zipfile.ZipFile(stream, 'w') as package:
        for name, data in payloads.items():
            package.writestr(name, data)
    assets = Assets()
    with zipfile.ZipFile(stream) as package:
        source = read_materials(assets, package, materials)
    body = source.split('Buf::new([', 1)[1].split(']);', 1)[0]
    words = [int(word) for word in re.findall(r'\d+', body)]
    return [words[i:i+4] for i in range(0, len(words), 4)], assets


class Cutouts(unittest.TestCase):
    def test_actual_cutout_order_with_blended_prefix_and_uninitialized_scratch(self):
        world = Path(__file__).resolve().parents[1]/'rcc/world.rs'
        source = world.read_text()
        start = source.index('    fn order_cutouts(')
        end = source.index('\n    fn seed_blended(', start)
        method = re.sub(r'unsafe \{ self\.(\w+)\.as_u16_array\(\) \}',
                        r'self.\1', source[start:end])
        fixture = ORDER_FIXTURE.replace('METHOD', method)
        root = Path(__file__).resolve().parents[5]/'target/tmp'
        root.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=root) as folder:
            path = Path(folder)/'cutouts.rs'
            path.write_text(fixture)
            exe = path.with_suffix('.exe' if os.name == 'nt' else '')
            subprocess.run(['rustc', '--edition=2021', '--test', str(path), '-o', str(exe)],
                           check=True, capture_output=True, text=True)
            subprocess.run([str(exe)], check=True, timeout=30)

    def test_native_base_alpha_not_mips_tint_or_shared_descriptor(self):
        binary = struct.pack('<256H', *([0x1230, 0x456f] * 128))
        fractional = struct.pack('<256H', *([0x1237] * 256))
        base = dict(texture='binary', size=16, encoding='gpu-column16', rgba=True,
                    blend=True, mipmaps=True, clamp=True)
        materials = [base, dict(base, tint=[255, 255, 255, 127]),
                     dict(base, blend=False), dict(base, texture='fractional'),
                     dict(base, rgba=False), dict(tint=[1, 2, 3, 255])]
        rows, assets = packed(materials, {'binary': fractional + binary,
                                         'fractional': binary + fractional})
        self.assertEqual([row[1] for row in rows], [9, 1, 1, 1, 1, 0])
        self.assertEqual([row[0] for row in rows], [1, 1, 1, 2, 3, 0])
        self.assertEqual(len(assets.textures), 3)

    def test_image_fractional_alpha_and_grid_bits_stay_independent(self):
        def image(alpha):
            stream = io.BytesIO()
            value = Image.new('RGBA', (2, 2), (1, 2, 3, 255))
            value.putpixel((0, 0), (1, 2, 3, alpha))
            value.save(stream, 'PNG')
            return stream.getvalue()
        base = dict(texture='binary.png', rgba=True, blend=True, clamp=True,
                    mipmaps=True, atlas_grid_log2=3)
        rows, _ = packed([base, dict(base, texture='fractional.png')],
                         {'binary.png': image(0), 'fractional.png': image(128)})
        self.assertEqual([row[1] for row in rows], [15, 7])
        self.assertEqual([(row[1] >> 1) & 3 for row in rows], [3, 3])

    def test_all_default_alpha_textures_are_binary_and_billboards_use_them(self):
        path = Path(__file__).resolve().parents[1]/'assets/england/scene-grid-atlas.zip'
        with zipfile.ZipFile(path) as package:
            scene = json.loads(package.read('scene.json'))
            source = read_materials(Assets(), package, scene['materials'])
            body = source.split('Buf::new([', 1)[1].split(']);', 1)[0]
            words = [int(word) for word in re.findall(r'\d+', body)]
            alpha = set()
            for index, material in enumerate(scene['materials']):
                if not material.get('blend'):
                    continue
                self.assertEqual(words[index * 4 + 1] & 8, 8)
                size = material['size']
                base = package.read(material['texture'])[-max(512, size * size * 2):]
                values = {word & 15 for word, in struct.iter_unpack('<H', base)}
                self.assertEqual(values, {0, 15})
                alpha.add(material['texture'])
            self.assertEqual(len(alpha), 3)
            self.assertTrue(any(group.get('billboard') and
                                words[group['material'] * 4 + 1] & 8
                                for group in scene['groups']))


ORDER_FIXTURE = r'''
#![allow(unused_mut, non_upper_case_globals)]
use std::ops::{Index,IndexMut};
const GROUP_COUNT:u16=12;
const OPAQUE_COUNT:u16=4;
const MAP_TEXTURE_COUNT:u16=3;
struct Table([u16;16]);
impl Index<u16> for Table { type Output=u16;
    fn index(&self,i:u16)->&u16 { &self.0[i as usize] }
}
static MAP_MATERIALS:Table=Table([3,0,0,0, 1,0,0,0, 2,0,0,0, 1,0,0,0]);
#[derive(Clone,Copy)]
struct Array { data:*mut u16, initialized:*mut bool, len:usize }
impl Array {
    fn new(len:usize)->Self { Self { data:Box::leak(vec![0;len].into_boxed_slice()).as_mut_ptr(),
        initialized:Box::leak(vec![false;len].into_boxed_slice()).as_mut_ptr(),len } }
}
impl Index<u16> for Array { type Output=u16;
    fn index(&self,i:u16)->&u16 { assert!((i as usize)<self.len);
        unsafe { assert!(*self.initialized.add(i as usize),"uninitialized read {i}");
            &*self.data.add(i as usize) } }
}
impl IndexMut<u16> for Array {
    fn index_mut(&mut self,i:u16)->&mut u16 { assert!((i as usize)<self.len);
        unsafe { *self.initialized.add(i as usize)=true; &mut *self.data.add(i as usize) } }
}
struct World { alpha_mask:Array, alpha_ordinals:Array, blended_draws:Array, blended_order:Array }
impl World { METHOD }
#[test]
fn subsets_and_candidate_permutations_keep_prefix_and_only_read_present_records() {
    for subset in 0u16..256 {
        let ids:Vec<u16>=(0..8).filter(|id|subset&(1<<id)!=0).collect();
        let count=ids.len() as u16;
        for reverse in [false,true] {
            // Eight total records: any blended prefix fits exactly up to the cutout tail.
            let blended=8-count;
            let mut w=World { alpha_mask:Array::new(1), alpha_ordinals:Array::new(8),
                blended_draws:Array::new(48),blended_order:Array::new(8) };
            for i in 0..blended { w.blended_order[i]=0xaffe; }
            let mut present=ids.clone();
            if reverse { present.reverse(); }
            for (i,&id) in present.iter().enumerate() {
                let at=(7-i as u16)*6;
                w.blended_draws[at+2]=(id%4) | if id&1!=0 {32768} else {0};
                w.blended_draws[at+3]=id+OPAQUE_COUNT;
            }
            w.order_cutouts(blended,count);
            for i in 0..blended { assert_eq!(w.blended_order[i],0xaffe); }
            let mut expected=ids.clone();
            expected.sort_by_key(|id|(MAP_MATERIALS[(id%4)*4],*id));
            let actual:Vec<u16>=(0..count).map(|i|
                w.blended_draws[w.blended_order[blended+i]*6+3]-OPAQUE_COUNT).collect();
            assert_eq!(actual,expected);
        }
    }
}
'''


if __name__ == '__main__':
    unittest.main()
