---
id: cpu-v3-cpu-canvas
status: implemented
last-verified: 2026-11-10
---

# CPU canvas

`rcc/cpu_canvas.rs` is an ordinary software library for CPU drawing into the
20K system's fixed 400×240 RGB565 column16 framebuffer. It does not issue GPU
commands, present a frame, allocate memory or keep a CPU framebuffer mirror.

`unsafe { canvas_from_sectors(first, count) }` binds an existing plane
of at least 1600 contiguous 128-byte sectors. It has no GPU library dependency
or associated driver globals. The caller guarantees exclusive CPU write ownership:
no pending GPU work or Display reader may access that plane during drawing.
This raw-memory API deliberately does not derive an address from a logical
pointer or a `ColorBuffer` handle. A/B memory comes from the same system layout
as the corresponding GPU resources.

Drawing functions take the small copyable canvas as their first argument:

| Function | Behavior |
| --- | --- |
| `canvas_span(canvas, left, right, y, color)` | Horizontal half-open span clipped to the visible viewport |
| `canvas_pixel(canvas, x, y, color)` | One visible RGB565 pixel; outside coordinates are ignored |
| `canvas_rect(canvas, x, y, width, height, color)` | Clipped spans; width 0..400, height 0..240, origin within one viewport outside the visible area |
| `canvas_line(canvas, x, y, end_x, end_y, color)` | Integer Bresenham line; both endpoints must be visible |
| `canvas_circle(canvas, cx, cy, radius, color)` | Midpoint outline, visible center and radius 0..200; individual pixels clip |

Arguments outside these ranges do not draw. Shape calculations fit signed
16-bit arithmetic. Rasterization works on visible pixels only; it never writes
the framebuffer's 16 padded rows. Each span splits at column16 boundaries,
maps its physical sectors through DSEG3, waits for outstanding local-copy work
before changing the window and restores the previous mapping on return.
Stack/static mappings stay intact. Cached stores are not automatically
published to other devices by these drawing functions.

The core demo uses this explicit sequence:

```mermaid
sequenceDiagram
    participant GPU
    participant CPU
    participant Display
    CPU->>GPU: Clear completed non-Front Back
    GPU-->>CPU: wait returns; GPU writes visible, CPU cache invalidated
    CPU->>CPU: Draw canvas pixels and update UI grid
    CPU->>GPU: submit publishes CPU stores; overlay with ColorLoad::load
    GPU-->>CPU: wait returns; completed pixels visible
    CPU->>Display: Queue completed Back and pixel format
    Display-->>CPU: Confirm swap before old Front can be reused
```

The diagram's completion arrows are ownership boundaries. The next `Gpu::submit`
publishes CPU canvas stores through the existing full-cache clean. Callers
presenting CPU-only content must perform the equivalent cache publication
before handing memory to Display. The canvas itself never adds hidden waits
for GPU or Display ownership.

The unsafe constructor requires RCC checked compilation. The normal core-demo
recipe already uses that path; the standalone canvas boundary test supplies
`RustcCheck` without approving or importing any GPU resource library.
