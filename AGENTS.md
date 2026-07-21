# DigitalDesign-code Project Guide

Multi-crate workspace: `core` (`digital_design_code`, circuit component library), `cpu_macro` (`define_isa!` macro), `cpu_v1` (legacy CPU), and `cpu_v2` (current mainline and the focus of this file).

## cpu_v2 Structure (`src/`)

- `isa.rs` / `isa.html` — ISA v2.6 definitions (`cpu_macro` generates encoding, decoding, and `Display`); **do not modify**.
- `sim.rs` — instruction-set simulator (`eval` → `StateChange` → `commit`).
- `compiler/` — compilation pipeline: SSA/CFG IR (builder) → passes (constant propagation/CSE/DCE) → linear-scan register allocation → code generation (branch relaxation) → assembler/linker.
- `frontend/` — rcc frontend (syn parsing → subset validation → AST-to-IR lowering), plus `spec.md` (rcc language specification, kept in sync with the implementation).
- `rcc_std/` — standard library (heap/mem/mul/vec, self-hosted in rcc; automatically linked into every program, with unused functions removed by the linker).
- `dsl_rt.rs` — host-side implementations of rcc built-ins (`Ptr`/`Slice2`/`addr_of`/`halt`/`assert`) that keep rcc programs valid Rust readable by rust-analyzer and rustc.
- `dsl_progs/` — rcc example programs (named `*_dsl.rs`).
- `bin/rcc.rs`, `bin/rcc-run.rs` — CLI artifacts.
- `tests/` (`cpu_v2/tests/`) — integration tests: `common` (helpers), `rcc_basics`, `rcc_control`, `rcc_calls`, `rcc_memory`, `rcc_errors`, `rcc_std`, and `rcc_ported`.

## ISA v2.6 Quick Reference (16 Registers, Harvard Architecture)

- Instruction and data memory are each 64K. `r13 = ra` (written by call instructions) and `r14 = sp` (used implicitly by `sp_add`/`sp_sub`/`store_sp`/`load_sp`) are **not allocatable**.
- Compiler calling convention: returns in r0–r1, arguments in r2–r7, caller-saved r0–r7, callee-saved r8–r12 (saved as needed, with `ra` saved automatically by non-leaf functions), and `tmp = r15` (reserved for far calls, branch relaxation, and scratch use; not allocatable).
- Immediate limits: `j_cc` ±128 (out-of-range jumps are handled by backend branch relaxation), `addi` only ±1..8 with no zero, `cmp_i` u4 / `cmp_si` i4, `load_mem`/`store_mem` offset i4 (−8..+7), `store_sp`/`load_sp`/`sp_sub`/`sp_add` u8, and shifts only by u4 immediates.
- Direct calls undergo fixed-point whole-program linker relaxation: in-range calls use an unpadded single-slot `call_rel`; out-of-range calls use `load_lo` + `load_hi` + `call_reg`. The automatic function table considers only hot call targets that remain far after relaxation; table entries use a single-slot `call_abs`. Indirect calls use `tmp` + `call_reg`. The table starts at data address `0xff00` and is initialized before the `main` frame is established, using `sp` as the base with u8-offset `store_sp` instructions. Listings and `.dbg` files mark stack/table/static/runtime initialization as compiler-generated global initialization ranges.
- Functions are separated by one `halt` slot for disassembly boundaries and out-of-range PC fallback.

## rcc Language (See `src/frontend/spec.md`)

rcc is a strict Rust subset: valid rcc is valid Rust, and all semantics are unsafe. Types include `u16`, `i16`, `Ptr` (data pointer), `Array<u16|i16>` (single-word typed view), function pointers, and `bool` (conditions only; not storable). Arrays `[u16; N]` / `[i16; N]` support `as_array()` followed by `a[i]` / `a[i] = v`. `Array` indices are limited to `u16` or `i16` (write literals as `a[3u16]` / `a[-1i16]`; small offsets use i4 memory operations directly). `Array` supports `as_ptr()`, while `Ptr` supports `as_u16_array()` / `as_i16_array()`. Legacy `Slice2` operations `read`/`write`/`as_ptr`/`len` remain supported. The target performs no bounds checking. `const` and `static` live in the data segment, with an implicit `__data_init` at the `main` entry. For `addr_of(&x)`, globals become compile-time constants and locals become `sp + slot`. Supported control flow includes `if`, `while`, `for`, `break`, `continue`, `if` expressions, and short-circuit `&&`, `||`, and `!`. Function pointers use `LoadFuncAddr` + `call_reg`. **Unsupported:** `*`, `/`, `%`, structs, generics (except the `Array` type parameter), closures, `match`, references, and macros; each produces a positioned diagnostic. Standard-library functions are called directly by name (`malloc`, `free`, `mem_set`, `mem_copy`, `mul_16x4`/`8`/`16`, `vec_*`). Based on call-graph reachability, the compiler inserts one `init_heap` / `init_vec` call at the `main` entry, with arguments from `CompilerOptions`.

