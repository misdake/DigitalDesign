//! rcc standard library tests: heap (malloc/free + auto init), mem, mul, div, vec.

mod common;

use common::*;
use cpu_v2::CompilerOptions;

#[test]
fn test_mul() {
    let src = r#"
fn main() {
    let x = 37;
    let y = 1111;
    halt(mul_16x16(x, y));
}
"#;
    let opts = CompilerOptions::default();
    let (_, signal, _) = compile_program_and_run(src, &opts, 4000);
    assert_eq!(signal, Some(37u16.wrapping_mul(1111)));
}

#[test]
fn test_mul_operator_uses_the_library_fallback() {
    // CpuV2 has no hardware multiply: `*` lowers to the rcc_std mul_16x16 call.
    let src = r#"
fn main() {
    let mut acc: u16 = 0;
    let mut i: u16 = 0;
    while i < 8 {
        acc = acc + 37 * 11 + i * 3;
        i = i + 1;
    }
    halt(acc);
}
"#;
    let opts = CompilerOptions::default();
    let (_, signal, _) = compile_program_and_run(src, &opts, 8000);
    assert_eq!(signal, Some(3340));
}

fn heap_stat(mem: &[u16], begin: usize, end: usize) -> (usize, usize) {
    let mut alloc_count = 0;
    let mut alloc_size = 0;
    let mut sum = 0;
    let mut ptr = begin;
    while ptr < end {
        let flag = mem[ptr];
        if flag > (1 << 15) {
            let size = flag - (1 << 15);
            sum += size;
            assert_eq!(flag, mem[ptr + size as usize - 1]);
            ptr += size as usize;
        } else {
            let size = flag;
            sum += size;
            assert_eq!(flag, mem[ptr + size as usize - 1]);
            ptr += size as usize;
            alloc_count += 1;
            alloc_size += (size - 2) as usize;
        }
    }
    assert_eq!(sum as usize, end - begin, "heap corruption");
    (alloc_count, alloc_size)
}

#[test]
fn test_malloc_free_and_layout() {
    let src = r#"
fn main() {
    let ptr1 = malloc(1);
    let ptr2 = malloc(2);
    let _ptr3 = malloc(3);
    free(ptr2);
    free(ptr1);
    let ptr4 = malloc(2);
    let ptr5 = malloc(5);
    mem_set(ptr4, 2, 44);
    mem_set(ptr5, 5, 55);
    mem_copy(ptr5, ptr4, 2);
    halt(0);
}
"#;
    let opts = CompilerOptions::default();
    let (state, _signal, _) = compile_program_and_run(src, &opts, 4000);
    let (count, size) = heap_stat(
        state.mem.as_slice(),
        opts.heap_begin as usize,
        (opts.heap_begin + opts.heap_size) as usize,
    );
    assert_eq!(count, 3);
    assert_eq!(size, 11);
    // exact layout (same boundary-tag algorithm as the old embedded-DSL heap)
    assert_eq!(
        &state.mem[opts.heap_begin as usize..(opts.heap_begin + opts.heap_size) as usize],
        [4, 44, 44, 4, 32771, 0, 32771, 5, 0, 0, 0, 5, 8, 44, 44, 55, 55, 55, 0, 8]
    );
}

#[test]
fn test_heap_custom_region() {
    let src = r#"
fn main() {
    let p = malloc(4);
    p.write(0, 77);
    halt(p.read(0));
}
"#;
    let opts = CompilerOptions {
        heap_begin: 0x2000,
        heap_size: 16,
        ..CompilerOptions::default()
    };
    let (state, signal, _) = compile_program_and_run(src, &opts, 2000);
    assert_eq!(signal, Some(77));
    // malloc(4) took a 6-word block (4 content + 2 tags) at the region start
    assert_eq!(state.mem[opts.heap_begin as usize], 6);
}

#[test]
fn test_vec_basic() {
    let src = r#"
fn main() {
    let v = vec_new();
    assert(vec_len(v) == 0, 10);
    assert(vec_cap(v) == 4, 11); // vec_init_cap default = 4
    vec_push(v, 12);
    vec_push(v, 34);
    assert(vec_len(v) == 2, 20);
    assert(vec_get(v, 1) == 34, 21);
    vec_push(v, 56);
    vec_push(v, 78);
    vec_push(v, 90); // grows beyond capacity 4
    assert(vec_len(v) == 5, 30);
    assert(vec_get(v, 4) == 90, 31);
    let x = vec_pop(v);
    assert(x == 90, 40);
    assert(vec_len(v) == 4, 41);
    vec_free(v);
    halt(0);
}
"#;
    let opts = CompilerOptions {
        heap_size: 64,
        ..CompilerOptions::default()
    };
    let (state, signal, _) = compile_program_and_run(src, &opts, 8000);
    assert_eq!(
        signal,
        Some(0),
        "heap: {:?}",
        &state.mem[opts.heap_begin as usize..(opts.heap_begin + 20) as usize]
    );
}

