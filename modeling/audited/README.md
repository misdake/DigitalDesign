# Audited numerical modeling

`audited` is a domain-independent fixed-point modeling tool. It has no GPU,
processor, compiler, hardware-framework or board dependency. IP models depend
on this crate; this crate never depends on an IP or a final system.

## Data boundary

`Fixed<B,F,S>` has compile-time literals but no runtime constructor, raw getter,
native arithmetic or division. Checked external integers enter an Input store
before a frame starts. Runtime values come from audited operations and typed
memory reads. Dynamic addresses and predicates preserve value provenance.
`finish()` consumes the frame before exposing host observations; `audit()`
independently replays values, control, memory and resource counts.

Numerical mode (`Model::numerical`, `compute`) records work and payload I/O
without hardware capacities or cycle estimates. Scheduled mode additionally
binds declared resources and uses an ASAP dependency schedule. Neither an audit
nor an ASAP result proves algorithm accuracy, a static RTL pipeline, area or fmax.

Products record both logical operand widths and physical lowering. Narrow
products use DSP18, wide-by-narrow products use two DSP18 operations plus a wide
reassembly adder, and wide products use DSP36. A 9-bit product is currently a
DSP18 operation; small-product packing is not modeled. Rounding includes its
guard and increment adder. Variable right shifts truncate; RNE must be explicit.
`resize_exact` checks a contract and never implements saturation.

## Source ownership

| File | Responsibility |
| --- | --- |
| `src/lib.rs` | Closed types, formats, faults and resource declarations |
| `src/arithmetic.rs` | Audited numerical operations and product lowering |
| `src/model.rs` | Stores, event ledger, external inputs and independent audit |
| `src/scheduling.rs` | Optional resource scheduling and timing checks |
| `src/tests.rs` | Framework semantic and tamper regressions |
| `examples/support/triangle.rs` | Bounded mathematical driver, outside private internals |

## Validation

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label audited -CargoArgs @('-p','audited')
& scripts/run-cargo.ps1 -Subcommand run -Label audited-driver -CargoArgs @('-p','audited','--example','audited_triangle','--','target/audited/driver')
```

The triangle example is a framework regression, not a triangle setup algorithm.
All frames have event limits; scheduled frames additionally have cycle limits.
