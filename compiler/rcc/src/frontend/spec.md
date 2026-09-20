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
| `Array<T>` | **typed array view** | one-word unchecked address, where T is `u16`, `i16` or a struct; supports indexing and converts to/from `Ptr` |
| `Buf<T, N>` | **owned array: N consecutive words** | the array type (spec §10); `T` is `u16`, `i16` or a struct, initialized with `Buf::new([v; N])` / `Buf::new([e0, e1, ...])`, indexed with a `u16`/`i16` index |
| `(A, B, ...)` | **tuple: up to four scalars** | element `i` at word offset `i` (spec §9c); returnable, destructure with `let (a, b) = ...` |
| an `enum` name | **one word: the variant's discriminant** | C-style fieldless variants in declaration order (spec §9d); `E::A` is a constant, `e as u16` and (with `#[derive(PartialEq)]`) `==` |
| `fn(A, B) -> R` | **function pointer** (into instruction memory) | plain Rust fn pointer type; on a Harvard machine this is a *different kind* from `Ptr` and they never convert |
| `bool` | **one word: 0 or 1** | the type of comparisons and `&& \|\| !`; storable in a variable, passed and returned, and usable as a condition again (`if b`); `b as u16` / `b as i16` yields 0/1 (see §1.1) |
| a defined `struct` name | **struct value: one word address** | fields are 16-bit words in declaration order, total size padded to the struct's alignment; layout and access rules in §9b |
| `fix16` | **signed Q16.16 fixed-point scalar** | CPU V3-only; occupies one scalar F register (16 fractional bits) |
| `vec2` / `vec3` / `vec4` | **fix16 vectors** | CPU V3-only; a consecutive range of 2/3/4 scalar F registers (the host type keeps a zero tail) |
| `()` | unit | return type of procedures |

### 1.1 Type rules

- `let` accepts a type annotation: `let x: i16 = -5;`. Without one the type is inferred from
  the initializer; unsuffixed integer literals are flexible and adopt the type of the other
  side (`u16` by default).
- Arithmetic and bitwise operators require both sides to be the same integer type
  (`u16`/`u16` or `i16`/`i16`); mixing is an error — cast explicitly with `as`.
- Unary `-` is allowed only on `i16` (same as Rust); `!x` is bitwise not on integers and
  logical not on bools.
- A `bool` value is one word holding 0 or 1. A comparison or logical expression used where a value
  is needed (`let b = x < y;`, an argument, a return, a field) is **materialized**: one comparison
  becomes the ISA's Boolean-producing instruction (`SEQ`/`SEQI`, `SLT`/`SLTU`/`SLTI`/`SLTUI`), `!`
  flips the low bit, and a compound `&&`/`||` condition becomes a two-block diamond that the
  diamond-conversion pass folds back when it can. A stored bool is a condition again (`if b`,
  `while b`) and can be passed and returned like any other one-word value. It still never mixes
  with integers: use `b as u16` / `b as i16`, or compare (`x != 0`).
- `x as u16` / `x as i16` / `p as u16` / `a as Ptr` (only between `u16`/`i16`/`Ptr`) reinterpret bits.
- `>>` is logical on `u16` and arithmetic on `i16` (matches Rust and the ISA). The shift
  amount may be any integer expression: a literal selects the immediate encoding (SHLI/SHRI/ASRI
  on CpuV3), a variable selects the register-count encoding (SHL/SHR/ASR on CpuV3, masked to the
  low four bits; register-count shifts are rejected for CpuV2, whose ISA has only immediate shifts).
- `*` multiplication works on integers: CpuV3 lowers it to the hardware `MUL0` (or `MULI` for a
  constant operand); CpuV2 calls the rcc_std `mul_16x16` library.
- `/` and `%` work on integers: neither ISA has a divide, so both lower to the rcc_std `div`
  module (a 16-step shift-subtract routine, see §1.2). A literal power-of-two divisor on `u16`
  becomes a shift or a mask instead of a call.
