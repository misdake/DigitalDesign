//! rcc memory: Ptr operations, arrays (local/global), addr_of, consts,
//! statics and __data_init, memory content verification.

mod common;

use common::*;

#[test]
fn test_arrays_dsl_file() {
    let src = include_str!("../src/dsl_progs/arrays_dsl.rs");
    let (_state, signal) = compile_and_run(src, "main", 4000);
    // TILE[1] = 0xc3ff lands in grid[16], then in total, then in SCORE
    assert_eq!(signal, Some(0xc3ff));
}

#[test]
fn test_const_and_static() {
    let src = r#"
const WIDTH: u16 = 8;
const DOUBLE: u16 = WIDTH * 2;
static SCORE: u16 = 0;
static TILE: [u16; 3] = [11, 22, 33];
fn main() {
    // statics are initialized by __data_init at main entry
    assert(TILE.read(0) == 11 && TILE.read(2) == 33, 1);
    assert(DOUBLE == 16, 2);
    // writing through the address of a global
    let s = addr_of(&SCORE);
    s.write(0, TILE.read(1));
    halt(SCORE);
}
"#;
    let (state, signal, listing) =
        compile_program_and_run(src, &cpu_v2::CompilerOptions::default(), 10_000);
    assert_eq!(signal, Some(22));
    assert_eq!(state.mem[1], 11);
    assert!(listing.contains("global init: static data"), "{listing}");
}

#[test]
fn test_addr_of_local() {
    let src = r#"
fn bump(p: Ptr) {
    let v = p.read(0);
    p.write(0, v + 1);
}
fn main() {
    let mut x: u16 = 41;
    bump(addr_of(&x));
    bump(addr_of(&x));
    halt(x);
}
"#;
    assert_eq!(run(src), Some(43));
}

#[test]
fn test_local_array_as_param() {
    let src = r#"
fn fill(buf: Ptr, n: u16, v: u16) {
    let mut i: u16 = 0;
    while i < n {
        buf.add(i as i16).write(0, v);
        i += 1;
    }
}
fn sum(buf: Ptr, n: u16) -> u16 {
    let mut s: u16 = 0;
    let mut i: u16 = 0;
    while i < n {
        s += buf.add(i as i16).read(0);
        i += 1;
    }
    s
}
fn main() {
    let mut a: [u16; 10] = [0; 10];
    fill(a.as_ptr(), 10, 4);
    halt(sum(a.as_ptr(), 10));
}
"#;
    assert_eq!(run(src), Some(40));
}

#[test]
fn test_addr_of_param() {
    let src = r#"
fn set1(p: Ptr) {
    p.write(0, 1);
}
fn choose(x: u16, y: u16) -> u16 {
    let mut z: u16 = x;
    if y != 0 {
        set1(addr_of(&z));
    }
    z
}
fn main() {
    let a = choose(7, 1);
    let b = choose(7, 0);
    halt((a << 4) + b);
}
"#;
    assert_eq!(run(src), Some((1 << 4) + 7));
}

#[test]
fn test_ptr_ops_full() {
    let src = r#"
fn main() {
    // a scratch area high enough to clear the data section (no statics here)
    let base = Ptr::from_addr(0x100);
    base.write(0, 11);
    base.write(1, 22);
    base.write(2, 33);
    // addr() feeds back into from_addr
    let end = Ptr::from_addr(base.addr() + 2);
    let a = end.read(0); // base[2]
    // add() with a literal and with a (negative) i16 variable
    let b = base.add(1).read(0);
    let back: i16 = -1;
    let c = end.add(back).read(0); // base[1]
    // write through an offset pointer
    end.write(0, a + b);
    halt(end.read(0) + c);
}
"#;
    // host-side simulation of the same word accesses
    let mut mem = [0u16; 3];
    mem[0] = 11;
    mem[1] = 22;
    mem[2] = 33;
    let a = mem[2];
    let b = mem[1];
    let c = mem[1];
    mem[2] = a + b;
    assert_eq!(run(src), Some(mem[2] + c));
}

#[test]
fn test_ptr_from_addr_roundtrip() {
    let src = r#"
static G: [u16; 3] = [111, 222, 333];
fn main() {
    let p = G.as_ptr();
    // from_addr(addr(p)) rebuilds the same pointer
    let q = Ptr::from_addr(p.addr());
    // from_addr on a computed address lands on element 2
    let r = Ptr::from_addr(G.as_ptr().addr() + 2);
    halt(q.read(0) + q.read(1) + r.read(0));
}
"#;
    let g = [111u16, 222, 333];
    assert_eq!(run(src), Some(g[0] + g[1] + g[2]));
}

