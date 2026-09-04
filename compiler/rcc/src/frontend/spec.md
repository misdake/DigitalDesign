# rcc — a minimal Rust-subset language for this CPU

> rcc (retro console compiler) is a **tiny strict subset of Rust syntax**: every valid rcc
> program is also a valid Rust program, so rust-analyzer parses, highlights and navigates
> it with no plugin at all.
> The design is **C's memory model under Rust syntax**: all code is semantically unsafe
> bare-metal operation — no borrow checking, no lifetimes, no bounds checks.
> Anything outside the subset is a **hard error with a source location**.
>
> Target: a 16-bit Harvard-architecture CPU (separate instruction/data stores, see `isa.html`).

## 1. Type system

Only these types exist; no other primitive types are supported:

| type | meaning | notes |
|---|---|---|
| `u16` | unsigned 16-bit word | default integer type; literals `123`, `0x1f`, `123u16` |
| `i16` | signed 16-bit word | literals `123i16`, `-5i16`; comparisons/shifts are signed |
| `Ptr` | **data pointer** (into data memory) | the `dsl_rt::Ptr` newtype over a u16 address; no plain arithmetic, only its methods and `as` casts |
| `Array<T>` | **typed array view** | one-word unchecked address, where T is `u16` or `i16`; supports indexing and converts to/from `Ptr` |
| `fn(A, B) -> R` | **function pointer** (into instruction memory) | plain Rust fn pointer type; on a Harvard machine this is a *different kind* from `Ptr` and they never convert |
| `bool` | **condition expressions only** | the type of comparisons and `&& \|\| !`; cannot be stored in variables/memory (see §6) |
| `()` | unit | return type of procedures |

### 1.1 Type rules

- `let` accepts a type annotation: `let x: i16 = -5;`. Without one the type is inferred from
  the initializer; unsuffixed integer literals are flexible and adopt the type of the other
  side (`u16` by default).
- Arithmetic and bitwise operators require both sides to be the same integer type
  (`u16`/`u16` or `i16`/`i16`); mixing is an error — cast explicitly with `as`.
- Unary `-` is allowed only on `i16` (same as Rust); `!x` is bitwise not on integers and
  logical not on bools.
- `x as u16` / `x as i16` / `p as u16` / `a as Ptr` (only between `u16`/`i16`/`Ptr`) reinterpret bits.
- `>>` is logical on `u16` and arithmetic on `i16` (matches Rust and the ISA).
  **The shift amount must be a literal constant** (the ISA has no register-shift instruction).
- `*` multiplication, `/` and `%` are **not supported yet** (the hardware has no mul/div;
  multiplication will arrive via the library, division is out of scope).

## 2. The `Ptr` data pointer

Raw pointer arithmetic needs `unsafe {}` in real Rust (which would make rust-analyzer
complain), so all pointer operations go through `Ptr`'s inherent methods (the compiler
recognizes them as intrinsics; the IDE sees ordinary methods):

```rust
impl Ptr {
    fn from_addr(addr: u16) -> Ptr;   // build from a word address
    fn addr(self) -> u16;             // extract the word address
    fn add(self, off: i16) -> Ptr;    // address + off (may be negative)
    fn read(self, off: i16) -> u16;   // mem[self + off]
    fn write(self, off: i16, v: u16); // mem[self + off] = v
    fn as_u16_array(self) -> Array<u16>;
    fn as_i16_array(self) -> Array<i16>;
}
```

`Ptr` remains the untyped interface for address arithmetic and raw words. Convert it to
an `Array<T>` when typed indexing is clearer. Struct memory layouts remain out of scope.

## 3. Function pointers

- Function items can be bound to fn pointer variables or passed as fn pointer arguments:
  ```rust
  fn double(x: u16) -> u16 { x + x }
  fn apply(f: fn(u16) -> u16, x: u16) -> u16 { f(x) }   // indirect call
  fn main() { let g: fn(u16) -> u16 = double; apply(g, 21); }
  ```
- An indirect call `f(x)` must match the declared fn pointer signature
  (up to 6 parameters, up to 1 return value).