- FPU types (CPU V3) are **signed Q16.16**: 16 fractional bits, range about
  `[-32768, +32767.99998]`, all arithmetic wrapping. `+`, `-`, `*` work
  component-wise on same-typed FPU values; `vecN * fix16` and `fix16 * vecN`
  scale the vector; unary `-` negates. Comparisons exist only on `fix16`
  (signed ordering through the scalar `CMP` subop and the pending test). There
  are no implicit conversions between FPU and integer types — use the numeric
  `fix16::from_int` / `.to_int()` or the raw-half moves
  `fix16::from_words(lo, hi)` / `.lo_bits()` / `.hi_bits()`. Raw-half moves and
  numeric conversions are deliberately distinct: `from_words`/`lo_bits`/`hi_bits`
  correspond to `ILO2F`/`IHI2F`/`FLO2I`/`FHI2I`/`FLD`/`FST`, while
  `from_int`/`to_int` correspond to `I16TOF`/`FTOI16`.
- FPU lowering has landed for **scalar `fix16`** (milestone C1): construction
  (`from_int`, `from_words`, `zero`), `+`/`-`/`*` and `+=`/`-=`/`*=` on `fix16`,
  unary `-`, the `abs`/`floor`/`ceil`/`round`/`trunc` methods, the raw-half
  `lo_bits`/`hi_bits`, `to_int`, and `fix16` comparisons all lower to the FPU
  v2 scalar/aux/memory ISA and run on the emulator and the RTL. Vector forms
  (`vec2/3/4`, `fdot`, `vec4::import`/`export`), the special functions
  (`frcp`/`frsqrt`/`fsincos`) and the prescale helpers still need C2/C3 and are
  rejected with an explicit diagnostic, so no silently wrong code is emitted.

### 1.2 Division and remainder

`/` and `%` are integer-only and follow C: the quotient truncates toward zero and the remainder
takes the sign of the dividend. Neither ISA has a divide, so the frontend lowers the operation to
the rcc_std `div` module: a 16-step shift-subtract core over `u16`, whose signed entry points take
absolute values and restore the sign.

A **constant** `u16` divisor is cheaper and never calls the routine when a round-up magic exists: a
power of two becomes a shift (`/`) or a mask (`%`), and any other constant becomes one `MUL16` plus
shifts — `q = mulhi(x, m) >> s` when the magic fits 16 bits, or the 17-bit form
`q = ((x & t) + ((x ^ t) >> 1)) >> (s - 1)` with `t = mulhi(x, m)`, which averages `x + t` without a
17-bit intermediate. The compiler verifies each candidate magic against **every** 16-bit numerator
before using it and falls back to the routine otherwise. `i16` always calls the routine, because an
arithmetic shift rounds toward negative infinity. The magic path uses `MUL16`, which CpuV2 lacks, so
a CpuV2 build of a program with a non-power-of-two constant divisor is rejected by the V2 backend
(§12.1).

A zero divisor is **defined**: `x / 0` is `0` and `x % 0` is `x`, on both the target and (through the
`dsl_rt` entry points) the host. `div_u16` / `rem_u16` / `div_i16` / `rem_i16` are real functions in
`rcc_std` that mirror the `dsl_rt` host shims, so a program whose divisor may be zero calls them
explicitly and runs identically both ways. The bare `/` and `%` operators keep rustc's host panic on
a zero divisor — the host executes the real operator, exactly like the array bounds check the target
does not have (§10.3).


`x /= d` and `x %= d` are the compound forms of the same operation and go through the same
lowering, including the constant-divisor specialization (`x /= 4` is a shift, `x %= 4` a mask),
so a compound form never costs more than the two-operand spelling.
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

Declared for real in `dsl_rt` (so the IDE sees them); the compiler lowers them directly.
C1 lowers the scalar `fix16` rows and leaves the `vecN`/`fdot`/`frcp`/`frsqrt`/`fsincos`/prescale
rows rejected with an explicit "after C1" diagnostic on CPU V3.

