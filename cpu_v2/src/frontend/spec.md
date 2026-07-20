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
}
```

There is no array/struct sugar: `p.add(i).read(0)` means `p[i]`. Struct memory layouts
come with the library phase (out of P1 scope).

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
| `dev_recv(dev: u8, ch: u8) -> u16` | read a device (not yet) |
| `dev_send(dev: u8, ch: u8, v: u16)` | write a device (not yet) |

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

- Subset programs live in `cpu_v2/src/dsl_progs/` with **file names ending in `_dsl.rs`**
  (they are both rcc sources and cargo modules, so rustc/rust-analyzer read them directly).
- The compiler frontend lives in `cpu_v2/src/compiler/frontend/`
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

### 10.2 Accessing arrays (the `Slice2` trait)

`dsl_rt` provides a real Rust extension trait `Slice2` implemented for `[u16; N]`/`[i16; N]`
(const generics), so method calls resolve cleanly in rust-analyzer; the compiler recognizes
them as intrinsics:

| method | meaning |
|---|---|
| `arr.read(i) -> u16` | `arr[i]` (i is any integer expression) |
| `arr.write(i, v)` | `arr[i] = v` |
| `arr.as_ptr() -> Ptr` | address of element 0 (array *decays* to a pointer, like C) |
| `arr.len() -> u16` | N as a compile-time constant |

On the host these methods index real Rust arrays — so **the host run keeps Rust's bounds
check for free**, while the target emits raw unchecked addressing (exactly the C model).

### 10.3 Arrays as parameters

There are no fat slices (`&[u16]` is two words — not supported). Pass arrays as `Ptr`:

```rust
fn blit(tiles: Ptr, n: u16) { ... }
blit(TILE.as_ptr(), 8);
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

Struct members (including array members) come with the library phase; nothing in §9–§11
precludes them (a struct is just an address plus offsets).

## 12. Out of scope for now

`&x` references, fat slices, struct definitions, `static mut`, heap allocation of arrays,
multi-dimensional arrays (use `arr[i * W + j]`), `*`, `/`, `%`.

## 13. The toolchain

### 13.1 Compilation pipeline

`frontend::compile_program(src, opts, loader)` compiles a whole program:

1. the main source plus any `mod name;` files resolved through `loader`;
2. the **rcc_std library** (`cpu_v2/src/rcc_std/`, written in rcc itself) is always appended;
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
| `data_base` | 0 | static data section base address |
| `heap_begin` | 0x1000 | heap region start |
| `heap_size` | 20 | heap region size in words |
| `vec_init_cap` | 4 | `vec_new()` initial capacity |

### 13.3 Artifacts

- `rcc <input.rs> [-o out.bin] [--lst out.lst] [--no-opt] [--stack-init N] [--data-base N]
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

`dsl_rt` keeps the subset programs valid Rust: `Ptr` methods, `Slice2` (const-generic array
access with bounds checks on the host), `addr_of`, intrinsics. `rcc_std` is a real module
tree for the same reason.

`rcc-dbg [input.bin] [--port N]` opens an existing binary in the web debugger. With no
input file (or with `--playground`) it serves a single-file playground: source is compiled
to an in-memory debugger session, then the page can either switch between its editor and
debugger views or open the debugger in a separate window. Playground recompilation replaces
the active session without writing temporary `.bin`, `.lst`, or `.dbg` files. External
`mod` files are intentionally unavailable in this mode; the embedded standard library is
still included normally. The editor's `Optimize` toggle controls the compiler passes; turning
it off also enables the debug-friendly scalar-local frame-slot behavior described above.
