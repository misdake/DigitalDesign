//! Flat Q12 attitude port. State words are raw signed i32 pairs:
//! reference[0..9], matrix[9..18], position[18..21], angular[21..24],
//! contact deltas[24..26], pivot[26..29]. No terrain query occurs here.
use crate::dsl_rt::*;
use crate::integer;
use crate::trig_lookup;

pub fn get(a: Array<u16>, i: u16) -> fix32 {
    fix32::load_words(a, i * 2)
}
pub fn put(a: Array<u16>, i: u16, v: fix32) {
    fix32::store_words(a, i * 2, v);
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
fn abs(v: fix32) -> fix32 {
    if v < n(0) {
        return -v;
    }
    v
}
fn length2(x: fix32, y: fix32) -> fix32 {
    let a = abs(x);
    let b = abs(y);
    if b <= a {
        return a + b - integer::sar(b, 1);
    }
    a + b - integer::sar(a, 1)
}
fn sine(table: Array<i16>, a: fix32) -> fix32 {
    trig_lookup::index(table, (a.lo_bits() & 65532) >> 2)
}
fn cosine(table: Array<i16>, a: fix32) -> fix32 {
    trig_lookup::index(table, 4096 + ((a.lo_bits() & 65532) >> 2))
}
// Returns -1 on an arithmetic failure. Callers inspect it before comparison.
pub fn angle(table: Array<i16>, num: fix32, den: fix32) -> fix32 {
    let minimum = fix32::from_words(0, 0x8000);
    if num == minimum || den == minimum {
        return n(-1);
    }
    let mut a = abs(num);
    let mut b = abs(den);
    let mut v = n(0);
    if a != n(0) {
        if b == n(0) {
            v = n(16384);
        } else if a == b {
            v = n(8192);
        } else {
            let complement = a > b;
            if complement {
                let temp = a;
                a = b;
                b = temp;
            }
            let mut numerator = a;
            let mut denominator = integer::sar(b, 8);
            if b < fix32::from_words(0, 1) {
                numerator = integer::shl(a, 15);
                denominator = integer::shl(b, 7);
            }
            if denominator == n(0) {
                return n(-1);
            }
            let index = integer::div(numerator, denominator);
            if index < n(0) || index > n(256) {
                return n(-1);
            }
            v = trig_lookup::index(table, index.lo_bits());
            if complement {
                v = n(16384) - v;
            }
        }
    }
    if den >= n(0) {
        if num < n(0) {
            v = -v;
        }
    } else {
        if num < n(0) {
            v = v - fix32::from_words(32768, 0);
        } else {
            v = fix32::from_words(32768, 0) - v;
        }
    }
    v
}

pub fn rotate(m: Array<u16>, base: u16, axis: u16, a: fix32, table: Array<i16>) {
    if a == n(0) {
        return;
    }
    let mut first = 0u16;
    let mut second = 3u16;
    if axis == 0 {
        first = 3;
        second = 6;
    }
    if axis == 1 {
        second = 6;
    }
    let c = cosine(table, a);
    let s = sine(table, -a);
    let mut i = 0u16;
    while i < 3 {
        let x = get(m, base + first + i);
        let y = get(m, base + second + i);
        let mut p = integer::mul(c, x) + integer::mul(s, y);
        let mut q = integer::mul(c, y) - integer::mul(s, x);
        if axis == 1 {
            p = integer::mul(c, x) - integer::mul(s, y);
            q = integer::mul(s, x) + integer::mul(c, y);
        }
        put(m, base + first + i, integer::trunc_shift(p, 12));
        put(m, base + second + i, integer::trunc_shift(q, 12));
        i += 1;
    }
}
fn transform(m: Array<u16>, base: u16, v: Array<u16>, vb: u16, i: u16) -> fix32 {
    integer::sar(
        integer::mul(get(m, base + i), get(v, vb))
            + integer::mul(get(m, base + 3 + i), get(v, vb + 1))
            + integer::mul(get(m, base + 6 + i), get(v, vb + 2)),
        12,
    )
}
/// Row pass, then columns of the updated matrix. Failure keeps earlier writes.
pub fn normalize(m: Array<u16>, base: u16) -> bool {
    let mut transpose = 0u16;
    while transpose < 2 {
        let mut row = 0u16;
        while row < 3 {
            let mut start = base + row * 3;
            let mut stride = 1u16;
            if transpose != 0 {
                start = base + row;
                stride = 3;
            }
            let a = get(m, start);
            let b = get(m, start + stride);
            let c = get(m, start + stride * 2);
            let sum = integer::mul(a, a) + integer::mul(b, b) + integer::mul(c, c);
            let denominator = integer::sar(sum, 13) + n(2048);
            if denominator == n(0) {
                return false;
            }
            // Rust checked_div rejects MIN/-1 as well as zero.
            let mut j = 0u16;
            while j < 3 {
                let numerator = integer::shl(get(m, start + j * stride), 12);
                if numerator == fix32::from_words(0, 0x8000) && denominator == n(-1) {
                    return false;
                }
                put(m, start + j * stride, integer::div(numerator, denominator));
                j += 1;
            }
            row += 1;
        }
        transpose += 1;
    }
    true
}

// Exact signed 64-bit multiply-add using existing 16-bit multiply windows.
// The four-limb accumulator has no hidden global scratch or ISA extension.
fn add_word(mut a: Array<u16>, at: u16, value: u16) {
    let mut i = at;
    let mut carry = value;
    while i < 4 && carry != 0 {
        let old = a[i];
        a[i] = old + carry;
        carry = (a[i] < old) as u16;
        i += 1;
    }
}
fn mac(a: Array<u16>, x: fix32, y: fix32) {
    let mut i = 0u16;
    while i < 2 {
        let mut xv = x.lo_bits();
        if i != 0 {
            xv = x.hi_bits();
        }
        let mut j = 0u16;
        while j < 2 {
            let mut yv = y.lo_bits();
            if j != 0 {
                yv = y.hi_bits();
            }
            add_word(a, i + j, xv * yv);
            add_word(a, i + j + 1, mul16(xv, yv));
            j += 1;
        }
        i += 1;
    }
    let mut hi = fix32::load_words(a, 2);
    if x < n(0) {
        hi = hi - y;
    }
    if y < n(0) {
        hi = hi - x;
    }
    fix32::store_words(a, 2, hi);
}
fn negate(mut a: Array<u16>) {
    let mut carry = 1u16;
    let mut i = 0u16;
    while i < 4 {
        let v = !a[i];
        a[i] = v + carry;
        carry = (a[i] < v) as u16;
        i += 1;
    }
}
fn rounded25(a: Array<u16>) -> fix32 {
    let negative = a[3u16] & 0x8000 != 0;
    if negative {
        negate(a);
    }
    add_word(a, 1, 256);
    let v = fix32::from_words(
        (a[1u16] >> 9) | (a[2u16] << 7),
        (a[2u16] >> 9) | (a[3u16] << 7),
    );
    if negative {
        return -v;
    }
    v
}
fn gram(m: Array<u16>, out: Array<u16>) {
    let mut row = 0u16;
    while row < 3 {
        let mut col = row;
        while col < 3 {
            let mut v = n(0);
            let mut k = 0u16;
            while k < 3 {
                v = v + integer::mul(get(m, row * 3 + k), get(m, col * 3 + k));
                k += 1;
            }
            put(out, row * 3 + col, v);
            put(out, col * 3 + row, v);
            col += 1;
        }
        row += 1;
    }
}
fn gram_bound(g: Array<u16>, bound: fix32, row_sum: bool) -> bool {
    let unit = fix32::from_words(0, 256);
    let mut r = 0u16;
    while r < 3 {
        let mut sum = n(0);
        let mut c = 0u16;
        while c < 3 {
            let mut v = get(g, r * 3 + c);
            if r == c {
                v = v - unit;
            }
            v = abs(v);
            if !row_sum && v > bound {
                return false;
            }
            sum = sum + v;
            c += 1;
        }
        if row_sum && sum > bound {
            return false;
        }
        r += 1;
    }
    true
}

// Raw Q12 matrices are small signed i16 values after the explicit guard.
// Multiplying both DOTSTORE operands by 256 keeps every Gram low bit exact.
fn raw_m8(v: fix32) -> fix32 {
    fix32::from_words(v.lo_bits() << 8, ((v.lo_bits() as i16) >> 8) as u16)
}
fn raw_m12(v: fix32) -> fix32 {
    fix32::from_words(v.lo_bits() << 12, ((v.lo_bits() as i16) >> 4) as u16)
}
fn gram_fpu(m: Array<u16>, g: Array<u16>) {
    let a = vec3::new(raw_m8(get(m, 0)), raw_m8(get(m, 1)), raw_m8(get(m, 2)));
    let b = vec3::new(raw_m8(get(m, 3)), raw_m8(get(m, 4)), raw_m8(get(m, 5)));
    let c = vec3::new(raw_m8(get(m, 6)), raw_m8(get(m, 7)), raw_m8(get(m, 8)));
    put(g, 0, fdot(a, a));
    let ab = fdot(a, b);
    put(g, 1, ab);
    put(g, 3, ab);
    let ac = fdot(a, c);
    put(g, 2, ac);
    put(g, 6, ac);
    put(g, 4, fdot(b, b));
    let bc = fdot(b, c);
    put(g, 5, bc);
    put(g, 7, bc);
    put(g, 8, fdot(c, c));
}
// r=floor(d/16), residue=d mod16. Preserve nearest rounding with ties away.
pub fn polar_round(m: fix32, r: fix32, residue: u16) -> fix32 {
    let mut base = m + integer::sar(r, 21);
    let high = r.hi_bits() & 31;
    if high > 16 || (high == 16 && r.lo_bits() != 0) {
        base = base + n(1);
    } else {
        if high == 16 && (base >= n(0) || residue & 15 != 0) {
            base = base + n(1);
        }
    }
    base
}
fn polar_fpu(a: Array<u16>, b: Array<u16>, g: Array<u16>) -> bool {
    let mut i = 0u16;
    while i < 9 {
        let v = get(a, i);
        if v < n(-8192) || v > n(8192) {
            return false;
        }
        i += 1;
    }
    gram_fpu(a, g);
    if !gram_bound(g, fix32::from_words(0, 32), true) {
        return false;
    }
    // Reuse the nine Gram slots for E=S*I-Gram after the range check.
    i = 0;
    while i < 9 {
        let mut e = -get(g, i);
        if i % 4 == 0 {
            e = e + fix32::from_words(0, 256);
        }
        put(g, i, e);
        i += 1;
    }
    let mut row = 0u16;
    while row < 3 {
        let e = vec3::new(get(g, row * 3), get(g, row * 3 + 1), get(g, row * 3 + 2));
        let mut col = 0u16;
        while col < 3 {
            let m = vec3::new(
                raw_m12(get(a, col)),
                raw_m12(get(a, 3 + col)),
                raw_m12(get(a, 6 + col)),
            );
            let r = fdot(e, m);
            let mut residue = 0u16;
            // Only negative exact half candidates need the low four product bits.
            if (r.hi_bits() & 31) == 16
                && r.lo_bits() == 0
                && get(a, row * 3 + col) + integer::sar(r, 21) < n(0)
            {
                residue = (get(g, row * 3).lo_bits() * get(a, col).lo_bits()
                    + get(g, row * 3 + 1).lo_bits() * get(a, 3 + col).lo_bits()
                    + get(g, row * 3 + 2).lo_bits() * get(a, 6 + col).lo_bits())
                    & 15;
            }
            put(
                b,
                row * 3 + col,
                polar_round(get(a, row * 3 + col), r, residue),
            );
            col += 1;
        }
        row += 1;
    }
    true
}
pub fn polar_wide(a: Array<u16>, b: Array<u16>, g: Array<u16>, w: Array<u16>) {
    let mut i = 0u16;
    while i < 9 {
        let mut k = 0u16;
        let mut j = 0u16;
        while j < 4 {
            let mut ws = w;
            ws[j] = 0;
            j += 1;
        }
        while k < 3 {
            let mut coefficient = -get(g, (i / 3) * 3 + k);
            if i / 3 == k {
                coefficient = coefficient + fix32::from_words(0, 768);
            }
            mac(w, coefficient, get(a, k * 3 + i % 3));
            k += 1;
        }
        put(b, i, rounded25(w));
        i += 1;
    }
}

// This dispatch changes no caller state and uses the existing A/B/G/w buffers.
pub fn polar_iteration(a: Array<u16>, b: Array<u16>, g: Array<u16>, w: Array<u16>) {
    if !polar_fpu(a, b, g) {
        gram(a, g);
        polar_wide(a, b, g, w);
    }
}

/// Both polar iterations and final Gram guard. Only success commits to caller.
pub fn restore(m: Array<u16>, base: u16) -> bool {
    let local: Buf<u16, 18> = Buf::new([0; 18]);
    let a = local.as_array();
    let next: Buf<u16, 18> = Buf::new([0; 18]);
    let b = next.as_array();
    let grams: Buf<u16, 18> = Buf::new([0; 18]);
    let g = grams.as_array();
    let mut wide: Buf<u16, 4> = Buf::new([0; 4]);
    let w = wide.as_array();
    let mut i = 0u16;
    while i < 9 {
        let v = get(m, base + i);
        if abs(v) > n(8192) || v == fix32::from_words(0, 0x8000) {
            return false;
        }
        put(a, i, v);
        i += 1;
    }
    gram(a, g);
    if !gram_bound(g, fix32::from_words(0, 32), true) {
        return false;
    }
    let c0 = integer::mul(get(a, 4), get(a, 8)) - integer::mul(get(a, 5), get(a, 7));
    let c1 = integer::mul(get(a, 3), get(a, 8)) - integer::mul(get(a, 5), get(a, 6));
    let c2 = integer::mul(get(a, 3), get(a, 7)) - integer::mul(get(a, 4), get(a, 6));
    mac(w, get(a, 0), c0);
    mac(w, -get(a, 1), c1);
    mac(w, get(a, 2), c2);
    if w[3u16] & 0x8000 != 0 || (w[0u16] | w[1u16] | w[2u16] | w[3u16]) == 0 {
        return false;
    }
    let mut pass = 0u16;
    while pass < 2 {
        polar_iteration(a, b, g, w);
        i = 0;
        while i < 9 {
            put(a, i, get(b, i));
            i += 1;
        }
        pass += 1;
    }
    gram(a, g);
    if !gram_bound(g, n(16384), false) {
        return false;
    }
    i = 0;
    while i < 9 {
        put(m, base + i, get(a, i));
        i += 1;
    }
    true
}

/// Returns false at the original recoverable error boundary. `events[0]`
/// records guards and normalization/correction decisions for differential replay.
pub fn step(
    state: Array<u16>,
    config: Array<u16>,
    control: Array<u16>,
    sine_table: Array<i16>,
    angle_table: Array<i16>,
    mut events: Array<u16>,
) -> bool {
    let dt = get(control, 0);
    let frame = control[2u16];
    events[0u16] = 0;
    put(
        state,
        21,
        get(state, 21)
            + integer::trunc_shift(
                integer::mul(get(state, 24), fix32::from_words(config[0u16], 0)),
                config[1u16],
            ),
    );
    put(
        state,
        23,
        get(state, 23)
            - integer::trunc_shift(
                integer::mul(get(state, 25), fix32::from_words(config[2u16], 0)),
                config[3u16],
            ),
    );
    rotate(
        state,
        9,
        1,
        integer::trunc_shift(integer::mul(get(state, 22), dt), 14),
        sine_table,
    );
    let first = angle(
        angle_table,
        abs(get(state, 1)),
        length2(get(state, 0), get(state, 2)),
    );
    if first < n(0) {
        return false;
    }
    if first > n(12740) {
        events[0u16] |= 1;
        let mut rate = get(state, 23);
        if get(state, 1) >= n(0) {
            if rate < n(0) {
                rate = integer::sar(-rate, 2);
                events[0u16] |= 2;
            }
            rate = rate + n(5000);
        } else {
            if rate > n(0) {
                rate = -integer::sar(rate, 2);
                events[0u16] |= 2;
            }
            rate = rate - n(5000);
        }
        put(state, 23, rate);
    }
    let second = angle(
        angle_table,
        abs(get(state, 7)),
        length2(get(state, 6), get(state, 8)),
    );
    if second < n(0) {
        return false;
    }
    if second > n(12740) {
        events[0u16] |= 4;
        let mut rate = get(state, 21);
        if get(state, 7) <= n(0) {
            if rate < n(0) {
                rate = -integer::trunc_shift(get(state, 23), 2);
                events[0u16] |= 8;
            }
            rate = rate + n(5000);
        } else {
            if rate > n(0) {
                rate = -integer::trunc_shift(get(state, 23), 2);
                events[0u16] |= 8;
            }
            rate = rate - n(5000);
        }
        put(state, 21, rate);
    }
    let before: Buf<u16, 6> = Buf::new([0; 6]);
    let v = before.as_array();
    let mut i = 0u16;
    while i < 3 {
        put(v, i, transform(state, 9, state, 26, i));
        i += 1;
    }
    rotate(
        state,
        9,
        0,
        integer::trunc_shift(integer::mul(get(state, 21), dt), 14),
        sine_table,
    );
    rotate(
        state,
        9,
        2,
        integer::trunc_shift(integer::mul(get(state, 23), dt), 14),
        sine_table,
    );
    i = 0;
    while i < 3 {
        put(
            state,
            18 + i,
            get(state, 18 + i) + get(v, i) - transform(state, 9, state, 26, i),
        );
        i += 1;
    }
    if frame % 10 == 0 {
        events[0u16] |= 16;
        if !normalize(state, 9) {
            return false;
        }
        if config[4u16] != 0 {
            events[0u16] |= 32;
            if !restore(state, 9) {
                return false;
            }
        }
    }
    true
}