| function | meaning |
|---|---|
| `halt(x: u16) -> !` | halt the machine with signal x |
| `assert(cond: bool, sig: u16)` | halt(sig) unless cond holds |
| `cnt1(x: u16) -> u16` | number of set bits in x (`POPCNT`) |
| `log2(x: u16) -> u16` | integer base-2 logarithm; returns 0 when x is 0 |
| `clz(x: u16) -> u16` | count leading zeros (`CLZ`); CpuV3-only |
| `sextb(x: u16) -> i16` | sign-extend the low byte (`SEXTB` on CpuV3, a shift expansion on CpuV2) |
| `mul8(a: u16, b: u16) -> u16` | unsigned product bits [23:8] (`MUL8`); CpuV3-only |
| `mul16(a: u16, b: u16) -> u16` | unsigned product bits [31:16] (`MUL16`); CpuV3-only |
| `signal(ty: u8, value: u16)` | CpuV3 `SIGNAL` event with compile-time constant type 1..=15 (retires as a NOP in hardware; a no-op on hosts) |
| `read_cseg() -> u16` / `read_dseg() -> u16` | CpuV3-only: read the CSEG/DSEG special register (`MFSR`) |
| `dev_recv(dev: u8, ch: u8) -> u16` | read a device register; device and channel are compile-time constant IDs |
| `dev_send(dev: u8, ch: u8, v: u16)` | write a device register; device and channel are compile-time constant IDs |
| `dcache_clean_all() -> u16` | CPU V3-only: blocking full compiler memory/control barrier; write every dirty D-cache line and return final maintenance status |
| `dcache_invalidate_all() -> u16` | CPU V3-only: blocking full compiler memory/control barrier; clean and invalidate the complete D-cache, then return final maintenance status |
| `mtsr_dseg(v: u16)` | CPU V3-only: write the DSEG special register (MTSR DSEG) |
| `jseg(cseg: u16, target: u16) -> !` | CPU V3-only: atomically switch CSEG to `cseg` and jump to `target` (JSEG); never returns |
| `icache_invalidate_delayed_and_jump(cseg: u16, target: u16) -> !` | CPU V3-only: terminal barrier lowered to adjacent `ICACHE_INVALIDATE_ALL_DELAYED; JSEG`; never returns |
| `fix16::from_int(i16)` / `.to_int() -> i16` | CPU V3-only numeric conversion (`I16TOF` / `FTOI16`; `to_int` truncates toward zero) |
| `fix16::from_words(lo, hi)` / `.lo_bits() -> u16` / `.hi_bits() -> u16` | CPU V3-only raw-half moves, low half first (`ILO2F`/`IHI2F`/`FLO2I`/`FHI2I`) |
| `fix16::zero()`, `vecN::zero()` | all-zero host value (target lowering with C1) |
| `vec2/3/4::new(...)` | build a vector from fix16 lanes (consecutive scalar F registers) |
| `vec4::import(Ptr) -> vec4` / `vec4::export(v, Ptr)` | four consecutive Q16.16 values, two little-endian words each, low half first (`FLDV4`/`FSTV4`) |
| `.x()` / `.y()` / `.z()` / `.w()` | lane extraction from the consecutive range |
| `.abs() .floor() .ceil() .round() .trunc()` | component-wise unary (the scalar/vector ALU subops) |
| `fdot(a, b) -> fix16` | dot product through the 64-bit Q32.32 ACC, narrowed once (`DOT` + `DOTSTORE`) |
| `frcp(x)` / `frsqrt(x)` / `fsincos(x) -> vec2` | special functions (scalar; `fsincos` yields `{sin, cos}`); target lowering with C3, host models panic |
| `v3_length2_shift(vec3) -> u16` / `v3_length2_scaled(vec3) -> fix16` / `v3_normalize_safe(vec3) -> vec3` / `v3_distance2_gt(vec3, vec3, fix16) -> bool` | prescale library contract, not opcodes; `scaled` returns the **prescaled** squared length after component shifts and Q16.16 narrowing, and `shift` returns `k`; `scaled << 2k` is only an approximate reconstruction when `k > 0`; target lowering with C1/C2, pure reference model in the CPU V3 architecture crate |

## 6. Design decisions

