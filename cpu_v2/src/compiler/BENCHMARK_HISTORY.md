# Compiler benchmark history

This tracks the default optimized compiler (`opt`, function table `auto`) on
[`benchmark_suite_dsl.rs`](../dsl_progs/benchmark_suite_dsl.rs). The suite
combines broad scalar/control-flow coverage, CRC-16, and recursive in-place
quicksort. Each run starts from reset with a 20,000-cycle limit.

| Compiler stage | Instructions | Cycles |
| --- | ---: | ---: |
| Before compact direct calls (`2866118`) | 433 | 5627 |
| Compact direct calls (`d34a436`, measured at `721b706`) | 413 | 5589 |
| Conditional/immediate optimization (`e78f4b2`) | 394 | 5480 |
| SP static initialization and main prologue fix (2026-07-20 working tree) | 375 | 5461 |

Every run compiled 29 reachable functions and halted with the same signal,
`0xcc3e` (`52286`). The benchmark was added at `721b706`; its source was also
compiled with `2866118` to recover the earlier baseline.

The improvements were:

- `d34a436`: near direct calls shrink from three reserved slots to one
  `call_rel`; linker relaxation reaches a fixed point after call and branch
  sizes change. This removed 20 words and 38 cycles under `auto`.
- `e78f4b2`: encodable constants use immediate comparisons, conditions are
  inverted to prefer fallthrough, and redundant control-flow edges/default
  values are cleaned up. This removed another 19 words and 109 cycles. A bug
  found during this work now prevents unencodable constants such as `16` from
  being incorrectly rewritten to four-bit comparison immediates.
- Current working tree: static initialization uses `sp`-relative stores, and
  main no longer preserves callee-save registers or `ra` for a nonexistent
  caller. This removed another 19 words and 19 cycles.

Overall, the image fell from 433 to 375 instructions (`13.39%`) and execution
fell from 5627 to 5461 cycles (`2.95%`). `auto` selects no function-table entries
for this suite after compact near-call relaxation, because `call_abs` plus table
initialization would not be profitable.

Reproduce the current result with:

```text
target/debug/rcc cpu_v2/src/dsl_progs/benchmark_suite_dsl.rs \
  -o benchmark.bin --lst benchmark.lst
target/debug/rcc-run benchmark.bin 20000
cargo test -p cpu_v2 --test rcc_benchmarks
```
