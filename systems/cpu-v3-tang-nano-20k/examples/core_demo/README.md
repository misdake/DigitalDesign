# Core demonstration gallery

One ordinary application with six navigable pages. The CPU draws animated
triangles, waves and controller illustrations directly into the RGB565 Back
plane; the GPU clears that plane and overlays text/icons. The screen is a
visual gallery, with per-page titles, small corner controls and no development HUD.

Start with the [application quickstart](/systems/cpu-v3-tang-nano-20k/docs/application-quickstart.md)
for prerequisites, build, simulation and the source/asset recipe.

## Build and run

From the repository root:

```powershell
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- preview core-demo
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- run core-demo
```

Each command creates a fresh snapshot and separate run directory. `--snapshot`
consumes an existing export; `build core-demo --out NEW-DIRECTORY` exports without
running. `--page 0..5` selects the initial
page. Build option `--frames N` stops after N confirmed application presents;
zero, the default, runs continuously. Simulator `--frames` instead bounds host
display samples, and `--max-cpu-steps` bounds CPU work. Simulation time is not
physical CPU/GPU time. The simulator executes the actual package and functional
GPU, including UI pixels and Display color interpretation.

`run` opens a window with realtime input from XInput slot 0; use `--headless` or
`--xinput-slot N` to change this. The direct simulator remains available with
explicit backend, sample and CPU limits. See the
[workflow guide](/systems/cpu-v3-tang-nano-20k/docs/application-workflow.md#preview-and-run).
Physical firmware uses the ordinary DS2 input device.

## Controls

| Control | Action |
| --- | --- |
| Up / down, Cross on home | Select / open a highlighted menu item |
| L1 / R1 | Previous / next page, wrapping at the ends |
| Left / right on triangles/waves/geometry | Change CPU rotation/amplitude or GPU focal length on a press edge |
| Cross on triangles/waves/geometry | Pause / resume animation |
| Up / down on color | Focus SRGB / LINEAR |
| Cross on color | Apply the focused option |
| Triangle on triangles/waves/geometry | Reset parameter and animation |
| Start | Return home |

The page itself stays selected on reset. Navigation uses edges, so holding a
page button does not repeatedly switch pages. Input and wall time continue to
be sampled while animation is paused. Global navigation buttons remain reserved
for the gallery; the controller page shows held states of the other buttons.
Color choice has its own focused/applied state and does not use left/right or
Cross-to-pause. Each page keeps its own title. There is no global brand bar, page counter or footer prose;
Each corner hint pairs a 24×16 controller keycap with its action icon: L1/previous,
R1/next, D-pad/adjust or select, Cross/pause or confirm, Triangle/reset and
Start/home. Paused pages show a play action beside Cross. Shoulder labels use
the same native project-font pixels inside their keycaps; there is no footer prose.

## Pages and observable behavior

| Page | Demonstration |
| --- | --- |
| 0 | Five gallery entries and selection highlight |
| 1 | CPU-filled gradient triangle and RGB wireframe; active time rotates both, left/right changes their orientation |
| 2 | CPU Q16.16 sine/cosine traces and a phase-circle pointer; left/right changes amplitude, Cross freezes motion |
| 3 | Two calibrated stick plots, sixteen button panels and a connection icon |
| 4 | Explicit SRGB / LINEAR radio options above gray and color swatches |
| 5 | Two textured, depth-tested GPU cubes: fixed nonuniform world/normal matrices on the left, runtime TRS rotation on the right; left/right adjusts perspective |

The current hardware fixes the source at 400x240 and output at 800x480 with
2x scaling. The gallery does not change HDMI timings. SRGB presents already
encoded RGB565 directly; LINEAR interprets the same stored RGB565 as linear
light and enables the hardware sRGB transfer for scanout.
UI atlas pixels are RGBA5551, converted by the GPU to the RGB565 framebuffer.

The font producer uses the shared native source writer to emit native bytes, a
typed const atlas description and icon IDs. Source declarations import
those bytes with `include_bytes!`, request GPU storage aligned to 64 KiB, and
construct a static atlas description. Address, capacity and glyph count follow
the asset automatically; see [static assets](/systems/cpu-v3-tang-nano-20k/docs/static-assets.md).
Display interpretation is then staged together with each swap. It returns to
raw RGB565 when leaving page 4. The UI is a shared external grid updated only
after its preceding GPU use finishes; scanout reads the completed framebuffer.

## Source and static content

```text
rcc/
  main.rs
  pages/       six pages and CPU-page dispatch
  common/      frame passes, state, UI, palette and static working set
```

The recipe declares `pages/` and `common/` as ordinary RCC source roots. Existing
module names stay flat; the directories group application files by responsibility.

| File | Responsibility |
| --- | --- |
| `rcc/common/working_set.rs` | Const application-owned surfaces, commands, push and UI storage |
| `rcc/main.rs` | Startup resource ownership, frame scheduling, GPU completion and Display handoff |
| `rcc/common/state.rs` | Press-edge controls, modulo active time and shared animation phase |
| `rcc/common/frame.rs` | Shared Back selection, CPU clear and color/depth pass construction |
| `rcc/pages/cpu_pages.rs` | CPU page dispatch and Display-format selection |
| `rcc/pages/home_page.rs` | Gallery entries and selection highlight |
| `rcc/pages/triangles_page.rs` | CPU filled-gradient triangle and wireframe animation |
| `rcc/pages/waves_page.rs` | CPU sine/cosine traces and phase-circle animation |
| `rcc/pages/controller_page.rs` | Stick plots, button panels and connection indicator |
| `rcc/pages/color_page.rs` | Color-option UI, swatches and Display interpretation |
| `rcc/pages/geometry_page.rs` | Startup GPU resources, per-frame pose/labels and geometry pass state/draws |
| `rcc/common/ui.rs`, `rcc/common/palette.rs` | Shared text/icon layout and colors |
| `tools/assets.py` | Selected project-font base asset, typed resources and coordinated icon IDs |
| `tools/geometry.py` | Small cube and mip texture through the shared native encoders |
| `application.rs` | Application-owned build recipe and bounded startup options |

Each page owns its content updates and drawing loops. Shared passes stay separate
from page content; main retains submit/wait and Display ownership. Geometry
preparation, pass state and drawing use separate borrowed-resource functions,
within RCC's six-parameter function ABI. Live resource handles remain explicit
borrowed parameters.

The gallery uses the public `UiGrid` API for text and icons. Development-only
numeric output is absent; [numeric text helpers](/systems/cpu-v3-tang-nano-20k/docs/ui-text.md)
remain available to SDK consumers independently.

The selected [project-font base asset](/assets/fonts/README.md) contains 91 native ASCII-indexed slots
plus eleven action/state glyphs and seven 24×16 keycaps split into three
records each (31,488 bytes total).
Only the requested uppercase/digit/punctuation glyphs are populated. Native
ASCII IDs avoid a runtime translation table; ID255 skips without loading its
record. The base remains 64-KiB-aligned, but the memory extent and glyph count
are generated together. The normal asset manifest and CB06 COPY record carry
only this payload. Applications can append icons through `ui_font.base_asset`.

The shared ordinary [CPU canvas](/systems/cpu-v3-tang-nano-20k/docs/cpu-canvas.md)
owns pixel addressing and temporary DSEG3 windows; this app owns geometry,
page layout and state. Each CPU page frame retires a GPU clear, draws CPU graphics,
submits a load/overlay pass (publishing cached CPU stores), waits for it, then
presents the completed Back. Neither CPU nor GPU modifies Front. The extra
clear/overlay boundary is explicit; there is no hidden frame scheduler.
The GPU page records color/depth clear, both model draws and UI into one pass;
models, texture, camera and state are registered once at startup and reused across
page changes. Only the rotating pose and focal projection update each frame.
The source-world X basis reflects at draw conversion, so screen-left uses +X.
Pause freezes pose while retaining projection adjustment; Triangle restores the
initial phase and focal setting. Native matrices apply the required reflection
inside the driver, never in the application's world/normal values.

## Bounded qualification

The test consumes an explicitly exported, continuous package; it never patches
the executable or application state. It publishes real controller snapshots,
advances explicit host time and waits for actual Display applications.

```powershell
$env:CORE_DEMO_ARTIFACT = (Resolve-Path target/core-demo/app).Path
cargo test --release -p cpu-v3-tang-nano-20k --test core_demo -- --ignored --nocapture --test-threads=1
```

Every present has an eight-million-step bound. The test checks key/action pairs
and the pause/resume hint transition, plus six-page
navigation, actual triangle rotation and pause, sine/cosine pixels and phase
marker, button colors and disconnect, all32
gray values and both Display formats, including focus-before-confirm and the
left/right negative control. Fourteen raw positions per stick are checked
against an independent floating-point calibration/dead-zone/distance oracle.
Both axes follow the shared +X right / +Y down contract; normalized direction
and maximum-axis travel map full diagonals onto the circular gate instead of
clipping a square. The rendering code reuses the `gamepad` → `stick_read`
processing already used by the old DS2 diagnostic; it applies no second radial
correction or Y inversion. Only the final screen coordinate is quantized.
The GPU page checks both cubes, static-side stability, actual rotating-side
movement, pause, perspective adjustment, deterministic reset and repeated entry.
It also verifies that Front, CPU-page depth and
framebuffer padding are not overwritten by CPU drawings, and checks static/observed-stack usage.
The regular canvas test compiles without a GPU library and independently checks
clipped spans, borders, primitive pixels and DSEG3 restoration. PPM evidence is
written below `target/core-demo/key-legends/pages`. These are correctness checks;
they do not establish FPGA timing or GPU performance.