#[test]
fn test_local_array_init_forms() {
    let src = r#"
fn main() {
    let mut z: [u16; 5] = [7; 5]; // repeat form
    let l: [u16; 4] = [1, 2, 4, 8]; // list form
    z.write(2, z.read(0) + l.read(3));
    let mut sum: u16 = 0;
    let mut i: u16 = 0;
    while i < 5 {
        sum += z.read(i);
        i += 1;
    }
    i = 0;
    while i < 4 {
        sum += l.read(i);
        i += 1;
    }
    halt(sum);
}
"#;
    let mut z = [7u16; 5];
    let l = [1u16, 2, 4, 8];
    z[2] = z[0] + l[3];
    let expected: u16 = z.iter().sum::<u16>() + l.iter().sum::<u16>();
    assert_eq!(run(src), Some(expected));
}

#[test]
fn test_as_ptr_aliases_array() {
    let src = r#"
fn main() {
    let mut a: [u16; 4] = [1, 2, 3, 4];
    let p = a.as_ptr();
    // raw pointer writes land in the array's frame slots
    p.write(0, p.read(3) + 10);
    p.add(1).write(1, p.read(0)); // a[2] = a[0]
    halt(a.read(0) + a.read(1) + a.read(2) + a.read(3));
}
"#;
    let mut a = [1u16, 2, 3, 4];
    a[0] = a[3] + 10;
    a[2] = a[0];
    assert_eq!(run(src), Some(a.iter().sum()));
}

#[test]
fn test_array_len_mismatch() {
    expect_error(
        "fn main() { let a: [u16; 3] = [1, 2]; }",
        "array initializer has 2 elements, expected 3",
    );
    expect_error(
        "fn main() { let a: [u16; 2] = [1, 2, 3]; }",
        "array initializer has 3 elements, expected 2",
    );
    expect_error(
        "static A: [u16; 3] = [1, 2]; fn main() {}",
        "initializer has 2 elements, expected 3",
    );
}

#[test]
fn test_data_init_layout() {
    // globals are laid out from data address 0 in declaration order;
    // __data_init stores each non-zero word at main entry
    let src = r#"
static A: u16 = 5;                    // addr 0
static B: [u16; 4] = [10, 0, 30, 40]; // addr 1..=4 (B[1] is zero: nothing stored)
static C: i16 = -2;                   // addr 5 (raw sign bits)
static D: [u16; 3] = [0; 3];          // addr 6..=8 (all zero: nothing stored)
fn main() {
    addr_of(&A).write(0, A + 1);
    halt(0);
}
"#;
    let mut expected = [0u16; 9];
    expected[0] = 5 + 1; // written through addr_of(&A)
    expected[1] = 10;
    expected[3] = 30;
    expected[4] = 40;
    expected[5] = (-2i16) as u16;
    let (state, signal) = compile_and_run(src, "main", 10_000);
    assert_eq!(signal, Some(0));
    assert_eq!(&state.mem[..9], &expected[..]);
}

#[test]
fn test_data_init_uses_sp_across_pages_and_restores_it() {
    let src = r#"
static DATA: [u16; 4] = [11, 22, 33, 44];
fn main() {
    halt(DATA.read(0) + DATA.read(1) + DATA.read(2) + DATA.read(3));
}
"#;
    let opts = cpu_v2::CompilerOptions {
        data_base: 0x00fe,
        stack_init: 0x9000,
        ..Default::default()
    };
    let (state, signal, listing) = compile_program_and_run(src, &opts, 10_000);

    assert_eq!(signal, Some(110));
    assert_eq!(&state.mem[0x00fe..=0x0101], &[11, 22, 33, 44]);
    assert_eq!(state.reg[cpu_v2::SP_REG as usize], 0x9000);
    for offset in [0xfe, 0xff, 0x00, 0x01] {
        assert!(
            listing.contains(&format!("mem[r14 + 0x{offset:02x}] = r15")),
            "{listing}"
        );
    }
}

#[test]
fn test_static_scalar_read_write() {
    let src = r#"
static SCORE: u16 = 100;
static TICK: i16 = -7;
fn main() {
    // reading a static loads the word; writing goes through its address
    addr_of(&SCORE).write(0, SCORE + 23);
    let t: i16 = TICK + 3;
    addr_of(&TICK).write(0, t as u16);
    assert(TICK == -4, 1);
    halt(SCORE);
}
"#;
    assert_eq!(run(src), Some(100 + 23));
}

#[test]
fn test_const_arithmetic() {
    let src = r#"
const N: u16 = 6;
const A: u16 = 40 + 2;
const B: u16 = A << 1;
const C: u16 = B - A;
const D: u16 = C | 0x0F;
const E: u16 = D & 0x3C;
const F: i16 = -5;
const G: i16 = F - F - F;
fn main() {
    // a const can size a local array
    let buf: [u16; N] = [0; N];
    assert(buf.len() == N, 1);
    halt(A + B + C + D + E + G as u16);
}
"#;
    let (a, f) = (40u16 + 2, -5i16);
    let b = a << 1;
    let c = b - a;
    let d = c | 0x0F;
    let e = d & 0x3C;
    let g = f - f - f;
    assert_eq!(run(src), Some(a + b + c + d + e + g as u16));
}

