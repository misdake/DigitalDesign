//! rcc standard library tests: heap (malloc/free + auto init), mem, mul, vec.

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