## Common Commands

Local builds must initialize the MSVC environment and put Cargo first on `PATH`; otherwise, GNU `link` from Git Bash shadows MSVC `link.exe`. Build-environment initialization is documented here (equivalent to the obsolete `run_tests.bat`):

```bat
:: Contents of build_env.bat (run line by line in cmd.exe, or save and invoke as a .bat file)
call "C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul 2>&1
set PATH=C:\Users\misdake\.cargo\bin;%PATH%
```

After initialization, Cargo commands work normally:

- Tests/static checks: `cargo test -p cpu_v2`, `cargo clippy -p cpu_v2 --all-targets`.
- Build binaries: `cargo build -p cpu_v2 --bins` (outputs: `target/debug/rcc.exe`, `rcc-run.exe`).
- Compile an rcc program: `./target/debug/rcc <input.rs> [-o out.bin] [--lst out.lst] [--no-opt] [--function-table auto|none|all|name,...] [--stack-init N] [--data-base N] [--heap-begin N] [--heap-size N] [--vec-cap N]` (numbers may be decimal or `0x`-prefixed). `mod name;` is resolved beside the input file as `<name>.rs`, `<name>.dsl.rs`, or `<name>/mod.rs`. Compilation produces three artifacts: a `.bin` binary image (RCC1 header + u16-LE instructions), a `.lst` disassembly (function signatures, block roles, call names, and `; line N`), and `.dbg` debug information (file table, function table, functions with address/frame/variable locations as `rN`/`frame+N`/`ssa`, global variable addresses, and the PC-to-line map used by the debugger).
- Run: `./target/debug/rcc-run <input.bin> [max_cycles]` → prints the halt signal and cycle count.
- Debug: `./target/debug/rcc-dbg <input.bin> [--port 8321]` → opens the single-page web debugger at http://127.0.0.1:8321. It is **source-first**: the source pane highlights the current line and allows source-line breakpoints by clicking line numbers; controls include next line, step over, step out, single instruction, continue, and reset. The secondary disassembly pane gives the current PC the highest-priority blue highlight with a red arrow on the left; disassembly-address breakpoints are unsupported. Additional panes show registers and flags, memory with address/sp/heap/data shortcuts, globals (name/type/address/value), live locals for the current function (`rN`/`frame+N`, with `ssa` entries hidden), and the shadow call stack (function name + return address). **Next line enters the next source line**, including line-by-line entry into callees; step over uses shadow-call-stack depth to avoid entering callees; step out returns to the caller. Source indentation is preserved; clicking source-line text highlights its corresponding disassembly instructions; clicking a `Ptr` variable jumps to memory. API: `GET /api/state`, `GET /api/mem?addr=&len=`, `POST /api/cmd?cmd=step|next|over|out|continue|reset`, and `POST /api/breakline?file=<idx>&line=<n>&on=0|1`.
- View disassembly: compile with `--lst`, or run `test -p cpu_v2 compiler::tests::optimize::test_listing_demo -- --nocapture`. `Compiler::finish` always returns `(Vec<Instruction>, String)` containing function signatures, block roles, call names, and `; line N`.

## Conventions

- Every test must pass a maximum cycle count to `simulate` to prevent infinite-loop hangs. Prefer computing expected values with equivalent Rust code at the test site instead of using magic numbers.
- Project files must use English only, including code, comments, specifications, and commits.
- The `docs/` directory no longer exists. Its original redesign drafts are obsolete; this file and `src/frontend/spec.md` describe the current state, and build commands are documented here.
- The user decides when changes are committed unless they explicitly request a commit.
- Test layout: keep pipeline unit tests in the relevant files under `src/compiler/`; put language, library, and integration tests in categorized files under `cpu_v2/tests/`.