- Taking a function's address is a relocation (the linker backfills the absolute address);
  indirect calls go through `call_reg`.

## 4. Control flow

- `if cond { } else if cond { } else { }` (statement form; `else` optional).
- `if cond { a } else { b }` as an **expression** (both branches same type; C's `cond ? a : b`).
- `while cond { }`, `loop { }`, `break;`, `continue;`.
- `for i in a..b { }` and `for i in a..=b { }` (step is always 1; `i` takes the range's type).
- Conditions are `bool` expressions (comparisons, `&&`, `||`, `!`). Loop-carried variables
  become phis automatically.
- `return;` / `return expr;` (must match the signature). A trailing tail-expression
  (no semicolon) in a function body is the return value, as in Rust.

## 5. Intrinsics

Declared for real in `dsl_rt` (so the IDE sees them); the compiler lowers them directly:

| function | meaning |
|---|---|
| `halt(x: u16) -> !` | halt the machine with signal x |
| `assert(cond: bool, sig: u16)` | halt(sig) unless cond holds |
| `cnt1(x: u16) -> u16` | number of set bits in x |
| `log2(x: u16) -> u16` | integer base-2 logarithm; returns 0 when x is 0 |
| `dev_recv(dev: u8, ch: u8) -> u16` | read a device register; device and channel are compile-time constant IDs |
| `dev_send(dev: u8, ch: u8, v: u16)` | write a device register; device and channel are compile-time constant IDs |
| `dcache_clean_all() -> u16` | CPU V3-only: blocking full compiler memory/control barrier; write every dirty D-cache line and return final maintenance status |
| `dcache_invalidate_all() -> u16` | CPU V3-only: blocking full compiler memory/control barrier; clean and invalidate the complete D-cache, then return final maintenance status |
| `mtsr_dseg(v: u16)` | CPU V3-only: write the DSEG special register (MTSR DSEG) |
| `jseg(cseg: u16, target: u16) -> !` | CPU V3-only: atomically switch CSEG to `cseg` and jump to `target` (JSEG); never returns |
| `icache_invalidate_delayed_and_jump(cseg: u16, target: u16) -> !` | CPU V3-only: terminal barrier lowered to adjacent `ICACHE_INVALIDATE_ALL_DELAYED; JSEG`; never returns |

## 6. Design decisions

- **bool does not exist as a stored type**: the machine has no byte/bool instructions, so a
  stored bool would waste a whole word and invite arithmetic-on-bool confusion. Comparisons
  and logical operators cover control flow; to keep a flag, use `u16` 0/1.
  `let b = x < y;` is currently an error (bool only lives in conditions).
- **u16 vs i16 matters**: signed comparisons (`cmp_s`) and arithmetic shifts are only
  produced when both operands are `i16`; mixed integer arithmetic is an error, because 
  implicit conversions hide too many bugs on a 16-bit machine.
- **Unsupported means error**: these Rust features are rejected with a span — generics,
  traits, impls, closures, match, destructuring patterns, macros, references `&`, slices/array
  literals, strings, floats, other integer types, `unsafe`, `extern`, lifetimes,
  `const`/`static`, attributes (except ignored `#[allow(...)]`), `use` (parsed but ignored;
  it exists for the IDE).
- **No division** (`/`, `%`), **no multiplication yet** (`*`): both report "not supported yet".

## 7. A complete example

```rust
// sum_dsl.rs — sum 1..=10 and halt with the result
use crate::dsl_rt::*;

fn main() {
    let mut sum: u16 = 0;
    for i in 1..=10u16 {
        sum += i;
    }
    halt(sum);
}
```

The output links with cpu_v2's `Compiler` and runs on the `sim` simulator.

## 8. File organization

- CPU V2 subset programs live in `ip/cpu-v2/src/dsl_progs/` with **file names ending in `_dsl.rs`**
  (they are both rcc sources and cargo modules, so rustc/rust-analyzer read them directly).
- The compiler frontend lives in `compiler/rcc/src/frontend/`
  (syn parsing → subset validation → AST→IR lowering).

## 9. Constants and globals (data section)

Three kinds of file-level data items; everything else (`static mut`, `let` at file scope) is an error.

### 9.1 `const` — compile-time constants

```rust
const WIDTH: u16 = 160;
const HALF: i16 = -3;
```

Inlined as immediates at every use; costs no memory. The initializer must be a constant
expression (literals and arithmetic on other consts).

### 9.2 `static NAME: Ty = expr;` — global scalars

```rust
static SCORE: u16 = 0;
static TICK: i16 = -1;
```

One word in data memory at a compiler-assigned address; the compiler emits a hidden
`__data_init` routine at the start of `main` that stores each non-zero initializer.
- Reading `SCORE` as a value loads the word.
- Writing goes through the address: `addr_of(&SCORE).write(0, v)` (immutable `static` reads
  are safe Rust, so rust-analyzer stays quiet; mutation is intentionally explicit).

### 9.3 `static NAME: [Ty; N] = [e0, e1, ...];` — global arrays

```rust
static TILE: [u16; 8] = [0x3c, 0x66, 0xc3, 0xff, 0xff, 0xc3, 0x66, 0x3c];
```

N consecutive words in data memory (the sprite/tile/palette data of a game). Same access
rules as local arrays (§10), same `__data_init` emission for non-zero words.

## 10. Arrays

C semantics: an array is N consecutive words, addressed by a plain (single-word) pointer,
**no bounds checks** on target. Array types are `[u16; N]` and `[i16; N]`.

### 10.1 Local arrays

```rust
let mut buf: [u16; 8] = [0; 8];        // stack, zero-filled (or a full list [1,2,..,8])
buf.write(i, 7);
let x = buf.read(i) + buf.read(3);
```

Local arrays live in the stack frame (a compile-time sized local area, see §11).

### 10.2 Typed array views and indexing

`arr.as_array()` produces `Array<u16>` or `Array<i16>`. It is only a typed one-word
address; the length is not carried at run time and target accesses are unchecked. Index
expressions must be `u16` or `i16`. Give a bare literal an explicit suffix (`a[3u16]` or
`a[-1i16]`) so the same source also type-checks in Rust; `i32` and `usize` indices are not
supported. Small literal offsets lower directly to the load/store i4 address field.

```rust
let mut storage: [u16; 8] = [0; 8];
let mut words = storage.as_array();
words[i] = 7;
words[3u16] += 1;
let x = words[i] + words[3u16];
let raw: Ptr = words.as_ptr();
```

An array view passed to a function uses one argument register. Declare an `Array<T>`
parameter `mut` only when assigning through its index:

```rust
fn clear_first(mut words: Array<u16>) { words[0u16] = 0; }
clear_first(storage.as_array());
```

Convert a raw pointer with `p.as_u16_array()` or `p.as_i16_array()`. The explicit method
name supplies the element type without generic-method inference.

### 10.3 Legacy fixed-array methods

`dsl_rt` provides a real Rust extension trait `Slice2` implemented for `[u16; N]`/`[i16; N]`
(const generics), so method calls resolve cleanly in rust-analyzer; the compiler recognizes
them as intrinsics. These methods remain supported for compatibility:

| method | meaning |
|---|---|
| `arr.read(i) -> u16` | `arr[i]` (i is any integer expression) |
| `arr.write(i, v)` | `arr[i] = v` |
| `arr.as_ptr() -> Ptr` | address of element 0 (array *decays* to a pointer, like C) |
| `arr.as_array() -> Array<T>` | typed address of element 0 |
| `arr.len() -> u16` | N as a compile-time constant |

On the host these methods index real Rust arrays — so **the host run keeps Rust's bounds
check for free**, while the target emits raw unchecked addressing (exactly the C model).

### 10.4 Arrays as parameters

There are no fat slices (`&[u16]` is two words — not supported). Pass typed data as
`Array<T>`, or use `Ptr` when the function intentionally operates on raw words:

```rust
fn blit(tiles: Array<u16>, n: u16) { ... tiles[i] ... }
blit(TILE.as_array(), 8);
```

## 11. Taking addresses: `addr_of`

There is no `&` operator (references are out of subset). The intrinsic
`addr_of(&x) -> Ptr` takes the address of a variable:

- **globals** (`addr_of(&SCORE)`, or `TILE.as_ptr()`): the address is a **compile-time
  constant** (an immediate in the emitted code).
- **locals** (`addr_of(&x)`): the *allocation* is decided at compile time — the variable is
  placed in the function's stack frame instead of a register — but the **address value is
  only known at run time** (`sp + slot`). The compiler emits `mov sp, t; addi t, slot`
  wherever `addr_of(&x)` is evaluated. So: compile-time placement, run-time value.

Any local whose address is taken, and every local array, becomes **memory-resident**: all
its reads/writes go through frame slots (the existing `load_sp`/`store_sp` machinery).
The frontend decides residency statically by scanning for `addr_of` uses and array-typed
`let`s — no escape analysis. The frame layout becomes
`[callee-save saves][locals/arrays][spill slots]`, all sized at compile time.
The entry function has no caller and never returns, so it omits callee-save and return-address
saves; any locals and spills still allocate their normal frame slots.

Struct members (including array members) come with the library phase; nothing in §9–§11
precludes them (a struct is just an address plus offsets).

## 12. Out of scope for now

`&x` references, fat slices, struct definitions, `static mut`, heap allocation of arrays,
multi-dimensional arrays (use `arr[i * W + j]`), function inlining/`#[inline]`, `*`, `/`, `%`.

## 13. The toolchain

### 13.1 Compilation pipeline

`frontend::compile_program(src, opts, loader)` compiles a whole program:

1. the main source plus any `mod name;` files resolved through `loader`;
2. the **rcc_std library** (`compiler/rcc/src/rcc_std/`, written in rcc itself) is always appended;
   unused functions are dropped by the linker;
3. **automatic library initialization**: if the program's call graph reaches `malloc`/`free`,
   a single `init_heap(heap_begin, heap_size)` call is inserted at the start of `main`; if it
   reaches `vec_*`, a single `init_vec(vec_init_cap)` call follows. each init runs exactly once
   per program, with parameters from `CompilerOptions`.

### 13.2 `CompilerOptions`

| option | default | meaning |
|---|---|---|
| `opt` | all on | optimization passes (const-prop/cse/dce/coalesce) |
| `stack_init` | 0 | initial sp of the entry fn (0 = simulator default; frames grow downward) |
| `function_table` | `Auto` | `Disabled`, automatically profitable/hot direct callees, all direct callees, or an explicit list of function names |
| `data_base` | 0 | static data section base address |
| `heap_begin` | 0x1000 | heap region start |
| `heap_size` | 20 | heap region size in words |
| `vec_init_cap` | 4 | `vec_new()` initial capacity |

### 13.3 Artifacts

- `rcc <input.rs> [-o out.bin] [--lst out.lst] [--no-opt]
  [--function-table auto|none|all|name,...] [--stack-init N] [--data-base N]
  [--heap-begin N] [--heap-size N] [--vec-cap N]` — compiles to a binary image
  (`RCC1` magic + word count + u16-LE words), a disassembly listing with function
  signatures, block roles, call targets, and source line comments (`; line N`), and a
  `.dbg` debug-info file (below).
- `rcc-run <input.bin> [max_cycles]` — runs the image on the simulator and prints
  the halt signal (decimal/hex) and cycle count.

Full-program compiler diagnostics include the originating source file, one-based line and
column, the relevant source line, and a caret. This applies to syntax errors, subset/type
errors, module loading errors, and errors produced while lowering a module function. The
playground moves the editor caret to the primary diagnostic location after a failed build.

### 13.6 Debug info (`.dbg`)

Alongside the binary and listing, `rcc` writes `<input>.dbg` for a hypothetical debugger:

- **files**: index of source files (main file, `mod` files, rcc_std files);
- **function table**: table index and function name for each `call_abs` target;
- **initialization sections**: address ranges and details for compiler-generated stack,
  function-table, static-data, heap, and vector initialization code;
- **functions**: name, address range, source file, frame size, and every local variable
  with a location: `rN` (ABI register for params), `frame+N` (frame slot — arrays and
  address-taken locals), `global@0xADDR`, or `ssa` (register/versioned). Local entries
  also carry an inclusive lexical `scope START..END` line range; parameter values are
  captured at call entry because the ABI argument registers are caller-save;
- **globals/consts**: static names with types and data addresses, constants with values;
- **line table**: `line 0xADDR <file> <line>` per instruction that maps to a source line.
  Supporting instructions introduced for a source operation (call slots, branch
  legalization, ABI moves, and address legalization) retain that operation's line;
  function prologues and other source-independent instructions have no entry.

The mapping is statement-granular and best-effort through optimization (folded/eliminated
code simply has no entries). With all optimization passes disabled, ordinary scalar locals
are materialized in stable frame slots so their values remain inspectable during their
lexical lifetime; optimized builds may report such SSA locals as unavailable.

### 13.4 Library parameters and runtime cells

`init_heap` stores the heap bounds in static cells (`HEAP_BEGIN`/`HEAP_END` in the data
section) which `malloc`/`free` read at run time — no compile-time patching of library code.
`init_vec` does the same for `VEC_INIT_CAP`.

### 13.5 Host/IDE side

`dsl_rt` keeps the subset programs valid Rust: `Ptr` methods, typed `Array<T>` indexing,
`Slice2` (const-generic fixed-array access), `addr_of`, and other intrinsics. `rcc_std` is
a real module tree for the same reason.

`rcc-dbg [input.bin] [--port N]` opens an existing binary in the web debugger. With no
input file (or with `--playground`) it serves a single-file playground: source is compiled
to an in-memory debugger session, then the page can either switch between its editor and
debugger views or open the debugger in a separate window. Playground recompilation replaces
the active session without writing temporary `.bin`, `.lst`, or `.dbg` files. External
`mod` files are intentionally unavailable in this mode; the embedded standard library is
still included normally. The editor's `Optimize` toggle controls the compiler passes; turning
it off also enables the debug-friendly scalar-local frame-slot behavior described above.
The playground's `Calls` selector exposes the automatic, disabled, and all-target function
table modes.

### 13.7 Direct calls and the function table

The compiler can lower selected direct calls to the single-word `call_abs` instruction.
At program entry it sets `sp` to `0xff00` before creating the main frame and initializes the
selected function addresses with `store_sp` offsets `0..255`. This avoids a separate table-base
register and provides direct access to the full table without base-increment instructions.
The default `Auto` mode first performs whole-program call relaxation, then considers only
direct call sites that remain out of `call_rel` range. Repeated or statically hot far calls
(recursion or calls in loops) enter the table when their estimated runtime saving pays for
table initialization. `All` selects every directly called reachable function; `Functions`
accepts an explicit name list. Indirect function-pointer calls still use `call_reg`.

Direct calls not selected for the table have variable-width encodings. The linker repeatedly
lays out all functions and lowers every reachable target to a single `call_rel`; it reruns
intra-function branch relaxation until both call and branch sizes are stable. Calls that remain
out of range use `load_lo` + `load_hi` + `call_reg`. Near calls have no reserved padding, so
returning from `call_rel` immediately executes the next real instruction.

The function table reserves data addresses `0xff00..=0xffff`, so static allocation may not
enter that range. When a non-empty table is used with the default `stack_init = 0`, the entry
stack pointer is initialized to `0xff00` so downward-growing frames cannot overwrite the
table. An explicit stack address above `0xff00` is rejected.

Compiler-generated startup code is represented separately from source code. The listing starts
with a `global initialization` summary and marks the instruction ranges for stack setup,
function-table writes, static data, and any heap/vector runtime setup. The same ranges are stored
in `.dbg`; the generated instructions have no user-source line ownership, while instruction
stepping and the disassembly panel keep them visible as compiler-generated initialization sections.
Static data initialization groups non-zero words by 256-word address page, temporarily uses `sp`
as the page base, and writes each value with `store_sp`'s full u8 offset. After the section it
restores the main frame's stack pointer before any user or runtime code executes.
