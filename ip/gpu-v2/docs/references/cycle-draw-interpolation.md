# Preparation and interpolation cycle models

`vliw_cycle::prepare::Prepare` implements the production private preparation
datapath. `interpolator::Stream` connects that datapath to the actual context,
geometry-replay and `compute::Compute` owners. Raster traversal and producer-pool
admission remain external ports. These models do not render a complete DRAW.

The [frontend/producer composition](/ip/gpu-v2/docs/references/cycle-draw-producer.md)
explicitly connects the actual Raster and pool to these ports.

## Private preparation

The control reads the sole production 512-word preparation program. PA contains
the mutable unsigned17 operands in an 18-bit synchronous table and its original
fixed factors. PB36 and PC48 each have an independent writer and registered
read address driving asynchronous cells. Reset clears registers and Q, retaining
RAM contents and configuration-time constants. Unwritten portable cells retain
explicit unknown bits.

The wide MAC captures A, signedness and B on each enabled edge and produces
its result from the previous A/B and current C. It free-runs with CE; a valid
observation does not gate the primitive. The quantization pipeline preserves
the source's signed window, truncation toward zero and orientation handling.
The canonical Nx/Ny/C words share PB with later coefficient preparation.

Attributes are read in vertex order 2/0/1 and published as three S17 fields:
base2, vertex1-base2 and vertex0-base2. Normal, RGB and UV component widths
retain their selected production meanings. A nonempty context publishes all
six component rows. Empty-bound control may finish after a legal prefix and
does not publish a runtime configuration.

## Context and geometry handoff

Preparation writes the inactive coefficient and attribute pages while the old
I pipeline reads the active page. Final B rows 0/1/4 and C rows 0..3 cross into
the runtime banks; other private rows remain private. Parameter publication
uses the same inactive page. Its lifetime ends only at a later legal rewrite.

The new Raster may start when preparation reaches its nonempty context offer
and old Raster is idle, even while old I is busy. PB then replays nine geometry
words in edge-major order: Nx, Ny, C for edges 0/1/2. Runtime geometry addresses
are 0/4/8, 1/5/9 and 2/6/10. Nx/Ny are doubled; C remains unchanged. The final
replay step publishes bounds. Replay owns the single PB read address on those
edges and the original collision monitors remain active.

A pending Raster handoff blocks new quad admission and sample launch. Only
actual compute/LOD drain permits configuration commit, active-bank toggle,
RGB-mode and prepared-color installation. An empty context retires its inactive
work without acquiring Raster or runtime ownership, including while old R/I is
busy. Pool identifiers retain the production three-bit quad and two-bit lane
layout at these ports; these context models do not allocate pool slots.

## Four-phase computation

`Compute` implements the fixed phi0..3 schedule. Scalar H0/H1/H2, cover and anchor
feed the original D accumulation, shared leading window, R18 reciprocal
interpolation and RNE correction. The immutable reciprocal/log words come from
the existing generator; the same synchronous numeric RAM owns four H cells.
No second hand-maintained numeric table is introduced.

Weights use the original clipping order. Two registered pair MACs interpolate
the six components from their S17 rows, including the source's half-up base
term. Three writes per pixel retain the selected packed normal/UV/RGB/depth
format. Numerical depth rejection is a separate event from geometric coverage;
zero-D and out-of-range depth do not erase the scheduled result writes.

The analytic LOD owner preserves its fixed four-gradient calendar, two anchor
pages, coefficient read addresses, shared wide-MAC phase, normalization and log
returns. Per-quad offsets have their own writer. Result order wraps at eight
quad identities. Configuration waits for all outstanding pixel and LOD returns.
Live far, texture dimensions and maximum mip follow the command profile.

Every composition evaluates old outputs before clocking each child once.
Contract errors identify the source monitor; behavior after that edge is
unspecified. They do not add a recovery protocol or undo accepted writes.

## Qualification boundary

The prepare fixture checks canonical geometry and published attributes against
independent integer calculations. Its empty cases also scan the viewport for
covered samples. Compute checks reciprocal rounding, weights, packed results,
depth and analytic LOD using an independent integer reference. The shared
inputs are stimulus records, not candidate intermediate answers.

Explicit ignored RTL entries compare actual portable preparation RAM/DSP,
compute/LOD and the combined production interpolator on every sampled edge,
including known masks, CE stalls and context overlap. The combined fixture
uses controlled Raster and pool peers; its I busy and return events come from
the actual compute model and DUT. Compiled DUT mutations exercise numeric
RAM age, rounding, DSP C timing and context ownership. Invalid early sampling
must hit the original monitor before a trace mismatch.

Fixtures bound execution at 100000 wall edges or less. RTL processes have a
30-second limit and independent watchdog. These checks do not establish
production Raster traversal, pool storage, pixel backends, native-memory DRAW,
whole-GPU completion, physical timing or board behavior.
