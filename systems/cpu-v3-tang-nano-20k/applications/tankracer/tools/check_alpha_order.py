"""Direction-rank integrity and differential checks of the actual RCC sort body."""
import argparse
import json
from pathlib import Path
import struct
import subprocess
import unittest

from alpha_order import build, PITCH_DEGREES, YAW_SECTORS


class Ranks(unittest.TestCase):
    def test_complete_permutations_preserve_depth_and_stable_ids(self):
        rows = []
        for i, xyz in enumerate(((0, 0, 0), (0, 0, 0), (-20, 7, 13), (9, -4, -7))):
            rows.extend([*(v & 65535 for v in xyz), 1, 1, 1, 2, i, 1, 0, i])
        payload, sectors, thresholds = build(rows, 1)
        ranks = struct.unpack(f'<{len(payload)//2}H', payload)
        self.assertEqual(len(thresholds), (len(PITCH_DEGREES)-1)*2)
        for key in range(YAW_SECTORS*len(PITCH_DEGREES)):
            row = ranks[key*sectors*64:(key+1)*sectors*64]
            self.assertEqual(sorted(row[:3]), [0, 1, 2])
            self.assertTrue(all(v == 65535 for v in row[3:]))
        # Zero pitch and yaw sorts by increasing Z; offsets in camera-space Z
        # are common to all records and do not affect this order.
        row = ranks[4*sectors*64:5*sectors*64]
        self.assertLess(row[2], row[0])
        self.assertLess(row[0], row[1])

    def test_missing_or_duplicate_identity_is_rejected(self):
        row = [0]*11
        with self.assertRaisesRegex(ValueError, 'stable IDs'):
            build(row+row, 0)


def check_rust(output):
    output.mkdir(parents=True, exist_ok=True)
    world = Path(__file__).resolve().parents[1]/'rcc/world.rs'
    source = world.read_text(encoding='utf-8')
    body = source[source.index('fn repair_blended_order('):source.index('\nfn fixed(')]
    harness = r'''
use std::ops::{Index,IndexMut};
#[derive(Clone,Copy)] struct Array<T>{data:*mut T,len:usize}
impl<T> Index<u16> for Array<T>{type Output=T;fn index(&self,i:u16)->&T{assert!((i as usize)<self.len);unsafe{&*self.data.add(i as usize)}}}
impl<T> IndexMut<u16> for Array<T>{fn index_mut(&mut self,i:u16)->&mut T{assert!((i as usize)<self.len);unsafe{&mut *self.data.add(i as usize)}}}
fn fixed(a:Array<u16>,i:u16)->i32{(u32::from(a[i])|(u32::from(a[i+1])<<16)) as i32}
#[test] fn seeded_repair_matches_independent_total_order_with_guards(){
 let mut seed=1927u32;
 for case in 0..2000 {
  let n=case%161;let mut values=Vec::new();let mut records=vec![0u16;n*6];
  for i in 0..n {seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);let depth=if case%7==0 {0}else{seed as i32};let id=(n-i+case%17*200) as u16;values.push((id,depth));records[i*6+3]=id;records[i*6+4]=depth as u16;records[i*6+5]=((depth as u32)>>16) as u16;}
  let mut expected=(0..n).collect::<Vec<_>>();expected.sort_by(|&a,&b|values[b].1.cmp(&values[a].1).then(values[a].0.cmp(&values[b].0)));
  let mut guarded=vec![0xdead;n+4];for i in 0..n {guarded[i+2]=(n-1-i) as u16;}
  if case%3==0 {for i in 0..n {seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);let j=(seed as usize)%n;guarded.swap(i+2,j+2);}}
  let data=Array{data:records.as_mut_ptr(),len:records.len()};let order=Array{data:unsafe{guarded.as_mut_ptr().add(2)},len:n};
  repair_blended_order(data,order,n as u16);
  assert_eq!(&guarded[2..n+2],expected.iter().map(|&i|i as u16).collect::<Vec<_>>(),"case {case}");
  assert_eq!(guarded[..2],[0xdead;2]);assert_eq!(guarded[n+2..],[0xdead;2]);
  assert!(blended_ordered(data,order,n as u16));
 }
}
'''
    rust = output/'sort.rs'
    rust.write_text(harness+body, encoding='utf-8')
    executable = output/'sort.exe'
    subprocess.run(['rustc', '--edition=2021', '--test', str(rust), '-o', str(executable)], check=True, timeout=60)
    subprocess.run([str(executable)], check=True, timeout=60)
    (output/'result.json').write_text(json.dumps(dict(cases=2000, maximum_groups=160, source=str(world), exact_order=True))+'\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args, remaining = parser.parse_known_args()
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(Ranks)
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    if not result.wasSuccessful():
        raise SystemExit(1)
    check_rust(args.output)
