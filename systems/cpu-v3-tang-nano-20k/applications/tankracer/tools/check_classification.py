"""Check the actual RCC coarse classifier against the original visibility rules."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

from check_blended_sort import function


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    world = Path(__file__).resolve().parents[1] / 'rcc/world.rs'
    source = world.read_text()
    functions = ['fixed', 'row', 'project', 'absolute_basis', 'extent_at', 'center_visible', 'intersects',
                 'classify_cluster']
    fixture = PRELUDE + '\n'.join(function(source, name) for name in functions)
    fixture += TESTS
    path = args.output / 'fixture.rs'
    path.write_text(fixture)
    executable = args.output / 'fixture.exe'
    subprocess.run(['rustc', '--edition=2021', '--test', str(path), '-o', str(executable)], check=True)
    result = subprocess.run([str(executable)], capture_output=True, text=True, check=True)
    print(result.stdout, end='')
    (args.output / 'result.json').write_text(json.dumps(dict(
        world_sha256=hashlib.sha256(world.read_bytes()).hexdigest(),
        fixture_sha256=hashlib.sha256(path.read_bytes()).hexdigest(), stdout=result.stdout), indent=2))


PRELUDE = r'''
#![allow(non_camel_case_types, dead_code, unused_mut)]
use std::ops::{Add, Sub, Mul, Neg, Index, IndexMut};
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct fix32(i32);
macro_rules! fix { ($v:literal) => { fix32(($v as f64 * 65536.0) as i32) }; }
impl fix32 {
    fn from_words(lo:u16, hi:u16)->Self { Self((lo as u32 | (hi as u32)<<16) as i32) }
    fn zero()->Self { Self(0) } fn one()->Self { Self(65536) }
    fn abs(self)->Self { Self(self.0.abs()) }
    fn lo_bits(self)->u16 { self.0 as u16 }
    fn hi_bits(self)->u16 { (self.0>>16) as u16 }
}
impl Add for fix32 { type Output=Self; fn add(self,b:Self)->Self { Self(self.0.checked_add(b.0).unwrap()) } }
impl Sub for fix32 { type Output=Self; fn sub(self,b:Self)->Self { Self(self.0.checked_sub(b.0).unwrap()) } }
impl Mul for fix32 { type Output=Self; fn mul(self,b:Self)->Self { Self(((self.0 as i64*b.0 as i64)>>16).try_into().unwrap()) } }
impl Neg for fix32 { type Output=Self; fn neg(self)->Self { Self(self.0.checked_neg().unwrap()) } }
#[derive(Copy,Clone)] struct Vec3 { x:fix32,y:fix32,z:fix32 }
type vec3=Vec3;
impl Vec3 { fn new(x:fix32,y:fix32,z:fix32)->Self { Self{x,y,z} } }
#[derive(Copy,Clone)] struct Vec4 { x:fix32,y:fix32,z:fix32,w:fix32 }
#[derive(Copy,Clone)] struct Mat3x4 { row0:Vec4,row1:Vec4,row2:Vec4 }
struct Mat3x3 { row0:Vec3,row1:Vec3,row2:Vec3 }
struct View { matrix:Mat3x4,absolute:Mat3x3 }
fn fdot(a:vec3,b:vec3)->fix32 {
    fix32(((a.x.0 as i64*b.x.0 as i64+a.y.0 as i64*b.y.0 as i64+a.z.0 as i64*b.z.0 as i64)>>16).try_into().unwrap())
}
#[derive(Copy,Clone)] struct Array<T> { data:*mut T, len:usize }
impl<T> Index<u16> for Array<T> {
    type Output=T;
    fn index(&self,i:u16)->&T { assert!((i as usize)<self.len); unsafe { &*self.data.add(i as usize) } }
}
impl<T> IndexMut<u16> for Array<T> {
    fn index_mut(&mut self,i:u16)->&mut T { assert!((i as usize)<self.len); unsafe { &mut *self.data.add(i as usize) } }
}
fn array(len:usize)->Array<u16> { Array { data:Box::leak(vec![0;len].into_boxed_slice()).as_mut_ptr(),len } }
const DISPLAY_DISTANCE:fix32=fix!(25);
const X_PER_DEPTH:fix32=fix!(0.78515625);
const Y_PER_DEPTH:fix32=fix!(0.47265625);
fn view(yaw:f64,pitch:f64,x:f64,y:f64,z:f64)->View {
    let f=|v:f64|fix32((v*65536.0).round() as i32);
    let a=Vec4{x:f(yaw.cos()),y:f(0.0),z:f(-yaw.sin()),w:f(x)};
    let b=Vec4{x:f(yaw.sin()*pitch.sin()),y:f(pitch.cos()),z:f(yaw.cos()*pitch.sin()),w:f(y)};
    let c=Vec4{x:f(yaw.sin()*pitch.cos()),y:f(-pitch.sin()),z:f(yaw.cos()*pitch.cos()),w:f(z)};
    View { matrix:Mat3x4{row0:a,row1:b,row2:c},absolute:Mat3x3{
        row0:absolute_basis(a),row1:absolute_basis(b),row2:absolute_basis(c)} }
}
fn oracle(view:&View,words:Array<u16>,at:u16)->u16 {
    let bound=row(words,at);
    let depth=-project(&view.matrix.row2,&bound);
    let extent=vec3::new(fixed(words,at+8),fixed(words,at+10),fixed(words,at+12));
    if !intersects(view,&bound,extent,depth) { return 0; }
    let nearest=depth-extent_at(&view.absolute.row2,extent);
    if nearest>fix32::zero() && depth+bound.w<=DISPLAY_DISTANCE
        && project(&view.matrix.row0,&bound).abs()+extent_at(&view.absolute.row0,extent)<=nearest*X_PER_DEPTH
        && project(&view.matrix.row1,&bound).abs()+extent_at(&view.absolute.row1,extent)<=nearest*Y_PER_DEPTH { return 2; }
    1
}
fn write(mut words:Array<u16>,at:u16,v:fix32) { words[at]=v.lo_bits();words[at+1]=v.hi_bits(); }
fn random(seed:&mut u32)->f64 { *seed=seed.wrapping_mul(1664525).wrapping_add(1013904223); *seed as f64/u32::MAX as f64 }
fn center_oracle(view:&View,bound:&Vec4,depth:fix32)->bool {
    let x=project(&view.matrix.row0,bound).abs();
    let y=project(&view.matrix.row1,bound).abs();
    depth>fix32::zero() && depth<=DISPLAY_DISTANCE
        && x<=depth*X_PER_DEPTH && y<=depth*Y_PER_DEPTH
}
'''

TESTS = r'''
#[test]
fn classifier_matches_original_over_motion_and_jumps() {
    let mut seed=41627;
    let data=array(128*16);
    for id in 0..128u16 {
        for axis in 0..3u16 { write(data,id*16+axis*2,fix32(((random(&mut seed)*100.0-50.0)*65536.0) as i32)); }
        let mut radius=0.0;
        for axis in 0..3u16 {
            let extent=random(&mut seed)*2.0;
            radius+=extent*extent;
            write(data,id*16+8+axis*2,fix32((extent*65536.0) as i32));
        }
        write(data,id*16+6,fix32((radius.sqrt()*65536.0).ceil() as i32));
    }
    for frame in 0..600 {
        let slow=frame as f64*0.00001;
        let v=match frame {
            200..=209 => view(3.1,0.4,2.0,4.0,-20.0),
            210..=399 => view(random(&mut seed)*6.28-3.14,random(&mut seed)*2.8-1.4,
                random(&mut seed)*80.0-40.0,random(&mut seed)*40.0-20.0,
                random(&mut seed)*80.0-40.0),
            400 => view(0.0,0.0,2049.0,0.0,0.0),
            _ => view(slow,slow*0.5,slow*10.0,slow*2.0,-slow*5.0),
        };
        for id in 0..128u16 {
            let expected=oracle(&v,data,id*16);
            assert_eq!(classify_cluster(&v,data,id*16),expected,"fresh {frame}/{id}");
            let bound=row(data,id*16);
            let depth=-project(&v.matrix.row2,&bound);
            assert_eq!(center_visible(&v,&bound,depth),center_oracle(&v,&bound,depth));
        }
    }
}
#[test]
fn clipping_boundaries_include_fixed_point_neighbors() {
    let data=array(16);
    let v=view(0.0,0.0,0.0,0.0,0.0);
    for depth in [0,1,25*65536,25*65536+1] {
        for extent in [0,1,65536] {
            for x in [-1,0,1] {
                write(data,4,fix32(-depth));
                write(data,6,fix32(extent));
                write(data,8,fix32(extent));
                write(data,10,fix32(extent));
                write(data,12,fix32(extent));
                write(data,0,fix32(((depth as i64*X_PER_DEPTH.0 as i64)>>16) as i32+x));
                assert_eq!(classify_cluster(&v,data,0),oracle(&v,data,0));
            }
        }
    }
}
#[test]
fn center_boundaries_keep_closed_side_edges_and_open_near_plane() {
    let v=view(0.0,0.0,0.0,0.0,0.0);
    for depth in [-1,0,1,65536,25*65536,25*65536+1] {
        for x_delta in [-1,0,1] {
            for y_delta in [-1,0,1] {
                let d=fix32(depth);
                let bound=Vec4{x:d*X_PER_DEPTH+fix32(x_delta),
                    y:d*Y_PER_DEPTH+fix32(y_delta),z:-d,w:fix32::zero()};
                assert_eq!(center_visible(&v,&bound,d),center_oracle(&v,&bound,d));
            }
        }
    }
}
'''

if __name__ == '__main__':
    main()
