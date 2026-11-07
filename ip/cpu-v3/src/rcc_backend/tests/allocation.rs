use super::*;

#[test]
fn heap_realloc_grows_in_place_and_preserves_words() {
    let source = r#"
            fn main() {
                let p = malloc(4);
                unsafe {
                    p.write(0, 11);
                    p.write(1, 22);
                    p.write(2, 33);
                    p.write(3, 44);
                };
                let q = heap_realloc(p, 8);
                if q.addr() != p.addr() { halt(10); }
                if unsafe { q.read(0) } != 11 || unsafe { q.read(3) } != 44 { halt(11); }
                let r = heap_realloc(q, 2);
                if r.addr() != p.addr() || unsafe { r.read(1) } != 22 { halt(12); }
                free(r);
                halt(1);
            }
        "#;
    let options = CompilerOptions {
        heap_size: 24,
        ..CompilerOptions::default()
    };
    assert_eq!(run_with_std_options(source, options, 100_000).0, 1);
}

#[test]
fn heap_realloc_falls_back_and_best_fit_prefers_the_smaller_hole() {
    let source = r#"
            fn main() {
                let a = malloc(4);
                let b = malloc(10);
                let guard1 = malloc(4);
                let small = malloc(6);
                let guard2 = malloc(4);
                unsafe { a.write(0, 77) };
                let moved = heap_realloc(a, 8);
                if moved.addr() == a.addr() || unsafe { moved.read(0) } != 77 { halt(11); }
                free(b);
                free(small);
                let fit = malloc(5);
                if fit.addr() != small.addr() { halt(10); }
                free(fit);
                free(moved);
                free(guard1);
                free(guard2);
                halt(1);
            }
        "#;
    let options = CompilerOptions {
        heap_size: 64,
        ..CompilerOptions::default()
    };
    assert_eq!(run_with_std_options(source, options, 100_000).0, 1);
}

#[test]
fn vec_grows_with_the_default_heap_and_supports_reserve_and_shrink() {
    let source = r#"
            fn main() {
                let v = vec_new();
                let mut i: u16 = 0;
                while i < 8 {
                    vec_push(v, i + 10);
                    i += 1;
                }
                if vec_len(v) != 8 || vec_cap(v) < 8 { halt(10); }
                if vec_get(v, 0) != 10 || vec_get(v, 7) != 17 { halt(11); }
                while vec_len(v) > 3 { vec_pop(v); }
                vec_shrink_to_fit(v);
                if vec_cap(v) != 3 || vec_get(v, 2) != 12 { halt(12); }
                vec_clear(v);
                if !vec_is_empty(v) { halt(13); }
                vec_reserve(v, 4);
                if vec_cap(v) < 4 { halt(14); }
                vec_free(v);
                halt(1);
            }
        "#;
    assert_eq!(run_with_std_capped(source, 100_000), 1);
}

#[test]
fn zero_capacity_vec_can_grow_and_checked_pop_rejects_empty() {
    let grow = r#"
            fn main() {
                let v = vec_new();
                vec_push(v, 7);
                vec_push(v, 8);
                vec_push(v, 9);
                vec_push(v, 10);
                halt(vec_cap(v) * 100 + vec_get(v, 3));
            }
        "#;
    let options = CompilerOptions {
        heap_size: 20,
        vec_init_cap: 0,
        ..CompilerOptions::default()
    };
    assert_eq!(run_with_std_options(grow, options, 100_000).0, 410);

    let empty_pop = "fn main() { let v = vec_new(); vec_pop(v); halt(1); }";
    assert_eq!(run_with_std(empty_pop), 0xffe3);
}

#[test]
fn heap_coalesces_both_sides_and_rejects_double_free() {
    let coalesce = r#"
            fn main() {
                let a = malloc(4);
                let b = malloc(4);
                let c = malloc(4);
                free(a);
                free(c);
                free(b);
                let whole = malloc(18);
                if whole.addr() != a.addr() { halt(10); }
                free(whole);
                halt(1);
            }
        "#;
    let options = CompilerOptions {
        heap_size: 24,
        ..CompilerOptions::default()
    };
    assert_eq!(run_with_std_options(coalesce, options, 100_000).0, 1);

    let double_free = "fn main() { let p = malloc(1); free(p); free(p); halt(1); }";
    assert_eq!(run_with_std(double_free), 0xffe2);
}

#[test]
fn heap_at_zero_supports_exact_fit_and_reports_oom() {
    let source = r#"
            fn main() {
                let p = malloc(6);
                if p.addr() != 1 { halt(10); }
                unsafe { p.write(5, 77) };
                if unsafe { p.read(5) } != 77 { halt(11); }
                free(p);
                malloc(7);
                halt(1);
            }
        "#;
    let options = CompilerOptions {
        code_base: 0x100,
        heap_begin: 0,
        heap_size: 8,
        ..CompilerOptions::default()
    };
    assert_eq!(run_with_std_options(source, options, 100_000).0, 0xffe1);

    let maximum = r#"
            fn main() {
                let p = malloc(0x7ffd);
                if p.addr() != 1 { halt(10); }
                unsafe { p.write(32764i16, 77) };
                if unsafe { p.read(32764i16) } != 77 { halt(11); }
                halt(1);
            }
        "#;
    let maximum_options = CompilerOptions {
        code_base: 0x8000,
        data_base: 0x9000,
        heap_begin: 0,
        heap_size: 0x7fff,
        ..CompilerOptions::default()
    };
    assert_eq!(run_with_std_options(maximum, maximum_options, 100_000).0, 1);
}

#[test]
fn vec_checked_access_and_capacity_failures_are_stable() {
    let out_of_bounds = "fn main() { let v = vec_new(); vec_push(v, 7); vec_get(v, 1); halt(1); }";
    assert_eq!(run_with_std(out_of_bounds), 0xffe3);

    let overflow = "fn main() { let v = vec_new(); vec_reserve(v, 0xffff); halt(1); }";
    assert_eq!(run_with_std(overflow), 0xffe4);
}
