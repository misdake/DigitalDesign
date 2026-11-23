"""Run the actual World sort and member guard in a checked host Rust fixture."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess


def function(source, name):
    start = source.index("fn " + name + "(")
    end = source.index("\n}", start) + 2
    return source[start:end]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    world = Path(__file__).resolve().parents[1] / "rcc/world.rs"
    source = world.read_text()
    start = source.index("                            if same_blended {")
    end = source.index("                            output[target + 4]", start)
    member = source[start:end].replace("self.previous_blended", "previous").replace(
        "result.blended", "count").replace("words[record + 12]", "identity")
    fixture = PRELUDE + TESTS.replace("MEMBER_GUARD", member) + function(source, "blended_ordered") + function(source, "sort_back_to_front")
    path = args.output / "fixture.rs"
    data = fixture.encode()
    temporary = args.output / "fixture.rs.tmp"
    with temporary.open("xb") as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)
    assert path.read_bytes() == data and b"\0" not in data
    executable = args.output / "fixture.exe"
    subprocess.run(["rustc", "--edition=2021", "--test", str(path), "-o", str(executable)], check=True)
    result = subprocess.run([str(executable), "--nocapture"], check=True, capture_output=True, text=True)
    print(result.stdout, end="")
    report = {"world_sha256": hashlib.sha256(world.read_bytes()).hexdigest(),
              "fixture_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
              "checks": "actual source member guard and sort; checked uninitialized reads; oracle exact",
              "stdout": result.stdout}
    data = (json.dumps(report, indent=2) + "\n").encode()
    temporary = args.output / "result.json.tmp"
    with temporary.open("xb") as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, args.output / "result.json")
    assert (args.output / "result.json").read_bytes() == data and b"\0" not in data


PRELUDE = r'''
#![allow(unused_mut)]
use std::ops::{Index, IndexMut};
#[derive(Clone, Copy)]
struct Array<T> { data: *mut T, initialized: *mut bool, writes: *mut usize }
impl<T> Index<u16> for Array<T> {
    type Output = T;
    fn index(&self, index: u16) -> &T {
        assert!((index as usize) < 120);
        unsafe {
            assert!(*self.initialized.add(index as usize), "uninitialized read at {index}");
            &*self.data.add(index as usize)
        }
    }
}
impl<T> IndexMut<u16> for Array<T> {
    fn index_mut(&mut self, index: u16) -> &mut T {
        assert!((index as usize) < 120);
        unsafe {
            *self.initialized.add(index as usize) = true;
            *self.writes += 1;
            &mut *self.data.add(index as usize)
        }
    }
}
fn fixed(words: Array<u16>, at: u16) -> i32 {
    (words[at] as u32 | (words[at + 1] as u32) << 16) as i32
}
struct Fixture {
    records: [u16; 120], record_init: [bool; 120], record_writes: usize,
    order: [u16; 120], order_init: [bool; 120], order_writes: usize,
    previous: u16,
}
impl Fixture {
    fn new() -> Self {
        Self { records: [0;120], record_init: [false;120], record_writes: 0,
               order: [0;120], order_init: [false;120], order_writes: 0, previous: 0 }
    }
    fn frame(&mut self, items: &[(u16, i32)], fast: Option<bool>) {
        let mut output = Array { data: self.records.as_mut_ptr(),
            initialized: self.record_init.as_mut_ptr(), writes: &mut self.record_writes };
        let order = Array { data: self.order.as_mut_ptr(),
            initialized: self.order_init.as_mut_ptr(), writes: &mut self.order_writes };
        let previous = self.previous;
        let mut same_blended = previous != 0;
        let mut count: u16 = 0;
        for &(identity, depth) in items {
            let target = count * 6;
'''
TESTS = r'''
MEMBER_GUARD
            output[target + 4] = depth as u16;
            output[target + 5] = (depth >> 16) as u16;
            count += 1;
        }
        same_blended = same_blended && count == previous;
        self.previous = count;
        let before_writes = self.order_writes;
        if !same_blended || !blended_ordered(output, order, count) {
            sort_back_to_front(output, order, count);
        }
        let actual: Vec<u16> = (0..count).map(|i| items[order[i] as usize].0).collect();
        let mut expected = items.to_vec();
        expected.sort_by(|a,b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        assert_eq!(actual, expected.iter().map(|x| x.0).collect::<Vec<_>>());
        if let Some(expect_fast) = fast {
            assert!(count >= 2);
            assert_eq!(self.order_writes == before_writes, expect_fast);
        }
    }
}
#[test] fn first_empty_single_and_growth_never_read_uninitialized() {
    let mut f = Fixture::new();
    f.frame(&[], None); f.frame(&[(8,2)], None);
    f.frame(&[(8,2),(3,4),(2,1)], Some(false));
    f.frame(&[], None); f.frame(&[(3,2)], None);
    f.frame(&[(3,6)], None); f.frame(&[(7,6)], None); f.frame(&[], None);
}
#[test] fn unchanged_members_refresh_depth_and_reuse_order() {
    let mut f = Fixture::new(); f.frame(&[(8,9),(2,3),(5,-8)], Some(false));
    f.frame(&[(8,8),(2,4),(5,-7)], Some(true));
}
#[test] fn equal_depth_obeys_stable_id_and_then_reuses() {
    let mut f = Fixture::new(); f.frame(&[(8,9),(2,3)], Some(false));
    f.frame(&[(8,9),(2,9)], Some(false)); f.frame(&[(8,-4),(2,-4)], Some(true));
}
#[test] fn same_count_replacement_and_ordinal_change_require_fallback() {
    let mut f = Fixture::new(); f.frame(&[(8,9),(2,3)], Some(false));
    f.frame(&[(8,9),(5,3)], Some(false)); f.frame(&[(5,3),(8,9)], Some(false));
}
#[test] fn shrink_and_grow_require_fallback() {
    let mut f = Fixture::new(); f.frame(&[(8,9),(2,3),(5,-8)], Some(false));
    f.frame(&[(8,9),(2,3)], Some(false)); f.frame(&[(8,9),(2,3),(5,-8)], Some(false));
}
#[test] fn rear_depth_reversal_invalidates_order() {
    let mut f = Fixture::new(); f.frame(&[(8,9),(2,3),(5,-8)], Some(false));
    f.frame(&[(8,-9),(2,-3),(5,8)], Some(false));
}
#[test] fn changing_sequences_match_independent_total_order() {
    let mut f = Fixture::new(); let mut seed = 917u32;
    for frame in 0..1000 {
        let count = frame % 20;
        let mut items = Vec::new();
        for i in 0..count {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            items.push((i as u16 + ((frame / 3) % 7) as u16 * 20,
                        ((seed >> 16) as i16 as i32) / 4000));
        }
        f.frame(&items, None);
    }
}
'''

if __name__ == "__main__":
    main()
