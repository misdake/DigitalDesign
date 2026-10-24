# Calibrated stick input

`rcc/stick.rs` processes eight-bit axes independently of Device5 or a game. The
DS2 driver retains the original 0..255 bytes; callers explicitly pass connection
validity. Positive X points right and positive Y points down.

## Configuration and results

`StickConfig` contains `min_x/center_x/max_x`, `min_y/center_y/max_y`, and radial
`inner/outer` thresholds. Each axis must satisfy `min < center < max <= 255`;
thresholds must satisfy `0 <= inner < outer <= 256`. `stick_default()` selects
0/128/255 on each axis and inner24/outer256. Thresholds and results use Q8:
256 represents 1.0, so the default inner dead zone is 9.375% of full radius.

`stick_read(config, raw_x, raw_y, connected)` returns `StickSample`:

| Field | Meaning |
|---|---|
| `cal_x`, `cal_y` | Signed calibrated components before radial processing, each -256..256 |
| `x`, `y` | Signed motion vector after dead zone, rescaling and radial limiting |
| `dir_x`, `dir_y` | Approximately unit-length direction outside the dead zone |
| `magnitude` | Processed strength, 0..256 |
| `valid` | 1 for connected, valid configuration and eight-bit inputs; otherwise 0 |

Invalid or disconnected input returns all zeros. Valid input inside the inclusive
inner dead zone retains its calibrated components and validity, but motion,
direction and strength are zero. Outside the geometric dead zone, direction is
available even if a very small processed strength rounds to zero in Q8.
`stick_fixed(component)` converts a Q8 value to the CPU's Q16.16 `fix32` type.

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

For calibrated vector `v`, radius `r=length(v)` and `r > inner`:

```text
direction = v / r
strength  = clamp((r - inner) / (outer - inner), 0, 1)
motion    = direction * strength
```

Computation uses existing CPU Q16.16 arithmetic, reciprocal and reciprocal square
root instructions with one Newton refinement, then rounds/clamps Q8 results.
These are finite-precision vectors; rounded components need not lie exactly on
the mathematical unit circle. The independent numeric tests cover 2304 samples
over full axial travel, a two-dimensional grid, asymmetric calibration and a
reduced outer threshold, allowing two Q8 codes per component.

A square-shaped raw travel envelope remains visible in calibrated components.
Only motion length saturates at the outer radius; angles are preserved. There is
no square-to-disc warp, smoothing, axis snapping or independent per-axis dead
zone. The radial dead-zone/rescaling pattern also appears in Microsoft's
[XInput guidance](https://learn.microsoft.com/en-us/windows/win32/xinput/getting-started-with-xinput#dead-zone);
the thresholds here are this software's configuration, not XInput constants.

## Optional digital direction

`stick_cardinal(sample, previous, enter, release)` returns 0 neutral, 1 right,
2 down, 3 left or 4 up. Thresholds use processed Q8 strength and require
`0 <= release < enter <= 256`. At or below release it returns neutral; below
enter it retains a valid previous direction; at or above enter it selects the
dominant direction component, with X winning equal-component ties. Invalid input
or thresholds return neutral. The caller owns the previous value. This is
activation hysteresis, without angular hysteresis or repeat timing.

The [circular DS2 diagnostic](../examples/dualshock2_ui/README.md) demonstrates
raw versus processed travel and explicit center/dead-zone adjustment. It does
not assign game actions to this shared helper.
