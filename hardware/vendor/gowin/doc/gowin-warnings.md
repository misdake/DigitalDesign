---
id: gowin-warning-audit
status: implemented
last-verified: 2026-11-14
---

# Gowin warning audit

Both project exports enable all synthesis and place-and-route warnings. Each
exported Verilog compilation unit defines `SYNTHESIS`; simulation exports keep
their contract monitors. The exported IDE process configuration carries the
same top module, warning switches and supported board process options as Tcl.

Builds preserve the unedited vendor logs and `gowin-console.log`. Classification
deduplicates repeated console/log lines, writes `gowin-warnings.json`, and prints
the unacknowledged diagnostics and one summary. A new build with an unknown
warning or incomplete capture fails qualification and receives no fit seal.

The report includes all original diagnostics, stage/line locations, recognition
reasons, per-reason counts, and SHA-256 identities of its input files. Complete
capture requires completed stage logs, the actual synthesis `.prj` with its
all-warning option, a PnR command containing `-warning_all`, and a nonempty
post-route `.vo`. Missing or unfinished inputs are an explicit incomplete-capture
diagnostic. Console-only warnings are also collected.

Existing-artifact inspection recomputes the audit and returns the summary and
unacknowledged list in its `warnings` field. The detailed thousands of accepted
primitive outputs remain in the report, rather than the ordinary CLI summary.
Older fits remain readable and expose their incomplete or unacknowledged warning
coverage; their prior timing/resource qualification is not a warning-clean claim.
Fit version 2 binds the logs, PnR command, mapped `.vo`, console and warning report
alongside the original source, timing, resource and bitstream identities.

```powershell
cargo run --release -p digital-design-hardware-gowin --bin gowin-artifact -- target/hardware
```

An IDE build or unsealed experiment can also be checked directly:

```powershell
cargo run --release -p digital-design-hardware-gowin --bin gowin-artifact -- target/hardware --warnings-only cpu_v3_system
```

This mode prints a compact JSON summary, exits unsuccessfully for unknown or
incomplete diagnostics, and neither seals nor programs hardware. It needs no
Gowin installation. Its project argument is the generated output base name.

## Recognized implementation outputs

The classifier resolves each PA1001 source instance and net bit to an actual
mapped primitive port. A similar-looking name without that connection stays
unacknowledged. Ambiguous flattened instances must agree on the justification.

| Report reason | Justification and recognition boundary |
| --- | --- |
| `dsp-unused-shift-output` | MULT9X9/MULT18X18/MULTADDALU18X18 SOA/SOB shift-chain outputs; only synthesized SOA/SOB stubs, with the reported bit in the mapped connection. |
| `dsp-unused-cascade-output` | MULTALU36X18/MULTADDALU18X18/ALU54D CASO; only mapped cascade stubs. The instance has no downstream cascade consumer. |
| `dsp-unselected-result-bits` | Unselected DOUT bits of the listed multiplier/MAC/ALU primitives, connected to tool-generated DOUT stubs. These are the physical product's discarded precision bits; application-named result wires require their own review. |
| `memory-physical-output-padding` | Unused DO/DOA/DOB bits of native block/distributed RAM and ROM primitives, connected to generated output stubs. Physical port granularity exceeds the selected logical word. |
| `alu-carry-only-stage` | An unused ALU SUM with an actually connected COUT consumer. Comparisons, carries and selected arithmetic slices can need the carry without the sum. An isolated ALU output is not accepted by this rule. |
| `iodelay-unused-adjustment-status` | IODELAY DF adjustment-limit status; only the mapped generated DF stub. The selected fixed-delay design consumes DO data and does not perform dynamic adjustment. |
| `pll-explicitly-unused-output` | rPLL CLKOUTP/CLKOUTD/CLKOUTD3 connected to the four explicitly named unused PLL outputs. Used clocks and unrelated dangling nets remain visible. |
| `preserved-register-unused-shadow` | Gowin's unused reset-free DFF shadow of a preserved DFFS. Recognition requires the exact `_s0`/`_s1` pair in one hierarchy, matching D and CLK, the generated `_1_Q` stub, and a consumed live DFFS Q. The two input synchronizer stages and preservation attributes stay intact. |

PR1014 has one structural exception, `board-pll-reference-only`. In the mapped
design, `clk_d` must be driven by `clk_ibuf` from the board `clk`, and its only
consumers must be exactly two rPLL CLKIN ports. This is the 27 MHz oscillator
reference, not the generated fabric clock distribution. The PLLs regenerate
their output clocks; reference-path inter-PLL skew is not a fabric setup/hold
path. Adding a fabric clock consumer, changing the driver, or changing the
diagnostic prevents recognition. Generated-clock and I/O timing audits remain
mandatory. This exception makes no claim about unrelated physical noise.

## Source-owned exceptions

Exceptions for logical fields and intentional optimizer specialization live
beside their owner. Each requires an English reason:

```verilog
// Exact source-line exception, for this diagnostic code:
some_reader r(...); // gowin-lint: allow NL0002 -- Optional publication payload has no parent consumer.

// NL0001 also requires explicit instance suffixes, with bounded inclusive ranges:
RAM16SDP4 a(...); // gowin-lint: allow NL0001 instances=reader/lane[1:4].a -- This reader consumes only the first metadata nibble.

// Exact logical net names, scoped to the containing module:
// gowin-lint: unused PA1001 count[0],count[1] -- Only the upper occupancy threshold is consumed here.
```

The current quad-pool owner records its optional occupancy low bits, unused
publication read views, and the exact unconsumed nibbles in each specialized
reader. The triangle-descriptor owner records lanes 5..7: descriptor bits 20..31
have no consumers in the selected command parent. Vertex references, sign,
attribute mode and the separate epoch remain connected. These are bounded source annotations, not
whole-module or whole-warning-code exclusions. A new lane, instance, bit or
diagnostic outside the recorded scope remains unacknowledged.

Source-line locators are resolved inside the exported snapshot, including when
an artifact has moved. Missing sources, empty reasons and out-of-scope paths
cannot acknowledge a warning. Changing policy requires code review; no build
automatically accepts its new warning list.
