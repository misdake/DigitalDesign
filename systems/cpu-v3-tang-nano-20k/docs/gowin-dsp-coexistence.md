# GW2AR-18 DSP macro coexistence rules

Measured 2026-10-01 on `GW2AR-LV18QN88C8/I7` (Tang Nano 20K) with Gowin V1.9.8.11
Education. This document records how the synthesizer and placer pack DSP primitives into
physical DSP macros, because that packing determines the true cost of any datapath that
mixes multipliers, pre-adders, and the 54-bit ALU. It is a device characterization, not a
design document; consumers (GPU setup, CPU datapaths) reference these rules for resource
budgets.

## Why measurement was required

The placer packs primitives into macros only under utilization pressure, so a small build
shows an arbitrary spread, not the minimum packing (an earlier experiment observed
MULT18X18+ALU54D "sharing a macro" that was in fact two adjacent macros in one tile).
Each case below is therefore replicated N times so DSP usage approaches the device limit,
forcing the true minimum packing. Where pressure alone is inconclusive, a discriminant
build is used: a size that fits only if the primitives share macros, so PnR success or
failure (RP0002 tile precheck, PR0003 placement overflow) is itself the verdict. Hard-cell
audit confirms independent multipliers are never folded into fused primitives. All cells
share one clock (except c17), CE=1 (except c12), sync reset (except c13), signed inputs,
and full pipeline registers. All successful builds pass timing at 54 MHz.

## Measured device anatomy

- 48 18x18 multiplier slots = **24 DSP macros** = **12 DSP tiles** (two rows R19/R37,
  six columns), each tile holding macro `[0]` and `[1]`.
- A macro `RxCy[n]` has four lettered sub-slots `[A]`–`[D]` plus one unlettered ALU
  position:
  - MULT18X18 and PADD18 occupy `[A]` or `[B]` (full 18-wide slot);
  - MULT9X9 occupies any of `[A]`–`[D]` (half slot, four per macro);
  - an independent ALU54D occupies the whole macro (unlettered position);
  - fused primitives (MULTADDALU18X18, MULTALU18X18, MULTALU36X18) are placed as one
    whole macro;
  - MULT36X36 is placed as a whole tile (no `[n]`).

## Coexistence rules

**Can share one macro:**

- **Two independent MULT18X18** (`[A]`+`[B]`), with **per-slot control sets**: different
  CLK, different CE, or different reset mode (SYNC/ASYNC) does not prevent packing
  (c02/c12/c13/c17 all reach the theoretical 1.00 macro/instance minimum).
- **Four MULT9X9** (`[A]`–`[D]`), only with other 9x9 cells (c05).
- **Two PADD18** (`[A]`+`[B]`) — but a standalone PADD18 still costs its macro slot;
  it is not a free adder (c10).
- **Fused primitives**, exactly one macro each: MULTADDALU18X18 (two 18x18 products
  summed/subtracted), MULTALU36X18 (36x18 product with ALU, internally two 18x18
  multipliers hard-wired to the ALU54), MULTALU18X18 (one 18x18 with ALU; the second
  multiplier slot is wasted) (c09/c14/c15).

**Cannot share (strong coupling / mutual exclusion):**

- **An independent ALU54D never shares its macro with anything**, even when both
  multiplier sub-slots are free (c03 measures 1.50 macro/instance; the c03_n20
  discriminant build fails PnR at a size that would fit if sharing were possible).
  Consequently **2x MULT18X18 + ALU54D costs a minimum of 2 macros** (c04, discriminant
  c04_n13/c04_n14 fail). The only way to combine multiply and ALU in one macro is a
  fused primitive, whose ALU inputs come from the macro-internal multiplier/pre-adder
  paths, not from fabric.
- **MULT9X9 and MULT18X18 never mix** in a macro (c18 discriminant: separate placement
  needs 25 macros > 24 and fails; mixed placement would fit exactly 24).
- **PADD18 and MULT18X18 never mix**: they compete for the same `[A]`/`[B]` slot pool
  and are placed in separate macros even at 100% utilization pressure (c11_n12).
- **MULT36X36 occupies a whole tile** and excludes an independent ALU from that tile
  (c08 measures 3.00 macro/instance; c08_n9 discriminant fails).

