# Explicit GPU application callers

The member, semantic-change, RTT, profiler and Bloxorz callers use public
`Gpu`, `CommandBuffer`, scene resources and `Display` directly.
`gpu_demo_api.rs` is an ordinary application module: it constructs values,
formats UI digits and imports the boot package's fixed asset extents. It has
no driver state, frame owner, hidden submission or retirement loop.

Each application owns one command buffer, two display color planes and a depth
plane. It records into Back B first, submits by value, gets the buffer back from
`Gpu::wait`, queues that completed color plane and confirms the swap with
`Display::wait_vblank`. It resets only the returned buffer and alternates the
back plane. These examples deliberately use serial completion; no caller writes
a plane still being scanned or mutates a UI grid still used by its GPU work.

| Caller | Recording and ownership behavior |
| --- | --- |
| `gpu_renderer/rcc/members.rs` | Two draws snapshot one mutable Model; three-frame entry confirms B/A/B. |
| `gpu_renderer/rcc/changes.rs` | Eleven phases edit Camera, Model, Material and lighting; a live Pass retains its installed Camera, then a successor installs the edit. Alpha drawing uses explicit SrcOver state. |
| `gpu_renderer_rtt/rcc/scene.rs` | A producer completes into caller-owned RTT planes before a consumer records their TextureView. The view may be dropped after recording. Texture invalidation is explicit between stages. |
| `gpu_profiler_board/rcc/main.rs` | Static minimal diagnostic, bounded UART progress markers and explicit UI grid. |
| `gpu_profiler_board/rcc/full_scene.rs` | Six box meshlets, one RGB meshlet and seventeen rabbit meshlets; the RGB draw uses `draw_colored`. |
| `gpu_demo/rcc/public_main.rs` | The complete classic scene with continuous unified gamepad input, first-person camera, timing UI and explicit GPU/swap polling. |
| `gpu_renderer_bloxorz/rcc/main.rs` | Explicit input/time sampling, offscreen transition snapshots, ordinary/fade consumers and confirmed presentation before session advancement. |

The full box/RGB/rabbit scene explicitly preserves the original controller
`gpu_demo` scene contract: trilinear filtering (native code2), LESS with depth
write (code1), rabbit shininess exponent32 (native code12), and linear-light
RGB565 presentation (Device3 format1). The corresponding native Render word1
is `0x0056` for the 64x64 full-mip texture; Render word4 low six bits are41 for
unlit room/RGB and9 for the lit rabbit. The rabbit's Global word10 is `0x0033`
for mode3 and shininess code12. The static profiler's unlit material retains
S16/code8 (`0x0023`); the controller explicitly retains the native caller's S32
and 64/192 material responses even for unlit draws. Unlit Final bypasses lighting
with g=256/h=0, so the shininess field contributes no specular term there.
`Shininess::S12` means exponent12/code6, so the lit rabbit uses
`Shininess::S32`. Other demo factories retain their own defaults. The full
profiler entry has a fixed camera. `CPU_V3_BOOT_EXAMPLE=gpu_demo_api_column16`
selects `examples/gpu_demo/rcc/public_main.rs` for the complete public-API
controller scene. The earlier `gpu_demo_column16` entry at
`examples/gpu_demo/rcc/main.rs` remains a separate native caller.

The public controller caller consumes one `gamepad` frame before preparing any
previous-frame UART report. It retains both-stick movement/look, L1/R1 vertical
motion, the classic initial pose and bounded wall-time integration. It clears
the rendering timer before input/preparation and locks it after GPU retirement,
before presentation. `Gpu::try_wait` and `Display::try_wait_swap` let it service
nonblocking telemetry and enforce the existing 2000ms wait budgets. UI shows
only the previous successfully presented valid interval at row1,column2; the
debounced board button toggles visibility. Its native asset producer is reused
without the profiler wrapper's OBS atlas substitution. The default boot and
fixed profiler selections are unchanged.

The public controller uses fixed CPU command-buffer roles: slot0 retains the
room/RGB stream and slot1 records rabbit/UI work. Startup invalidates textures
with one separate ordinary maintenance submission, then records slot0's clear,
six room meshlets and RGB meshlet once. Each frame starts after the previous
dynamic submission and Display swap retire. Input updates the Camera; the
typed scene binding patches slot0's Target/Global/Instance for the current Back
without resetting its commands, and ordinary submit starts that static work.
While it runs, the CPU records slot1 with Load color/depth, a separate rabbit
Model and the UI. It waits for static retirement, submits slot1 through
`submit_capturable`, waits for dynamic retirement, then presents and confirms
the swap. Back A/B alternates; CPU slot roles do not. The default rabbit Model
remains stationary. Waits retain the existing deadline and telemetry handling.

An armed dynamic capture stops after static retirement and dynamic publication,
before dynamic START. Ordinary static and unarmed eligible work bypass the
capture gate. The existing explicit-v2 wire format omits Front; a functional
system qualification can observe Front separately without adding a wire range.
The controlled two-frame test moves/turns Camera and independently rotates the
rabbit only in its test source. Its full frame color/depth and guard comparison
is functional evidence, not native CPU/GPU timing or board qualification.

