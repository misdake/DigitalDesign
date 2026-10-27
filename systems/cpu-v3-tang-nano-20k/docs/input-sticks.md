# Calibrated stick input

`rcc/stick.rs` processes eight-bit axes independently of Device5 or a game. The
DS2 driver retains the original 0..255 bytes; callers explicitly pass connection
validity. Positive X points right and positive Y points down.

## Configuration and results

`StickConfig` contains `min_x/center_x/max_x`, `min_y/center_y/max_y`, and mapped radial
`inner/outer` thresholds. Each axis must satisfy `min < center < max <= 255`;
thresholds must satisfy `0 <= inner < outer <= 256`. `stick_default()` selects
0/128/255 on each axis and inner24/outer256. Configuration thresholds use Q8:
256 represents 1.0, so the default inner dead zone is 9.375% of full radius.
Result components use the native FPU Q16.16 `fix32` type, with 1.0 representing
full travel; there is no intermediate Q8 result quantization.

`stick_prepare(config)` validates configuration and returns a `StickPrepared`
snapshot. It stores endpoint/center values, four refined FPU axis-span
reciprocals, the FPU inner threshold and refined range reciprocal. Rebuild it
explicitly after changing calibration or dead-zone settings. A rejected
configuration produces `valid=0`; it cannot become valid without rebuilding.
Changing the source `StickConfig` does not change an existing snapshot.

`stick_read(prepared, raw_x, raw_y, connected)` returns `StickSample`:

| Field | Meaning |
|---|---|
| `cal_x`, `cal_y` | Signed calibrated components before square-ray processing, each approximately -1..1 |
| `x`, `y` | Signed motion vector after square-ray mapping and dead-zone rescaling |
| `dir_x`, `dir_y` | Approximately unit-length direction outside the dead zone |
| `magnitude` | Processed strength, 0..1 (`fix32`) |
| `valid` | 1 for connected, valid configuration and eight-bit inputs; otherwise 0 |

Invalid or disconnected input returns all zeros. Valid input inside the inclusive
inner dead zone retains its calibrated components and validity, but motion,
direction and strength are zero. Outside the mapped dead zone, direction is
available even for very small processed strength. All seven numeric fields retain
native FPU precision; `valid` alone is a `u16`. Games consume these values directly.
Only presentation converts to integers: the diagnostic quantizes final pixel
coordinates and formats its numeric labels. There is no Q8 conversion helper.

`stick_set_center(config, x, y)` explicitly updates both centers, only when the
configuration and both new centers are valid; rejection changes neither axis.
There is no startup calibration, automatic endpoint learning or persistence.
Applications may assign measured endpoints directly and check
`stick_config_valid` before use.

## Radial processing

Below center, an axis uses `(raw-center)/(center-min)`; above center it uses
`(raw-center)/(max-center)`. Values outside calibrated endpoints clamp to -1/+1.
The asymmetric spans preserve exact center and endpoints without discarding the
positive endpoint of an eight-bit axis.

For calibrated vector `v=(x,y)`, let `r=length(v)` and
`t=max(abs(x),abs(y))`. The ray from the center through `v` meets the unit
square at distance `r/t`; the fraction of that travel already covered is
`r/(r/t)=t`. For `t > inner`:

```text
direction = v / r
strength  = clamp((t - inner) / (outer - inner), 0, 1)
motion    = direction * strength
```

With inner0/outer1, each ray's square boundary maps to the circle, angles remain
unchanged, and every fractional position along that ray preserves its strength.
A half-travel diagonal is half strength; it does not saturate early as it would
under an uncorrected Euclidean-radius clamp. Inner/outer are radii *after* this
square-to-disc mapping. The inclusive inner dead zone is therefore
`max(abs(x),abs(y)) <= inner` in the calibrated square. A reduced outer threshold
intentionally reaches full strength early along every ray, not just diagonals.

Calibrated components retain the raw square envelope for diagnostics. There is
no smoothing, axis snapping or independent per-axis dead zone. Computation uses
existing CPU Q16.16 arithmetic. Reciprocal Newton refinement runs only during
configuration preparation. The sample path uses the existing RSQRT seed directly,
with a two-lane DOTSTORE and vector multiply, without a Newton correction or
per-sample1.5/0.5 constants. Before the length calculation, both calibrated components are
multiplied by64: this leaves the normalized direction unchanged and avoids losing
small squared lengths in Q16.16. The largest squared length is8192, within range.
Finite precision means components need not lie exactly on the ideal unit circle.
Independent floating-point checks cover2304 samples across axial travel, a
two-dimensional grid, asymmetric calibration and reduced outer travel, plus
explicit fractional-ray cases; tolerance is1/128 per component. A separate check
requires fractional result bits below the former Q8 precision to survive further
FPU arithmetic. An exhaustive compiled sweep also covers all65536 default axis
pairs with a zero dead zone, checking the same component error and unit-disc
bounds, including the shortest nonzero vectors.