**The MULT36X18 primitive does not exist** on this device family (UG287 documents only
MULT36X36 among wide multipliers). A 36x18 multiply should be built as MULTALU36X18
(1 macro, and its ALU is available for the following add or accumulate), never as
MULT36X36 (2 macros / 1 tile, and blocks the ALU).

## Budget counting model

PnR precheck RP0002 and the `DSP | xx%` report line both count **tiles** (12 total) with
kind-separated accounting. A design's macro demand can be computed directly:

```
macros = ceil(#MULT18X18 / 2) + ceil(#MULT9X9 / 4) + ceil(#PADD18 / 2)
       + 1 per independent ALU54D
       + 1 per fused primitive (MULTADDALU18X18 / MULTALU18X18 / MULTALU36X18)
       + 2 per MULT36X36 (one whole tile)
tiles  = ceil(macros / 2)   must be <= 12
```

## Case summary

macro/instance = distinct macro sites in the post-place file / N (MULT36X36 tile sites
count as 2 macros). Discriminant builds are listed only by their verdict.

| Case | Per-instance content | macro/instance | Verdict |
| --- | --- | ---: | --- |
| c01 | 1x MULT18X18 | 0.53 | multipliers pair up (0.5 theoretical, one odd cell) |
| c02 | 2x MULT18X18 | 1.00 | two independent multipliers share a macro |
| c03 | 1x MULT18X18 + 1x ALU54D | 1.50 | ALU owns a macro; multipliers pair; **no sharing** |
| c04 | 2x MULT18X18 + 1x ALU54D | 2.00 | multiplier pair in one macro, ALU alone in another |
| c05 | 4x MULT9X9 | 1.00 | four 9x9 share a macro |
| c06 | 1x MULT18X18 + 2x MULT9X9 | 1.00 | each kind in its own macro(s) |
| c07 | 1x MULT36X36 | 2.00 | whole tile |
| c08 | 1x MULT36X36 + 1x ALU54D | 3.00 | ALU must go to another tile |
| c09 | 1x MULTADDALU18X18 | 1.00 | fused primitive baseline |
| c10 | 2x PADD18 | 1.00 | standalone PADD occupies macro slots |
| c11 | 2x MULT18X18 + 2x PADD18 | 2.00 | kinds never mix, even at 100% pressure |
| c12 | 2x MULT18X18, different CE | 1.00 | control sets are per-slot |
| c13 | 2x MULT18X18, SYNC vs ASYNC reset | 1.00 | control sets are per-slot |
| c14 | 1x MULTALU36X18 | 1.00 | fused 36x18 + ALU |
| c15 | 1x MULTALU18X18 | 1.00 | fused 18x18 + ALU (one multiplier slot wasted) |
| c16 | 3x MULT18X18 + 2x MULT9X9 | 2.11 | matches kind-separated minimum |
| c17 | 2x MULT18X18, different CLK | 1.00 | control sets are per-slot |
| c18 | 15x MULT18X18 + 6x MULT9X9 | PnR fail | discriminant: 9x9 and 18x18 never mix |

## Implications for datapath design

- Multiply-add / multiply-accumulate chains should be written as fused primitives
  (MULTADDALU18X18, MULTALU36X18) instead of independent MULT + ALU54D pairs, which
  always cost one extra macro.
- A difference of two products (e.g., 2D cross terms) is the natural shape of
  MULTADDALU18X18; a 36-bit-operand multiply followed by an add is the natural shape of
  MULTALU36X18.
- Independent ALU54D instances should be avoided in budgeted datapaths; each costs a
  whole macro and shares with nothing.
- Pre-adders are not free: a standalone PADD18 occupies the same slot pool as an 18x18
  multiplier. Fabric shift-adds remain the right choice for constant multiplies.
- Static pipeline scheduling faces no control-set packing hazards: two multipliers in
  one macro keep independent CLK/CE/RESET.

## Evidence boundary and reproduction

Conclusions are synthesis + PnR evidence on the stated device/tool version; they describe
packing behavior, not on-board function, so no board run is applicable. Raw artifacts,
all 18 case modules, the LFSR/checksum anti-optimization harness, build scripts, and the
full case log live in the `gowin-dsp-coexistence` experiment workspace (an untracked
local working area outside this repository; rerun: `scripts/gen_builds.py`, then
`scripts/run_all.sh`, then `scripts/parse_results.py`; full narrative in its
`results.md`). If the tool version changes, rerun before relying on the packing rules.
