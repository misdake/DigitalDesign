# Target resource model

This document records the resource-accounting rules for target-specific FPGA
hardware. It separates source-level ownership from the implementation produced
by synthesis and place-and-route (PnR).

## Source-level ownership

- Only modules declared as `target_leaf` may request scarce target resources.
- Higher-level modules acquire a resource by instantiating the corresponding
  target leaf. They cannot request BSRAM, DSP, PLL, or physical board IO on
  behalf of arbitrary handwritten logic.
- A target leaf declares a conservative planning cost for each supported
  configuration. Instantiating the leaf repeats that cost; deduplicating its
  emitted Verilog module definition does not deduplicate resource requests.
- Unsupported or unmeasured configurations fail instead of estimating a cost.
- Capacity is checked while generating the project. An over-capacity request
  reports the target, component, requested amount, and remaining capacity and
  stops generation.

These rules answer which source construct is allowed to consume a resource.
They do not require the synthesized primitive to remain below the same HDL
hierarchy path.

## Physical implementation

Synthesis and PnR may eliminate unused leaves, merge equivalent logic, share
operators, fuse DSP stages, or move primitives across hierarchy boundaries.
Therefore per-instance primitive ownership is not a general post-synthesis
invariant.

Normal projects use this rule:

```text
normalized physical usage <= total source-level request
```

The check uses the PnR report, because PnR is the authority for fitted physical
resources. A zero request consequently requires zero physical usage. A report
containing an unknown DSP implementation mode fails closed until that mode has
an explicit normalization rule.

The generated `resource-report.txt` contains planning values. Gowin's PnR
report contains the actual implementation. They intentionally remain separate:
the planning report must not pretend to predict optimization decisions.

BSRAM currently retains a stricter synthesis-hierarchy audit because the
supported memory leaves map directly to measured block primitives. This can be
relaxed to aggregate auditing if a real optimization demonstrates legitimate
cross-leaf memory merging.

## Characterization projects

Small projects used to characterize a target leaf may add physical expectations
for Gowin DSP modes. An expectation can require an exact count, an upper bound,
or an inclusive range. These assertions detect an inference or packing change
in a deliberately controlled project; they are not added to normal application
projects.

For example, the DSP board self-test fixes the expected counts of `PADD18`,
`MULT18X18`, `MULTADDALU18X18`, and `ALU54D` after PnR. Its logical leaf claims
still use normalized 18x18 multiplier lanes, so target capacity accounting does
not expose vendor primitive names to ordinary modules.

Characterization evidence should include:

1. shared functional vectors for emulation and Verilog simulation;
2. successful synthesis, timing audit, and PnR resource report;
3. a board self-test when the primitive's hardware behavior or tool inference
   has not already been established.

Once a bottom target leaf is characterized, higher-level emulation and NAND
composition normally do not need repeated board testing unless they add new
handwritten Verilog, clocks, IO behavior, or timing assumptions.

## DSP evolution

The current normalized unit is one signed 18x18 multiplier lane. Composite
leaves request the measured number of lanes. This is deliberately narrower than
a universal DSP formula: future 9x9 packing, ALU54 operations, cascade chains,
rounding, saturation, or cross-instance merging may need different units or a
small vector of topology constraints.

When those modes are added:

- keep the logical request conservative and configuration-specific;
- record the measured primitive shape in a characterization project;
- compare large projects using aggregate fitted usage, not exact primitive
  identity or hierarchy;
- preserve raw PnR mode counts in diagnostics so regressions remain explainable.

Constant shifts are wiring and should not request a DSP. Variable shifts remain
fabric logic unless a measured target leaf intentionally implements a vendor
DSP shift/round mode. Resource accounting follows the selected target leaf, not
the surface Rust operator.

## Whole-device resources

Fitted SDRAM and flash are exposed as complete devices, not divisible capacity
balances. A target can grant each device once. Their bit capacities are target
metadata used for configuration and diagnostics. BSRAM and DSP resources remain
divisible physical FPGA resources whose concrete leaf configurations determine
the requested block or lane count.