The valid `stick_read` path stays in one function and returns a struct literal
directly into the caller's destination. It checks connection, raw-byte range
and prepared validity; endpoint coefficients are loaded only for interior,
noncentered axes. Scalar ABS avoids branch-and-copy absolute values. The raw
configuration is not revalidated and no reciprocal is computed per sample.
There are no hot-path helper calls or intermediate aggregate copies. Runtime
`fix32` fields occupy two16-bit words and use lossless FLD/FST access. Adjacent
full-vector lane results can use one FSTV; storing a result does not quantize it.
Prepared storage is19words and each sample is15words. The caller owns both
lifetimes; preparation belongs to settings changes, not to every frame.

## Optional digital direction

`stick_cardinal(sample, previous, enter, release)` returns 0 neutral, 1 right,
2 down, 3 left or 4 up. Thresholds are `fix32` processed strength and require
`0 <= release < enter <= 1`. At or below release it returns neutral; below
enter it retains a valid previous direction; at or above enter it selects the
dominant direction component, with X winning equal-component ties. Invalid input
or thresholds return neutral. The caller owns the previous value. This is
activation hysteresis, without angular hysteresis or repeat timing.

## Game input capture and consumption

`rcc/gamepad.rs` is the single-consumer game facade. It owns the Device5
state/scratch storage and two independently prepared stick caches; the game
does not need raw channel numbers, raw array indices or polling loops.

| Call | Contract |
|---|---|
| `gamepad_init()` | Reset software input/edge state and install default calibration for both sticks; no hardware transaction or wait |
| `gamepad_capture()` | Make one bounded coherent raw snapshot attempt and accumulate observed edges; true means a complete new snapshot was published; no numerical conversion |
| `gamepad_take_frame()` | Resolve pending stick results, copy a `GamepadFrame` and clear accumulated edges; no Device5 read |
| `gamepad_configure(which, config)` | Set left=0 or right=1 calibration; validate first, rebuild Prepared and invalidate that result; rejected changes preserve the previous configuration/result |
| `gamepad_direction(which)` | Resolve that stick if pending, then return its cached `vec2` direction |
| `gamepad_strength(which)` | Resolve that stick if pending, then return cached Q16.16 strength |
| `gamepad_stick_valid(which)` | Resolve that stick if pending, then report valid analog input |

`GamepadFrame` occupies thirteen words: `connected`, `id`, active-high `buttons`,
`pressed`, `released`, then `left_x`, `left_y`, `right_x`, `right_y` as native
two-word `fix32` values. Returned frames are independent snapshots; a later
capture does not change an already returned frame. Optional direction/strength
queries resolve the current captured input, so read them before another capture or configuration change when
they must accompany a particular frame. Invalid optional stick indices return
zero/false. Before initialization capture/configure return false and a frame
is neutral.

```rust
gamepad_init();
// Once per bounded input-service opportunity:
gamepad_capture();
// Once per game tick, after the desired service opportunity:
let input = gamepad_take_frame();
```

Extra captures cannot erase unconsumed observed edges. Taking a second frame
without capture or reconfiguration returns the same held buttons/motion with zero edges. A false
capture result does not mean no input: games continue integrating held motion
with the elapsed time of every tick. A torn snapshot retains the previous
coherent state and caches; disconnect clears buttons/edges and analog motion.
Digital41 remains connected with usable buttons but zero, invalid analog sticks,
even when custom analog calibration has a center other than128. Reconnect
establishes a fresh button baseline. There is no hidden sleep, retry loop or
software serial protocol.

Each cache contains a nineteen-word Prepared snapshot, eleven result words
(motion, direction, strength and validity), a packed raw X/Y byte pair and a
key state (uninitialized/disabled/analog): exactly thirty-two words. Input-owned
static storage totals84 words, excluding caller frames and call-stack temporaries.
A fresh hardware sample with unchanged raw/analog keys reuses the corresponding
result; the two sticks refresh independently. Capture compares keys and records
pending bits in the existing initialized-state word. Multiple different captures
before consumption therefore calculate only the final requested input. Returning
to the already installed key cancels the intermediate pending change. The first
frame or derived query resolves each pending stick once; later queries reuse it.
Configuration changes invalidate that stick's key, so its first consumer sees
the new configuration even without a new hardware sample. Editing the source
configuration later does not mutate the prepared snapshot. No caller-side
refresh/synchronization call is needed.

A cache miss reuses `stick_read` and its unchanged arithmetic/order. Its local
Sample is reduced to the eleven adjacent result words with a fixed native-word
copy, avoiding a second FLD/FST round trip and retaining all fractional bits.
The diagnostic calibrated X/Y values remain available through the original
`stick_read`; games do not retain or copy them every tick. Cache hits avoid the
numeric kernel; consumed misses pay the cache installation and key bookkeeping in
addition to conversion. This is a reuse tradeoff, not a guarantee that every
changing-input call costs less than direct conversion. Initialization and
explicit reconfiguration also remain cold paths.

The [circular DS2 diagnostic](/systems/cpu-v3-tang-nano-20k/examples/dualshock2_ui/README.md) demonstrates
raw versus processed travel and explicit center/dead-zone adjustment. It does
not assign game actions to this shared helper.