- **bool is one word, 0 or 1**: the machine has no byte/bool instructions, so a stored bool costs a
  whole word — but materializing a comparison costs only the ISA's Boolean-producing form
  (`SEQ`/`SEQI`, `SLT`/`SLTU`/`SLTI`/`SLTUI`, plus an XOR for negation), so a stored bool is cheap
  enough to keep. Bools still never mix with integers: write `b as u16`, or compare (`x != 0`).
  Comparing two bools, arrays of bool, and `static` bool are not supported yet (§12).
- **u16 vs i16 matters**: signed comparisons (`cmp_s`) and arithmetic shifts are only
  produced when both operands are `i16`; mixed integer arithmetic is an error, because 
  implicit conversions hide too many bugs on a 16-bit machine.
- **Unsupported means error**: these Rust features are rejected with a span — generics,
  traits, impls and methods, closures, macros, references `&`, slices and native `[T; N]` arrays
  (§10), strings, floats, other integer types, `unsafe`, `extern`, lifetimes, items declared
  inside a function body, attributes (except the ignored `#[allow(...)]` and the `#[doc]` /
  `#[repr(...)]` / `#[derive(PartialEq)]` that §9b/§9d accept), and `use` (parsed but ignored; it
  exists for the IDE). Patterns are limited to a plain identifier or a tuple in `let` (§9c) and to
  a constant or `_` in `match` (§9d): struct patterns, bindings, guards, ranges and `|` are errors.
- **Integer `*` is supported** (hardware MUL on CpuV3, `mul_16x16` library call on CpuV2), and so
  are `/` and `%` (the rcc_std `div` module, §1.2); a zero divisor is defined there — `x / 0` is
  `0` and `x % 0` is `x` on the target — while the bare host operator still panics.
- **FPU values live in the F register file**: the FPU v2 file is `F0..F63`, one scalar Q16.16
  value per register. A `fix16` value occupies one register; `vec2`/`vec3`/`vec4` occupy a
  consecutive range of 2/3/4 scalar registers (range-aware allocation lands with C2). The FPU ABI
  reserves `F0..F3` for returns, places arguments compactly from `F4` (up to `F27` for six `vec4`
  values), allocates `F28..F62`, and keeps `F63` as the parallel-move scratch; all F registers are
  caller-saved and ACC is caller-clobbered. Instruction selection must avoid the design's
  partial-overlap case (destination range partially overlapping a source range); a shared base
  (in-place) is legal.

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

### 9.3 `static NAME: Buf<T, N> = Buf::new([e0, e1, ...]);` — global buffers

```rust
static TILE: Buf<u16, 8> = Buf::new([0x3c, 0x66, 0xc3, 0xff, 0xff, 0xc3, 0x66, 0x3c]);
```

N consecutive words in data memory (the sprite/tile/palette data of a game). Same access
rules as local buffers (§10), same `__data_init` emission for non-zero words.

### 9.4 Aggregate statics — `struct`, tuple and buffer-of-struct tables

```rust
struct Sprite { x: u16, w: u16 }

static TABLE: Buf<Sprite, 2> = Buf::new([Sprite { x: 3, w: 5 }, Sprite { x: 7, w: 9 }]);
static TITLE: Sprite = Sprite { x: 1, w: 2 };
static PAIR: (u16, u16) = (10, 20);
```

An aggregate static is a constant image in the data section, laid out by the same rules as a local
one, and its name **is its address** (like a local aggregate): `TABLE.as_array()` indexes it,
`TITLE.x` reads a field, `PAIR.0` a tuple element, and `let p: Sprite = TITLE;` copies it. Writing
one goes through a view (`TABLE.as_array()[0u16].x = 5u16`) — the usual `static` idiom, since rcc
has no `static mut`.

The initializer must be a literal of that type built from constants (`const` values are fine, calls
are not) and every field/element must be present. Aggregate statics are allocated **after** the
scalar and `Buf<u16|i16, N>` statics, so adding one never moves an existing program's data.

## 9b. Structs

```rust
#[repr(C)]
struct Inner { a: u16, b: i16 }

#[repr(align(4))]
struct Point { x: u16, y: u16, inner: Inner, flags: Buf<u16, 2>, valid: bool }
```

