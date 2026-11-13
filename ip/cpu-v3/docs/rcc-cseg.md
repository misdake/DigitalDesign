---
id: cpu-v3-rcc-cseg
status: active
last-verified: 2026-11-13
---

# RCC code segment linking

The CPU V3 backend links a whole program into independently addressed 64-Kword
(128-KiB) instruction windows. Source placement syntax belongs to the
[RCC language specification](/compiler/rcc/src/frontend/spec.md#code-placement-on-cpu-v3).
Data/static allocation and the ordinary register ABI are shared across windows.
No ISA or RTL change is required; transfers use existing JSEG.

## Layout and calls

The entry must belong to the registry's default physical CSEG. Only a bank
with an incoming far call needs a four-word return island. If the default bank
is far-called, it also needs a cold-entry jump before that island; all return
islands then start at `code_base+1`. Otherwise the default bank has no header
and every far-called bank's island starts at `code_base`. There are no padding
words. A program using logical aliases of one physical bank needs no adapters.

All functions and generated stubs participate in per-bank branch relaxation
and capacity checks. Known constant loads use the short unsigned form for 0..15.
Function-offset loads and local branches converge together across banks:
shrinking a block can move another bank's target below 16 and unlock another
short load. Widths only shrink; final emission uses the converged addresses.
A program may exceed 128 KiB in total; no individual bank may exceed its 16-bit
code offset window. Bank overflow is a compilation error.

| Transfer | Implementation |
| --- | --- |
| Same physical CSEG | Existing relaxed JALREL, ordinary r14 return |
| Other physical CSEG | JALREL to one caller-bank target stub per destination function |
| Two or more far targets in one caller bank | Target stubs prepare r12/r15 and tail-jump to one shared entry |
| One far target in a caller bank | The target stub contains its entry directly |
| Far return | Ordinary callee returns to the bank-local common return island |

The target stub loads the destination CSEG/offset into r12/r15. A shared entry
subtracts two words from SP and stores `{caller return PC, caller CSEG}`. After
saving the original r14, it reuses r14 to read and store the actual caller CSEG;
this keeps r12/r15 intact. It sets r14 to the common return offset and executes
JSEG. The target stub's tail transfer does not overwrite r14 or add a stack
frame. Sharing adds at most one local jump per far call; the single-target case avoids
that extra transfer.

The island loads the saved segment/PC into r12/r15, restores SP and executes
JSEG back. Ordinary callees keep their existing prologue/epilogue, including
r14 preservation for nested calls. Recursion and nested far calls therefore
stack ordinary two-word return records without global scratch state.

Synthetic islands, target stubs and shared entries have an unconditional
continuation, so they have no trailing HALT guard. Once block order is fixed,
a tail jump to the immediately following block is removed as proven fallthrough.
Ordinary function guards remain unchanged. The Bloxorz closed shadow-math placement uses one 8-byte
return island in CSEG6, a 12-byte shared entry in CSEG7, and three target stubs
of 6/8/6 bytes. CSEG7 has no incoming far calls and needs no header. Its total
adapter overhead is 40 bytes; the source call sites remain ordinary JALREL.

Only r12, r14 and caller-saved r15 are used by the adapter. Integer arguments
r2..r7, returns r0/r1, callee-saved registers, FPU arguments/results and all DSEG
mappings are preserved as required by the ordinary ABI. Return records consume
two additional stack words per active far call. This includes hidden aggregate
return pointers; the existing six-GPR-argument limit remains unchanged.

Function-address relocations are 16-bit near values. Address-taking is rejected
when more than one physical CSEG remains live; direct calls are supported.
CSEG placement and inlining are independent. A retained out-of-line body stays
in its declared CSEG; inline copies execute in the final caller's CSEG and keep
references to the original globals/statics. Unexpanded nested calls retain the
target body's placement and use near/far adaptation as needed. Normal dead-code
removal can discard a fully inlined body. No extra purity restriction applies.

The existing inline hints, `#[inline(never)]`, recursive/special-transfer guards,
code growth, frame and spill checks still apply. Copied code consumes the
caller's bank capacity. Use `#[inline(never)]` for functions whose behavior
requires execution in their declared bank, including placement-sensitive code.
There is no automatic partitioning or migration when a bank is full.

## Products and system boundary

`CpuV3Program.words/listing/debug` describe the default bank. `cseg` identifies
its explicit entry bank; `code_segments` holds each additional bank's CSEG,
base, words, listing and debug map. PC offsets in each debug map are local to
that bank. `total_code_words()` includes all bank headers and stubs; inline
cost selection uses this total. Unannotated programs have no added header or
segments and retain the original single-window linker path.

The Tang Nano 20K application loader starts CSEG7 at offset zero. Its application
compiler accepts additional CSEGs1..6 in Main memory, rejects CSEG0/data and GPU
or asset windows, and includes their code as normal CB06 COPY resources. Resource
packing rejects overlap. Exported `application.csegN.v3bin/.asm/.dbg` files and
`code_segments` metadata retain physical identities; ordinary repack recipes
include those exact code resources. No separate boot image array is maintained.

The package sim/emu runners execute all COPY segments. Current interactive
debugger sessions require an implicit single-CSEG program and reject explicit
placement. Root symbol/near-call inspection remains rooted in `application.dbg`;
additional bank debug/listing files carry their own CSEG identity. Far stack
unwinding is not provided by the root-only diagnostic helper.

## Qualification

Bounded checked examples cover six scalar arguments, aggregate return pointers,
fixed-point returns, nested far calls, recursion, physical aliases, file/function
precedence, stack restoration and placement errors. Nonzero code bases put
function offsets and return immediates on the 15/16 boundary; high CSEG numbers
also exercise wide constant loads. Cross-CSEG inline examples cover multiple
caller banks, shared global side effects, retained/dead out-of-line bodies and
`inline(never)` boundaries. A synthetic linked image
covers aggregate code above 128 KiB and per-bank overflow. Bloxorz fits within
the default CSEG7 and does not request additional code banks. Multi-bank linking
is exercised by the dedicated compiler examples. Functional instruction simulation
and native CPU/cache/SDRAM emulation use the same packaged program and functional GPU.