#[test]
fn test_div_and_rem_unsigned() {
    // a variable divisor calls the rcc_std shift-subtract routine
    let src = r#"
fn main() {
    let mut acc: u16 = 0;
    let mut i: u16 = 1;
    while i <= 40 {
        acc = acc + (1000 / i) + (1000 % i);
        i = i + 1;
    }
    halt(acc);
}
"#;
    let opts = CompilerOptions::default();
    let (_, signal, _) = compile_program_and_run(src, &opts, 40_000);
    let mut expected: u16 = 0;
    for i in 1u16..=40 {
        expected = expected.wrapping_add(1000 / i).wrapping_add(1000 % i);
    }
    assert_eq!(signal, Some(expected));
}

#[test]
fn test_div_and_rem_power_of_two_literal_uses_shift_and_mask() {
    // a literal power-of-two divisor lowers to shift/mask, not a call
    let src = r#"
fn main() {
    let x: u16 = 12345;
    let a = x / 8u16;
    let b = x % 8u16;
    let c = x / 1u16;
    halt(a + b + c);
}
"#;
    let opts = CompilerOptions::default();
    let (_, signal, listing) = compile_program_and_run(src, &opts, 2000);
    assert_eq!(signal, Some(12345 / 8 + 12345 % 8 + 12345));
    assert!(
        !listing.contains("div_u16") && !listing.contains("rem_u16"),
        "power-of-two literal divisors must not call the divide routine:\n{listing}"
    );
}

#[test]
fn test_div_and_rem_define_a_zero_divisor() {
    // the helper entry points define `x / 0 == 0` and `x % 0 == x` on both the
    // host and the target; the raw `/` operator keeps Rust's host panic
    let src = r#"
fn main() {
    let z: u16 = 0;
    let zi: i16 = 0;
    let a: u16 = 1000;
    let s: i16 = -1000;
    if div_u16(a, z) == 0 && rem_u16(a, z) == a && div_i16(s, zi) == 0i16 && rem_i16(s, zi) == s {
        halt(1);
    } else {
        halt(0);
    }
}
"#;
    let opts = CompilerOptions::default();
    let (_, signal, _) = compile_program_and_run(src, &opts, 40_000);
    assert_eq!(signal, Some(1));
}

#[test]
fn test_div_and_rem_signed_follow_c_truncation() {
    let src = r#"
fn main() {
    let a: i16 = -7;
    let b: i16 = 2;
    let c: i16 = 7;
    let d: i16 = -2;
    let e: i16 = -13;
    let acc: i16 = (a / b) + (a % b) + (c / d) + (c % d) + (e / 5i16) + (e % 5i16);
    halt(acc as u16);
}
"#;
    let opts = CompilerOptions::default();
    let (_, signal, _) = compile_program_and_run(src, &opts, 40_000);
    let expected =
        (-7i16 / 2) + (-7i16 % 2) + (7i16 / -2) + (7i16 % -2) + (-13i16 / 5) + (-13i16 % 5);
    assert_eq!(signal, Some(expected as u16));
}

#[test]
fn test_div_edge_values_and_nesting() {
    // the divisors are variables on purpose: a *constant* divisor lowers to the
    // CpuV3-only MUL16 magic path, while this test exercises the library routine
    let src = r#"
fn main() {
    let a: u16 = 0xffff;
    let b: u16 = 0x8000;
    let d3: u16 = 3;
    let d7: u16 = 7;
    let d6: u16 = 6;
    let nested_div = 100 / (37 / d6);
    let nested_rem = 100 % (37 / d6);
    halt((a / d3) + (b / d3) + (a % d7) + (b % d7) + nested_div + nested_rem * 1000);
}
"#;
    let opts = CompilerOptions::default();
    let (_, signal, _) = compile_program_and_run(src, &opts, 40_000);
    let expected = (0xffffu16 / 3)
        .wrapping_add(0x8000u16 / 3)
        .wrapping_add(0xffffu16 % 7)
        .wrapping_add(0x8000u16 % 7)
        .wrapping_add(100u16 / (37u16 / 6u16))
        .wrapping_add((100u16 % (37u16 / 6u16)).wrapping_mul(1000u16));
    assert_eq!(signal, Some(expected));
}

#[test]
fn test_heap_init_inserted_once_and_only_when_used() {
    // using malloc twice must still insert exactly one init_heap call
    let src = r#"
fn get() -> u16 {
    malloc(1).addr()
}
fn main() {
    let a = get();
    let b = malloc(1).addr();
    halt(a + b);
}
"#;
    let opts = CompilerOptions::default();
    let (_, _, listing) = compile_program_and_run(src, &opts, 4000);
    let n = listing.matches("call init_heap").count();
    assert_eq!(n, 1, "init_heap must be called exactly once:\n{listing}");
    assert!(listing.contains("global init: runtime heap"), "{listing}");

    // not using the library: no init at all
    let src2 = "fn main() { halt(0); }";
    let (_, _, listing2) = compile_program_and_run(src2, &opts, 1000);
    assert!(!listing2.contains("call init_heap"));
    assert!(!listing2.contains("malloc"));
}
