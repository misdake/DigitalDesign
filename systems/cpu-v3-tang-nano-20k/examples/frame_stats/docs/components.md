# Frame statistics helper

Timestamp access, resolution, reset and wrap rules are defined by the
[system wall-clock contract](/systems/cpu-v3-tang-nano-20k/docs/wall-clock.md).
This example owns only the frame-interval state and character formatting.

## RCC state and characters

`frame_stats_reset` clears four words. `frame_stats_begin(state, now, completed)`
records the current frame-loop start and makes the preceding interval valid
only if a prior start exists and `completed` is true. Callers supply false
after an error, timeout, or any interval whose duration bound is unknown.
Failed intervals do not retain an old number that could look current.

At each new frame-loop start, `completed` describes the preceding frame's
successful GPU completion **and actual display ownership transfer**. Thus the
interval includes previous-frame CPU scene construction, GPU execution, UI,
presentation wait, and intervening loop overhead. It is total frame time,
not GPU-only time. The current frame displays the previous interval. The
first frame displays a placeholder. This component does not infer completion
from busy becoming zero and cannot itself validate presentation or a deadline.

`frame_stats_digits` writes five ASCII-compatible glyph IDs into five u16
words: `00000` through `65535`, or `-----` when invalid. Conversion uses at most
33 subtractions and no multiplication or division. The call uses a fixed
8-byte state and 10-byte output; neither needs a heap. An atlas must explicitly
map these IDs to glyphs; numeric IDs alone do not imply a built-in font.

The example runs these helpers as compiled CPU V3 code with the default RCC
optimizer. It checks first-frame invalidity, failed-frame invalidity, recovery,
zero duration, wrap, decimal boundaries, 65535 ms, and 128 deterministic inputs.
These tests do not exercise GPU UI, display, full-system synthesis, or physical
hardware. The example emits five IDs but does not own an atlas or UI grid.
