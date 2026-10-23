# Static native scene

The example has a minimal 400x240 RGB scene: eight floor tiles on a 3x3 board,
one missing corner, and a standing block. `rcc/bloxorz_static.rs` uses the normal
GPU CB/Pass API to record two identical frames into fixed Back B and Depth. It
uses the fixed-point camera and instance preparation described in
[RCC scene geometry](/systems/cpu-v3-tang-nano-20k/examples/bloxorz/docs/rcc-scene.md).
It does not present or write Front A.

## Static assets and coordinates

`tools/static-scene.py assets <directory>` builds a 4,224-byte asset image for
physical byte address `0x4b0000`. It reuses the existing native scene oracle's
`pack` and `prepare` functions for V96/T64 payloads and meshlet metadata. It does
not construct a command buffer or define an alternate GPU format.

| Offset | Content |
| --- | --- |
| `0x0000` | Two mesh directory entries |
| `0x0080` | Floor metadata; 256-byte padded payload follows |
| `0x0200` | Block metadata; 384-byte padded payload follows |
| `0x1000` | One white RGB565 texel in a 128-byte allocation |

Each mesh has one meshlet. The floor has 12 vertices and 6 triangles for its
three camera-facing faces; the block has 24 vertices and 12 triangles. Per-face
vertices preserve separate axis normals and flat RGB colors. The floor is a
fixture for this fixed camera, not an arbitrary-view closed tile mesh.

Source coordinates are twice game coordinates, paired with half instance scale.
The floor uses source half extents `(1, 0.2, 1)` and `quant_shift=8`; the block
uses `(0.98, 0.98, 0.98)` and `quant_shift=7`. Origins are rounded to that grid,
deltas fit unsigned 10 bits, normals use signed 10-bit axis values, and both
meshlets use `scale_s=5` with the normal18 path. Position projection and normal
transforms remain separate. Culling is disabled; depth LESS with writes resolves
the closed block. The fixture uses unlit RGB, not the imported atlas. Small dark seams in the
quantized tiled surface also occur in the integer reference; this fixture is an
interface/rendering check, not a finished material or seam-quality baseline.

## Recording and validation boundary

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label bloxorz-static-native -CargoArgs @('-p','cpu-v3-tang-nano-20k','--example','bloxorz','static_scene_cpu_recorded_native_gpu','--','--ignored','--nocapture','--test-threads=1')
```

This test needs Python, Icarus, `GOWIN_HOME`, and Pillow for the preview. It has
separate CPU and GPU phases:

1. Compile the RCC application and execute it in CPU V3's emulator. A control
   peer captures the actual submitted command bytes and object arena. Diagnostic
   Device 7 messages expose instance matrices to the independent reference; they
   never generate commands. The peer acknowledges submission without rendering.
   Its read-only Display status reports Front A in RGB565 with no Pending swap;
   writes to Display fail the test. After completion, the application uses
   `gpu_discard_ready` to release Back B without presenting it.
2. Replay those same bytes through the native GPU RTL and burst-memory model,
   with clock-enable gaps and read/write backpressure. Compare each completed
   frame against the independent integer reference, including all 96,000 RGB565
   pixels, depth, Front, padding, neighbouring allocations and immutable assets.

The CPU phase is bounded by 20 million instructions. The GPU phase has a
3-million-cycle watchdog, and each external tool has a 1,200-second wall limit.
Output lives in `target/bloxorz-static-native/`, including the actual CB, objects,
reference surfaces, native surfaces, testbench, generated RTL and preview PNG.
Unknown X/Z depth bits are explicitly rejected, with a short native negative
test. Color is exact; covered depth permits one D16 unit, while untouched depth must
remain exactly `0xffff`. Both identical CPU submissions must use identical object
bytes and command length. DMA padding follows the driver's 128-byte alignment.

This is compiled CPU recording followed by native GPU replay. It does not prove
simultaneous CPU/cache/SystemControl/GPU execution, cache-maintenance behavior or
physical SDRAM correctness. RCC uses default compiler optimization settings.
The scene is not selected by the boot build. The game session,
Display presentation, atlas materials, controller input and runtime shadows are
outside this static fixture.
