//! Synchronous impact-ring generation and animation, sharing the world's RNG.
use crate::dsl_rt::*;
use crate::{attitude, collision, effect_pool, integer, random, trig_lookup};
pub struct Particle {
    pub distance: Buf<i16, 2>,
    pub speed: Buf<u16, 4>,
    pub pitch: Buf<u16, 4>,
    pub yaw: Buf<u16, 4>,
    pub line: fix32,
    pub colors: Buf<u16, 4>,
}
pub struct Batch {
    pub particles: Buf<Particle, 10>,
    pub kind: fix32,
    pub corner: fix32,
    pub owner: fix32,
    pub active: bool,
    pub age: fix32,
    pub frame: fix32,
}
pub struct State {
    pub batches: Array<Batch>,
    // 65535 uses resident rows. A banked stage selects DSEG2 for the complete
    // operation; Vehicle/Context views must stay resident outside that window.
    pub page: u16,
    pub next: fix32,
}
pub struct Vehicle {
    pub entity: fix32,
    pub cooldown: Array<u16>,
    pub contacts: Array<u16>,
    pub contact_stride: u16,
    pub forward: fix32,
    pub basis: Array<u16>,
    pub has_basis: bool,
}
pub struct Context {
    pub frame: fix32,
    pub seed: Array<u16>,
    pub pool: effect_pool::Pool,
    pub sine: Array<i16>,
    pub angle: Array<i16>,
    pub forward_offset: Array<u16>,
    pub reverse_offset: Array<u16>,
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
fn g(v: Array<u16>, i: u16) -> fix32 {
    attitude::get(v, i)
}
fn p(v: Array<u16>, i: u16, x: fix32) {
    attitude::put(v, i, x);
}
pub fn particle_zero() -> Particle {
    Particle {
        distance: Buf::new([0; 2]),
        speed: Buf::new([0; 4]),
        pitch: Buf::new([0; 4]),
        yaw: Buf::new([0; 4]),
        line: n(0),
        colors: Buf::new([0; 4]),
    }
}
pub fn batch_zero() -> Batch {
    Batch {
        particles: Buf::new([
            particle_zero(),
            particle_zero(),
            particle_zero(),
            particle_zero(),
            particle_zero(),
            particle_zero(),
            particle_zero(),
            particle_zero(),
            particle_zero(),
            particle_zero(),
        ]),
        kind: n(0),
        corner: n(0),
        owner: n(-1),
        active: false,
        age: n(0),
        frame: n(0),
    }
}
fn shifted(kind: fix32) -> bool {
    return kind == n(1) || kind == n(2) || kind == n(5) || kind == n(6);
}
fn offset(v: &Vehicle, c: &Context, out: Array<u16>) {
    let mut i = 0u16;
    while i < 3 {
        p(out, i, n(0));
        i += 1;
    }
    if v.has_basis {
        let mut source = c.forward_offset;
        if v.forward < n(0) {
            source = c.reverse_offset;
        }
        collision::transform(out, v.basis, source, false);
    }
}
// Angles: shared sy/cy/sp/cp. Radius products retain each wrapping/truncation.
fn angles(out: Array<u16>, yaw: fix32, pitch: fix32, sine: Array<i16>) {
    let y = (yaw.lo_bits() & 65532) >> 2;
    let a = (pitch.lo_bits() & 65532) >> 2;
    p(out, 0, trig_lookup::index(sine, y));
    p(out, 1, trig_lookup::index(sine, y + 4096));
    p(out, 2, trig_lookup::index(sine, a));
    p(out, 3, trig_lookup::index(sine, a + 4096));
}
fn endpoint(out: Array<u16>, r: fix32, a: Array<u16>, origin: Array<u16>, offset: Array<u16>) {
    let x = integer::trunc_shift(-fmul_lo32(r, g(a, 0)), 12);
    let z = integer::trunc_shift(fmul_lo32(r, g(a, 1)), 12);
    p(out, 0, x + g(origin, 0) + g(offset, 0));
    p(
        out,
        1,
        integer::trunc_shift(fmul_lo32(z, g(a, 2)), 12) + g(origin, 1) + g(offset, 1),
    );
    p(
        out,
        2,
        integer::trunc_shift(fmul_lo32(z, g(a, 3)), 12) + g(origin, 2) + g(offset, 2),
    );
}
pub fn select(state: &State) -> u16 {
    let saved = read_dseg_page(2);
    if state.page != 65535 && state.page != saved {
        unsafe {
            write_dseg_page(2, state.page);
        }
    }
    saved
}
// Status: 0 skipped, 1 emitted, 2 arithmetic, 3 pool, 4 invalid ring/corner/basis.
pub fn emit(
    state: &mut State,
    v: &mut Vehicle,
    corner: fix32,
    kind: fix32,
    c: &mut Context,
) -> u16 {
    let saved = select(state);
    let status = emit_selected(state, v, corner, kind, c);
    effect_pool::restore(saved);
    status
}
fn emit_selected(
    state: &mut State,
    v: &mut Vehicle,
    corner: fix32,
    kind: fix32,
    c: &mut Context,
) -> u16 {
    // cooldown is u32 in the source.
    let cooldown = g(v.cooldown, 0);
    if kind != n(3) && kind != n(4) && cooldown.hi_bits() == 0 && cooldown.lo_bits() < 500 {
        return 0;
    }
    if state.next.hi_bits() != 0 || state.next.lo_bits() >= 4 || kind < n(0) || kind > n(6) {
        return 4;
    }
    let batch = &mut state.batches[state.next.lo_bits()];
    let mut i = 0u16;
    if batch.active {
        while i < 10 {
            let handle = batch.particles[i].line;
            if handle != effect_pool::released() && effect_pool::release(&mut c.pool, handle) != 0 {
                return 3;
            }
            i += 1;
        }
    }
    let mut angle = n(0);
    let mut spread = 65520u16;
    if kind != n(0) {
        let mut first = 2u16;
        if kind == n(1) || kind == n(3) || kind == n(5) {
            first = 0;
        }
        let mut x =
            g(v.contacts, (first + 1) * v.contact_stride) - g(v.contacts, first * v.contact_stride);
        let mut z = g(v.contacts, (first + 1) * v.contact_stride + 2)
            - g(v.contacts, first * v.contact_stride + 2);
        if v.forward < n(0) {
            x = -x;
            z = -z;
        }
        let value = attitude::angle(c.angle, x, z);
        if value == n(-1) {
            return 2;
        }
        angle = -value;
        if kind == n(3) || kind == n(4) {
            p(v.cooldown, 0, n(0));
        }
        spread = 5460;
        if kind == n(5) || kind == n(6) {
            spread = 9100;
        }
    }
    let displacement: Buf<u16, 6> = Buf::new([0; 6]);
    let off = displacement.as_array();
    if shifted(kind) {
        offset(v, c, off);
    }
    if corner.hi_bits() != 0 || corner.lo_bits() >= 4 {
        return 4;
    }
    let origin = unsafe {
        v.contacts
            .as_ptr()
            .add((corner.lo_bits() * v.contact_stride * 2) as i16)
            .as_u16_array()
    };
    let scratch: Buf<u16, 20> = Buf::new([0; 20]);
    let a = scratch.as_array();
    let points = unsafe { a.as_ptr().add(8).as_u16_array() };
    let far = unsafe { points.as_ptr().add(6).as_u16_array() };
    i = 0;
    while i < 10 {
        p(c.seed, 0, random::next(g(c.seed, 0)));
        let r0 = integer::sar(g(c.seed, 0), 4).lo_bits();
        p(c.seed, 0, random::next(g(c.seed, 0)));
        let r1 = integer::sar(g(c.seed, 0), 4).lo_bits();
        let speed = integer::shl(fix32::from_words(r0 % 55 + 10, 0), 8);
        let pitch = fix32::from_words(r0 % spread, 0) - fix32::from_words(spread >> 1, 0);
        let yaw = fix32::from_words(r1 % spread, 0) + angle - fix32::from_words(spread >> 1, 0);
        let particle = &mut batch.particles[i];
        particle.distance[0u16] = 1000;
        particle.distance[1u16] = 1800;
        p(particle.speed.as_array(), 0, speed);
        p(particle.speed.as_array(), 1, speed);
        p(particle.pitch.as_array(), 0, pitch);
        p(particle.pitch.as_array(), 1, pitch);
        p(particle.yaw.as_array(), 0, yaw);
        p(particle.yaw.as_array(), 1, yaw);
        let green = (r0 % 126 - 126) & 255;
        particle.colors[0u16] = 0;
        particle.colors[1u16] = 0;
        particle.colors[2u16] = 255 | (green << 8);
        particle.colors[3u16] = 0;
        angles(a, yaw, pitch, c.sine);
        endpoint(points, n(1000), a, origin, off);
        endpoint(far, n(1800), a, origin, off);
        let colors: Buf<u16, 4> = Buf::new([0; 4]);
        let mut color = colors.as_array();
        color[1u16] = 256;
        color[2u16] = 255 | (green << 8);
        particle.line = effect_pool::allocate(&mut c.pool, points, color);
        i += 1;
    }
    batch.owner = v.entity;
    batch.active = true;
    batch.corner = corner;
    batch.frame = c.frame;
    batch.age = n(0);
    batch.kind = kind;
    state.next = state.next + n(1);
    if state.next == n(4) {
        state.next = n(0);
    }
    1
}
pub fn tick(state: &mut State, v: &mut Vehicle, dt: fix32, c: &mut Context) -> u16 {
    let saved = select(state);
    let status = tick_selected(state, v, dt, c);
    effect_pool::restore(saved);
    status
}
fn tick_selected(state: &mut State, v: &mut Vehicle, dt: fix32, c: &mut Context) -> u16 {
    p(v.cooldown, 0, g(v.cooldown, 0) + dt);
    let scratch: Buf<u16, 34> = Buf::new([0; 34]);
    let a = scratch.as_array();
    let b = unsafe { a.as_ptr().add(8).as_u16_array() };
    let points = unsafe { a.as_ptr().add(16).as_u16_array() };
    let far = unsafe { a.as_ptr().add(22).as_u16_array() };
    let off = unsafe { a.as_ptr().add(28).as_u16_array() };
    let mut index = 0u16;
    while index < 4 {
        let batch = &mut state.batches[index];
        if batch.owner == v.entity && batch.active {
            if batch.frame == c.frame {
                return 0;
            }
            if batch.corner.hi_bits() != 0 || batch.corner.lo_bits() >= 4 {
                return 4;
            }
            let origin = unsafe {
                v.contacts
                    .as_ptr()
                    .add((batch.corner.lo_bits() * v.contact_stride * 2) as i16)
                    .as_u16_array()
            };
            let mut offset_ready = false;
            let mut i = 0u16;
            while i < 10 {
                let particle = &mut batch.particles[i];
                particle.distance[1u16] +=
                    integer::sar(fmul_lo32(g(particle.speed.as_array(), 1), dt), 8).lo_bits()
                        as i16;
                particle.distance[0u16] +=
                    integer::sar(fmul_lo32(g(particle.speed.as_array(), 0), dt), 8).lo_bits()
                        as i16;
                angles(
                    a,
                    g(particle.yaw.as_array(), 0),
                    g(particle.pitch.as_array(), 0),
                    c.sine,
                );
                let mut other = a;
                if g(particle.yaw.as_array(), 0) != g(particle.yaw.as_array(), 1)
                    || g(particle.pitch.as_array(), 0) != g(particle.pitch.as_array(), 1)
                {
                    angles(
                        b,
                        g(particle.yaw.as_array(), 1),
                        g(particle.pitch.as_array(), 1),
                        c.sine,
                    );
                    other = b;
                }
                let zero: Buf<u16, 6> = Buf::new([0; 6]);
                endpoint(
                    points,
                    n(particle.distance[0u16]),
                    a,
                    origin,
                    zero.as_array(),
                );
                endpoint(
                    far,
                    n(particle.distance[1u16]),
                    other,
                    origin,
                    zero.as_array(),
                );
                if shifted(batch.kind) {
                    if !v.has_basis {
                        return 4;
                    }
                    if !offset_ready {
                        offset(v, c, off);
                        offset_ready = true;
                    }
                    let mut j = 0u16;
                    while j < 6 {
                        p(points, j, g(points, j) + g(off, j % 3));
                        j += 1;
                    }
                }
                let colors: Buf<u16, 4> = Buf::new([0; 4]);
                let mut color = colors.as_array();
                color[0u16] = particle.colors[0u16];
                color[1u16] = (particle.colors[1u16] & 255) | 256;
                color[2u16] = particle.colors[2u16];
                color[3u16] = particle.colors[3u16] & 255;
                if effect_pool::update(&mut c.pool, particle.line, points, color) != 0 {
                    return 3;
                }
                i += 1;
            }
            batch.age = batch.age + dt;
            if batch.age > n(60) {
                i = 0;
                while i < 10 {
                    let particle = &mut batch.particles[i];
                    if particle.line != effect_pool::released() {
                        if effect_pool::release(&mut c.pool, particle.line) != 0 {
                            return 3;
                        }
                        particle.line = effect_pool::released();
                    }
                    i += 1;
                }
                batch.active = false;
                batch.age = n(0);
            }
        }
        index += 1;
    }
    0
}