#[test]
fn test_addr_of_local_swap() {
    let src = r#"
fn swap(a: Ptr, b: Ptr) {
    let t = a.read(0);
    a.write(0, b.read(0));
    b.write(0, t);
}
fn main() {
    let mut x: u16 = 3;
    let mut y: u16 = 8;
    swap(addr_of(&x), addr_of(&y));
    // both locals were written back through pointers
    halt((x << 4) + y);
}
"#;
    let (mut x, mut y) = (3u16, 8u16);
    std::mem::swap(&mut x, &mut y);
    assert_eq!(run(src), Some((x << 4) + y));
}

#[test]
fn test_addr_of_param_copy() {
    let src = r#"
fn bump_twice(v: u16) -> u16 {
    // an address-taken param is copied into the callee's frame at entry
    let p = addr_of(&v);
    p.write(0, p.read(0) + 1);
    p.write(0, p.read(0) + 1);
    v
}
fn main() {
    let a: u16 = 10;
    let r = bump_twice(a);
    // only the callee's copy changed; the caller's variable is untouched
    assert(a == 10, 1);
    halt(r + a);
}
"#;
    assert_eq!(run(src), Some(10 + 2 + 10));
}

#[test]
fn test_array_as_ptr_and_len() {
    let src = r#"
static KEYS: [u16; 5] = [3, 1, 4, 1, 5];
fn sum_at(p: Ptr, n: u16) -> u16 {
    let mut s: u16 = 0;
    let mut i: u16 = 0;
    while i < n {
        s += p.add(i as i16).read(0);
        i += 1;
    }
    s
}
fn main() {
    let local: [u16; 3] = [9, 2, 6];
    // the same helper walks a global array and a stack array
    let g = sum_at(KEYS.as_ptr(), KEYS.len());
    let l = sum_at(local.as_ptr(), local.len());
    halt((g << 4) + l);
}
"#;
    let keys = [3u16, 1, 4, 1, 5];
    let local = [9u16, 2, 6];
    let expected = (keys.iter().sum::<u16>() << 4) + local.iter().sum::<u16>();
    assert_eq!(run(src), Some(expected));
}

#[test]
fn test_i16_array_elements() {
    let src = r#"
static NEGS: [i16; 4] = [-1, -2, -3, -4];
fn main() {
    let mut local: [i16; 3] = [0; 3];
    // i16 elements are raw words; casts make the signedness explicit
    local.write(0, (-10i16) as u16);
    local.write(1, 20);
    local.write(2, (-30i16) as u16);
    let mut sum: i16 = 0;
    let mut i: u16 = 0;
    while i < local.len() {
        sum += local.read(i) as i16;
        i += 1;
    }
    let mut gsum: i16 = 0;
    i = 0;
    while i < NEGS.len() {
        gsum += NEGS.read(i) as i16;
        i += 1;
    }
    // signed comparison between elements
    let a = local.read(0) as i16;
    let b = local.read(2) as i16;
    let min = if a < b { a } else { b };
    assert(min == -30, 1);
    halt((sum + gsum + min) as u16);
}
"#;
    let local: [i16; 3] = [-10, 20, -30];
    let negs: [i16; 4] = [-1, -2, -3, -4];
    let expected =
        (local.iter().sum::<i16>() + negs.iter().sum::<i16>() + local[0].min(local[2])) as u16;
    assert_eq!(run(src), Some(expected));
}

#[test]
fn test_bubble_sort_matches_rust() {
    let src = r#"
static DATA: [u16; 8] = [50, 20, 90, 10, 70, 30, 80, 60];
fn sort(p: Ptr, n: u16) {
    let mut i: u16 = 0;
    while i < n {
        let mut j: u16 = 0;
        while j + 1 < n - i {
            let a = p.add(j as i16);
            let b = a.add(1);
            if a.read(0) > b.read(0) {
                let t = a.read(0);
                a.write(0, b.read(0));
                b.write(0, t);
            }
            j += 1;
        }
        i += 1;
    }
}
fn main() {
    sort(DATA.as_ptr(), DATA.len());
    halt(DATA.read(0) + DATA.read(DATA.len() - 1));
}
"#;
    let mut reference = [50u16, 20, 90, 10, 70, 30, 80, 60];
    reference.sort_unstable();
    let (state, signal) = compile_and_run(src, "main", 10_000);
    assert_eq!(signal, Some(reference[0] + reference[7]));
    // DATA is the only global, so it occupies data addresses 0..8
    assert_eq!(&state.mem[..8], &reference[..]);
}

#[test]
fn test_fib_local_array_matches_rust() {
    let src = r#"
fn main() {
    let mut fib: [u16; 10] = [0; 10];
    fib.write(0, 1);
    fib.write(1, 1);
    let mut i: u16 = 2;
    while i < fib.len() {
        fib.write(i, fib.read(i - 1) + fib.read(i - 2));
        i += 1;
    }
    halt(fib.read(9) + fib.read(0));
}
"#;
    let mut fib = [0u16; 10];
    fib[0] = 1;
    fib[1] = 1;
    for i in 2..10 {
        fib[i] = fib[i - 1] + fib[i - 2];
    }
    assert_eq!(run(src), Some(fib[9] + fib[0]));
}