- Fields keep declaration order and each occupies whole 16-bit words: a scalar
  (`u16`/`i16`/`Ptr`/`bool`) one word, a `Buf<T, N>` `N * sizeof(T)` words, a nested struct its own
  size. There is no packing, because the machine has no byte accesses.
- The total size is padded up to the struct's alignment. Alignment is one word by default;
  `#[repr(align(N))]` raises it. As in Rust `N` is in *bytes*, so on this 16-bit-word target
  `align(2)`, `align(4)`, `align(8)` and `align(16)` mean 1, 2, 4 and 8 words. `#[repr(C)]` is
  accepted (declaration order already is the layout); any other attribute except `#[allow]` and
  `#[doc]` is an error. A field is padded up to its own alignment, so a nested `#[repr(align(4))]`
  field starts at an even word offset.
- A struct value **is its word address** in the frame, exactly like a buffer: `let mut p: Point =
  Point { .. };` allocates `sizeof` words through a frame slot; the literal — or another value of
  the same struct type, copied word by word — initializes it; `p.x` loads a field and `p.x = v` /
  `p.x += v` stores one; field chains (`p.inner.a`) and buffer fields (`p.flags[1u16] = v`) work.
  The binding must be `mut` for any field write, and the type annotation is required.
- A struct *name* is not a value: read a field, copy it with a typed `let`, or take its address.
- **Returning structs**: `fn make(x: u16) -> Point` writes the result through a hidden destination
  pointer the caller supplies (§14), so `let p: Point = make(1u16);` fills `p`'s own frame slot —
  no copy at the call site. `return Point { .. };`, `return other;` and `return shifted(...)` all
  work, and a struct can be assigned wholesale (`p = make(1u16);`).
  Whole-aggregate assignment evaluates the complete right-hand value before the destination
  place, using temporary frame storage before copying it back. Thus `p = swap(view_of(&p))`
  reads the old `p` throughout the call. Initialization of a new binding still uses direct sret.
- **Passing structs**: a function takes a struct by pointer, written `Array<Point>` (the one-word
  typed view). Two ways to make one:
  - `view_of(&value)` — the address of one struct (or addressable scalar) value;
  - `buf.as_array()` — the first-element address of a buffer, including a buffer of structs.
  Inside the callee, `p[i]` is the element *address* (a struct value), so `p[i].x` reads a field at
  `i * sizeof` words: a shift for word-sized elements, a real multiply otherwise. A `mut view:
  Array<Point>` parameter may write through it, which is how a callee updates the caller's struct.
- Out of scope for now (§12): `impl` methods, `fix16`/`vecN` fields, and recursive layouts.
  (`static` structs are supported since §9.4.)

## 9c. Tuples

A tuple is a small fixed group of scalars — `(u16, u16)`, `(u16, i16, u16)` — with at most four
elements, each `u16`, `i16`, `Ptr` or `bool`. Element `i` sits at word offset `i`, so a tuple is
laid out exactly like a struct whose fields happen to be unnamed.

```rust
fn divmod_pair(a: u16, b: u16) -> (u16, u16) {
    (a / b, a % b)
}

let t = divmod_pair(47u16, 5u16);   // the type is inferred from the callee
let pair: (u16, u16) = t;           // a typed `let` copies it
let (q, r) = pair;                  // and a tuple pattern destructures it
halt(t.0 + q + r);                  // fields are positional: `.0`, `.1`, ...
```

- A tuple value is memory-resident like a struct: it is its word address, a bare tuple is not a
  value (`let t = (1u16, 2u16);` is fine — the literal is stored into `t`'s slot — but passing a
  tuple *name* around needs one of the forms above).
- Tuple *parameters* are not supported (a tuple is not a view); pass the elements, or use a struct
  with an `Array<T>` view when a callee must see many of them.
- `_` may ignore an element: `let (lo, _, _) = stats(a, b);`.

## 9d. C-style enums

```rust
#[derive(PartialEq)]
enum Trace { Idle, Run, Halt }
```