RTT planes occupy sector ranges `0xc000..0xc800` and `0xc800..0xd000`, each with
2048 sectors of caller-owned capacity. The current packers exclude these ranges
from assets. Display uses the ordinary demo planes. No CPU pixel conversion or
readback is used. Initial texture use and changed RTT content receive explicit
invalidation. Continuous `renderer_rtt_profiler` and bounded nearest/bilinear
entry points use the same recording implementation.

Profiler timing digits are application-observed submit-to-wait microseconds,
including CPU control overhead. Value65535 denotes an unavailable/saturated
observation. They are not a measurement of pure GPU execution or physical timing.

The ordinary `profiler_clock` module exposes a resume sequence and active
milliseconds. The shared diagnostic consumer adds the paused wall-clock interval
before returning Resumed, then increments the sequence. Both values use 16-bit
modulo arithmetic; a complete wall-clock wrap still increments the sequence.
The physical clock and input devices are unchanged. The changing-scene and full
profiler callers sample the sequence around submit/wait and mark that observation
unavailable if it changes. Bloxorz integrates active milliseconds and explicitly
captures/discards stale input edges after its own resumed wait. This is an
application decision, separate from GPU ownership and completion.

The capture consumer reads external ranges into its bounded low-memory cache.
Header snapshots, UART staging and RLE/CRC work also use low-memory arrays;
these are not destination DRAM line copies. The seven callers and ordinary
demo helper issue no CPU `dcache_line_copy` or resident-copy intrinsic. GPU
MEMCPY records retain their independent GPU meaning. The shared
window helper supplies the actual mapped read; its DWAIT, DSEG restoration and
submit publication boundaries remain necessary regardless of the CPU copy ISA.

The Bloxorz presentation accepts direction, restart and fixture-selection keys;
it exposes no undo command. Its unchanged rule engine still supports a general
256-entry undo ring. This caller instead resets the private ring cursor before
each move and reuses one six-word scratch snapshot. Visible game/session/motion
states remain identical; no rule, key or fixture is removed.

## Bounded caller qualification

`tools/qualify_api_callers.py` consumes an external A stage containing
`manifest.json` and its exact `sources/` files. It verifies every supplied hash
and invokes that stage's actual checked System loader and approved root. Local
application sources gain no private-helper privilege. Generated crates and
ordinary source copies remain under this worktree's `target/`.

Provide a migrated source, its bounded entry, constants from the ordinary asset
packer and the matching `assets.json`. Optional `--extra-dir` supplies ordinary
game modules. Both check profiles execute the actual CPU program with a finite
step limit and verify actual draw counts, confirmed Front order, frame sizes and
static plus observed stack against the16-KiB budget. GPU/Display peers model
control completion only: they do not render, scan out or model physical cache
publication. This qualification does not establish captured G3.

The `tests/fixtures/api_c_session.rs` scenario has3600 ticks, including more than
256 alternating move attempts, fixture selection and restart. `--session`
compares both profiles'27 visible state words per tick against the frozen
pre-migration CPU trace identity in `api_c_session_reference.json`. The fixture
records source and scenario hashes; it contains no second hand-maintained
instruction array. This checks the scratch-history change independently of GPU
rendering.

`--capture-build-dir` and `--capture-gate` enable the actual capture hooks using
per-profile build manifests. A gate can call the shared UART consumer or select
a controlled action; the driver verifies exactly one additional publication for
each Resumed action. It saves the actual frozen header, ranges and payloads before
GPU START and observes the executed dump path's stack. Each manifest must bind
the actual source inputs; adding `--native-rtl` also requires the selected RTL
export to be bound.

With `--native-rtl`, START runs the existing production GPU/stream-MC replay and
imports every writable output range before the CPU may observe GPU completion
or present. The native helper has finite cycle and process limits. This connects
actual recorded CPU data to rendered results; it remains a cold replay per
submission and does not model physical CPU caches, Display scanout or warm
cross-submission contention. Reference envelope encoding in that helper is a
transport adapter; actual UART encoding is checked separately.

`tests/support/api_c_native_pixels.py ui|rtt <result-directory>` checks the
completed public fixture outputs in both profiles. UI uses an independent
RGBA5551 atlas decoder and checks both entire physical planes. RTT checks all
targets, the small consumer viewport, retained producer colors, padding,
representative nearest samples and identical output bytes across profiles.
Detailed sampler arithmetic remains in the GPU's independent numerical tests.
The small viewport keeps the functional dependency check bounded while both
physical targets and their clears remain complete.

Generated qualification crates use distinct package names while sharing
dependency caches. The default CPU step bound is20million; full UART dump paths
can explicitly select a bound up to1billion. Native replays retain their own
500000-cycle/300-second limits, with a720-second outer limit for both profiles
and report generation. A timeout saves partial CPU progress where available;
it does not imply that the hardware stalled.
