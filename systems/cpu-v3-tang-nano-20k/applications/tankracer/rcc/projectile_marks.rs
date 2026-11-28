//! Twenty-four terrain marks share the live quad pool and scoped terrain view.
use crate::dsl_rt::*;
use crate::{attitude, effect_pool, ground, integer, quad_pool};
pub struct State {
    // i32: count, head, next, then twenty-four raw quad handles.
    pub core: Array<u16>,
}
pub struct Mark {
    pub origin: Array<u16>,
    pub material: fix32,
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
fn g(data: Array<u16>, i: u16) -> fix32 {
    attitude::get(data, i)
}
fn p(data: Array<u16>, i: u16, value: fix32) {
    attitude::put(data, i, value);
}
pub fn add(
    state: &State,
    mark: &Mark,
    map: &mut ground::World,
    quads: &mut quad_pool::Pool,
    lines: &effect_pool::Pool,
) -> fix32 {
    let x0 = g(mark.origin, 0) - n(1024);
    let x1 = g(mark.origin, 0) + n(1024);
    let z0 = g(mark.origin, 2) - n(1024);
    let z1 = g(mark.origin, 2) + n(1024);
    let mut a = ground::zero_hit();
    let mut b = ground::zero_hit();
    let first = map.query(x0, z0, &mut a);
    if first > 1 {
        return n(-2);
    }
    if first == 0 || a.flags.lo_bits() & 16 != 0 {
        return n(0);
    }
    let second = map.query(x1, z1, &mut b);
    if second > 1 {
        return n(-2);
    }
    if second == 0 || b.flags.lo_bits() & 16 != 0 {
        return n(0);
    }
    let mid = integer::trunc_shift(a.height + b.height, 1);
    let s = state.core;
    if g(s, 0) == n(24) {
        let head = g(s, 1).lo_bits();
        if quad_pool::release(quads, lines, g(s, 3 + head)) != 0 {
            return n(-1);
        }
        p(s, 1, fix32::from_words((head + 1) % 24, 0));
        p(s, 0, g(s, 0) - n(1));
    }
    let quad: Buf<u16, 32> = Buf::new([0; 32]);
    let q = quad.as_array();
    p(q, 0, x0);
    p(q, 1, a.height);
    p(q, 2, z0);
    p(q, 3, x1);
    p(q, 4, mid);
    p(q, 5, z0);
    p(q, 6, x1);
    p(q, 7, b.height);
    p(q, 8, z1);
    p(q, 9, x0);
    p(q, 10, mid);
    p(q, 11, z1);
    p(q, 12, mark.material);
    let mut style = q;
    style[26u16] = 33023;
    style[27u16] = 32896;
    style[28u16] = 257;
    style[29u16] = 319;
    style[30u16] = 16191;
    style[31u16] = 16129;
    let handle = quad_pool::allocate(quads, lines, q);
    if handle != n(-1) {
        let next = g(s, 2).lo_bits();
        p(s, 3 + next, handle);
        p(s, 2, fix32::from_words((next + 1) % 24, 0));
        p(s, 0, g(s, 0) + n(1));
    }
    n(0)
}