A fieldless enum is one word holding the variant's discriminant (`Idle` = 0, `Run` = 1, … in
declaration order). `Trace::Run` is a compile-time constant, so it works anywhere a `u16` works:
a binding, an argument, a return value, a struct field, a `Buf<Trace, N>` element, and `e as u16`.

- Variants must be fieldless, unique and without explicit discriminants; `#[derive(PartialEq)]`,
  `#[allow]` and `#[doc]` are the only accepted attributes.
- `==` and `!=` compare two values of the *same* enum, and — like real Rust — only when the enum
  derives `PartialEq`; ordering (`<`, `>=`, …) is not defined. An integer never compares with an
  enum, and arithmetic on enums is a type error.
- `match` is a **statement** (it has no value): `match e { Trace::Idle => { .. } Trace::Run => { .. } _ => { .. } }`.
  Patterns are integer literals, enum variants and `_`; bindings, guards, ranges, `|` and nested
  patterns are errors. The value is tested once and the arms become a chain of branches.
- Exhaustiveness follows Rust: a match on an enum must list every variant or have a `_` arm, and a
  match on integers needs `_` — otherwise the same source would not build on the host.
- Out of scope for now (§12): variants with payload, explicit discriminants, `#[repr]`, enum
  statics, and casting an integer *to* an enum.
## 10. Arrays: `Buf<T, N>`

C semantics: an array is N consecutive words, addressed by a plain (single-word) pointer,
**no bounds checks** on target. The array type is `Buf<T, N>` with `T` = `u16`, `i16` or a struct.

**Native `[T; N]` is not part of the subset.** Rust arrays implement `Index<usize>` only, and a
crate cannot add `Index<u16>` to them (the orphan rule), so a word-sized index could never
type-check on the host — which is exactly the check that keeps a host run honest. `Buf<T, N>` is a
`dsl_rt` type instead, so `buf[i]` is real Rust *and* one word of addressing on the target.

### 10.1 Local buffers

```rust
let mut buf: Buf<u16, 8> = Buf::new([0; 8]);   // stack, zero-filled (or a full list)
let mut lut: Buf<u16, 3> = Buf::new([3, 5, 7]);
buf[i] = 7;                                    // u16/i16 index, no cast
let x = buf[i] + buf[3u16];
```

A buffer lives in the stack frame (a compile-time sized local area, see §11) and its initializer
is always `Buf::new([v; N])` or `Buf::new([e0, e1, ...])` — the type annotation is required.

### 10.2 Indexing and views

Index expressions must be `u16` or `i16`; give a literal an explicit suffix (`a[3u16]`, `a[-1i16]`)
so the same source also type-checks in Rust. Small literal offsets lower directly to the load/store
i4 address field, a runtime index adds registers, and a struct element scales the index by its size.

`buf.as_array()` produces `Array<T>`, a one-word typed address: the way to hand a buffer to a
function (the length is not carried at run time, and target accesses are unchecked). The same
methods exist on a buffer field, so `p.flags[1u16]` and `p.flags.as_array()` both work.

```rust
let mut storage: Buf<u16, 8> = Buf::new([0; 8]);
let mut words = storage.as_array();
words[i] = 7;
words[3u16] += 1;
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

### 10.3 Buffer methods

`Buf<T, N>` carries the methods below as inherent methods in `dsl_rt` (const generics), so they
resolve cleanly in rust-analyzer; the compiler recognizes them as intrinsics:

| method | meaning |
|---|---|
| `buf.read(i) -> u16` | `buf[i]` (u16/i16 elements only) |
| `buf.write(i, v)` | `buf[i] = v` |
| `buf.as_ptr() -> Ptr` | address of element 0 (the buffer *decays* to a pointer, like C) |
| `buf.as_array() -> Array<T>` | typed address of element 0 |
| `buf.len() -> u16` | N as a compile-time constant |

On the host these index real Rust storage — so **the host run keeps Rust's bounds check for
free**, while the target emits raw unchecked addressing (exactly the C model).

### 10.4 Buffers as parameters

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

Any local whose address is taken, and every local buffer, becomes **memory-resident**: all
its reads/writes go through frame slots (the existing `load_sp`/`store_sp` machinery).
The frontend decides residency statically by scanning for `addr_of` uses and buffer-typed
`let`s — no escape analysis. The frame layout becomes
`[callee-save saves][locals/buffers][spill slots]`, all sized at compile time.
The entry function has no caller and never returns, so it omits callee-save and return-address
saves; any locals and spills still allocate their normal frame slots.

Struct members (including buffer members) are defined in §9b; the frame layout above applies to
them unchanged (a struct local is just an address plus offsets).

## 12. Out of scope for now

`&x` references, fat slices, native `[T; N]` arrays (use `Buf<T, N>`, §10), `static mut`, heap
allocation of buffers, multi-dimensional buffers (use `arr[i * W + j]`), function
inlining/`#[inline]`, and the limits listed in §9b/§9c/§9d/§14 (aggregate parameters,
aggregate-returning fn pointers, `impl`, `fix16`/`vecN` fields, recursive layouts, enum payloads,
and enum statics).

A stored `bool` is one word, and these remain out of scope: buffers of `bool` / `Array<bool>`,
`static` bool, comparing two bools (`b1 == b2`), and an integer cast *to* bool (write `x != 0`).
A stored bool value is CPU V3-only, like the other Boolean-producing paths (§12.1).

## 12.1 Target policy

rcc targets CPU V3 only. The CpuV2 backend is frozen legacy: it still compiles the programs it
compiled before, but CpuV2 compatibility is **not** an acceptance criterion for new work — a new
frontend feature has to be correct (and tested) on CPU V3 only, no new test has to cover CpuV2, and
no effort goes into keeping the two in step. A feature that happens to work on CpuV2 is a free bonus,
not a requirement. The CpuV2 backend keeps rejecting instructions it cannot lower with a clear panic
(e.g. any FPU-class instruction).

## 14. Calling convention: scalars in registers, aggregates by pointer

Scalar arguments (including `Array<T>` views and fn pointers) travel in the six argument registers
(`r2`..`r7`) and scalar results come back in the return registers (`r0`/`r1`), with FPU values in
the F registers. Aggregates — structs (§9b), tuples (§9c) and `Buf<T, N>` (§10) — are **not**
passed by value:

- **Parameters**: an aggregate cannot be a parameter. Pass `Array<T>` (a view of the aggregate, or
  of an array of them) or the fields.
- **Returns (sret)**: the caller supplies a *hidden destination pointer* as the first argument and
  the callee writes the result there, returning nothing in registers:
  - `let p: Point = make(1u16);` allocates `p`'s frame slot and passes its address, so the callee
    fills the variable directly (no copy);
  - `let t = divmod_pair(a, b);` does the same, taking the type from the callee's signature;
  - `return Point { .. };` / `return other;` / a tail expression write into the caller's
    destination — including `return shifted(...)`, which forwards the same pointer;
  - a returned value in any other position (an argument, a field base, an operand) is an error:
    bind it with `let` first.
  Because the destination occupies the first argument register, a function returning an aggregate
  takes at most **five** declared parameters. A fn pointer cannot return an aggregate (there is no
  indirect sret), so such a function must be called directly.
- `return` in an aggregate-returning function ends with a plain `ret`; the caller sees no result
  register at all.

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
  with a location: `rN` (ABI register for params), `frame+N` (frame slot — buffers and
  address-taken locals), `global@0xADDR`, or `ssa` (register/versioned). Local entries
  also carry an inclusive lexical `scope START..END` line range; parameter values are
  captured at call entry because the ABI argument registers are caller-save;
- **globals/consts**: static names with types and data addresses, constants with values;
- **struct layouts**: `type NAME size align` followed by indented `  field offset type` lines
  (spec §9b). A debugger can use these to expand `p.x`; tuples need no table, since element `i`
  always sits at word `i`;
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
`Buf<T, N>` (const-generic word storage with `u16`/`i16` indexing), `addr_of`, and other
intrinsics. `rcc_std` is a real module tree for the same reason.

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
