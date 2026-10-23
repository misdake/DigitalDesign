# RCC GPU driver

The application interface is `rcc/gpu.rs`. It manages one command buffer and a
single Clear/Store Pass with ordered Mesh/Instance draws for fixed Color B and Depth. The boot profile
starts Display on A; this driver does not present or change that selection.
Using it after another owner selects B for scanout violates the D1 precondition.
Display handoff and runtime recovery are separate features. The default CPU
Display diagnostic remains selected in the boot config.

## Application calls

| Call | Result and ownership |
|---|---|
| `gpu_init()` | Initialize once; Idle, Pending for unknown hardware busy, or Fault. Repeated init is rejected without forgetting work. |
| `gpu_cb_begin()` | Return an opaque nonzero identity, or zero when recording/reuse is unavailable. Only a new or explicitly completed CB can begin. |
| `gpu_pass_clear(cb, rgb565, d16)` | Record the complete pass. False preserves cursors/state on validation or capacity failure. No raw command, address, length or DMA/window pointer is an application argument. |
| `gpu_pass_begin(cb, rgb565, d16)` | Begin the sole Pass, returning its opaque identity or zero. Clears B and Depth. |
| `gpu_pass_set_global(pass, global)` | Encode/snapshot native Global64; identical encoded values consume no commands or object space. |
| `gpu_pass_set_render(pass, texture, render)` | Bind an immutable Texture handle and snapshot the remaining Render fields. Texture address/dimensions come from the handle. |
| `gpu_pass_draw_mesh(pass, mesh, matrix, rgba_low, rgba_high)` | Snapshot the native 21-u32 Instance matrix prefix and RGBA; expand the immutable compact directory in original order. |
| `gpu_pass_end(pass)` / `gpu_cb_finish(cb)` | Drain/store the Pass, then append END and zero fetch padding and seal Ready. Each transition occurs once. |
| `gpu_submit_idle(cb)` | Pending after successful cache handoff and both SENDs; Rejected if ownership/state prevents acceptance; CacheError faults the driver, retains the Ready CB and sends neither GPU command. |
| `gpu_poll(cb)` | Idle, Pending, Completed, Fault or Rejected. Completed transfers the CB back exactly once; later polls return Idle. |
| `gpu_wait(cb, limit)` | At most `limit` polls; Timeout retains ownership and permits later observation. A zero limit performs no poll. |

Status values are Idle=0, Pending=1, Completed=2, Fault=3, Timeout=4,
Rejected=5, CacheError=6, NotInitialized=7. Handles are identities of the one
fixed CB, not physical addresses or allocation tokens. Begin cannot discard
Ready/InFlight work, repeated pass recording cannot overwrite Ready work, and
repeated Completed polling cannot reclaim storage twice. Fixed targets/assets
change usage ownership; they are not deleted, relocated or reconstructed.

The module loader flattens RCC names. Internal `gpu_*` implementation modules
and raw/native encoders are not an application API or a memory-security boundary.
Applications use the calls above, serialize the single driver and do not write
Device4 or internal state directly. Native encoding tests may use internal entry
points; they are not a second submission interface.

## Fixed storage and units

The sole configuration is `rcc/gpu_layout.rs`, also consumed by host layout and
boot packing. Addresses below are physical **bytes**, with exclusive ends.

| Range | Owner |
|---|---|
| 0x400000..0x430000 | Fixed Color A, initial Front |
| 0x430000..0x460000 | Fixed Color B, D1 output |
| 0x460000..0x490000 | Fixed Depth |
| 0x490000..0x4a0000 | Driver command allocation, 64 KiB |
| 0x4a0000..0x4b0000 | Driver object allocation, 64 KiB |
| 0x4b0000..0x800000 | Fixed GPU asset allocations |

The entire high 4 MiB domain is Shared in `TangNano20kMemoryLayout`, never Main.
Boot's mandatory framebuffer ZERO remains legal, while optional COPY/ZERO
resources cannot occupy the command/object allocation. No hardware decoder,
PLL, CL3 memory timing, ordinary heap or FPGA image configuration is changed.

Internal physical bases use 128-byte sectors. CPU pointers and arena cursors
are **16-bit words**. Commands are low16 then high16; valid count can be 32768
words, exactly 65536 bytes, producing count-minus-one 16383. Padding through
the final 128-byte fetch sector belongs to the command allocation and is zero.
Objects remain separate from END and the valid command stream. No offset,
length or address conversion silently masks an invalid input.

The software state/CB descriptor and small encoding staging buffers stay in
ordinary CPU memory. They do not share cache lines with the GPU input arena.
There is no dynamic allocator, compaction, pingpong scheduler or second CB.

## Scoped DSEG3 access

`read_dseg_page(index)` and unsafe `write_dseg_page(index,page)` lower to the
existing single-entry MFSR/MTSR instructions. The selector is a compile-time
constant 0..3. Invalid constant physical pages are rejected; dynamic raw writes
have the documented eight-bit precondition. A write is retained as a compiler
memory/control barrier. No legacy bulk `mtsr_dseg` occurs in the GPU driver.

The driver validates its fixed allocation, word range and low-memory staging
extent before touching memory. A bounded copy splits at 16384-word page
boundaries. For each chunk it saves DSEG3, completes blocking full
clean+invalidate, installs the checked physical page, verifies readback, copies,
completes another clean+invalidate, and restores/verifies the saved page.
Only logical 0xc000..0xffff is remapped; stack growth from 0xc000 stays below
the window, and DSEG0..2 remain unchanged. No window pointer escapes a call.

Ordinary CPU stores, accepted old responses and early-release FST tails drain
through the existing CPU maintenance barrier. On a maintenance or mapping
failure the driver restores the old page if it installed a new one, records a
sticky software memory fault and permits no GPU submission. Completed chunks
may already be visible after an I/O failure: cursors are not published, the
resource stays held and the driver does not claim a byte rollback or visibility
success. Validation/capacity failures occur before destination writes.

An architectural CPU load/store/instruction fault stops execution rather than
returning an error to RCC. Software cannot restore a mapping after that stop;
coordinated reset is required. Scoped restoration covers reported maintenance
status and mapping-readback failure, not resumable CPU exception handling.

## Encoding and execution

`gpu_encode.rs` implements current u32 commands and native Global64, Render32,
Target32 and Instance128 with explicit pairs of 16-bit values. The canonical
format remains [GPU commands](/ip/gpu-v2/docs/commands.md) and
[native objects](/ip/gpu-v2/docs/formats.md#native-command-objects).
Global checks F16 far, modes/intensities, ray range and the exact integer
unit-light squared-length tolerance. Render validates complete texture
allocation, mip chain, filtering, depth flags, RGB specular and signed F4 bias.
Instance records all 21 matrix words, mandatory word21=65536, word22=RGBA and
zero padding. Recording snapshots source values before returning; subsequent
source mutation does not change previous objects. The raw native matrix entry
remains available alongside the affine helpers below.

The Clear pass records WAIT(all), FLUSH, DMA Target32, WAIT(data), SET_TARGET,
CLEAR(color), CLEAR(depth), WAIT(all), FLUSH, END. Before launch the sole driver
again blocks on full clean+invalidate and checks the result, then rechecks GPU
status. It saves in-flight ownership before serial count/start SENDs. Only
busy=0 **and** error=0 completes. Error is checked first, even when busy is zero;
sticky transport or software memory fault retains resources and stops new work.
Timeout does not clear busy, ACK, reset, cancel, present or release an allocation.

## Static assets and ordered recording

`GpuMesh` is a trusted boot/packer root: `first_sector/sectors` owns its full
allocation, and `directory_sector/directory_word/count` identifies its compact
table. Each entry is two little-endian u16 words: record byte-address/128 and
payload sector count (1..8). The record contains the current 32-byte metadata,
96-byte prefix padding and complete padded payload. The directory is software
data, not a GPU header or a new meshlet ABI. Empty Meshes emit no commands.

The generator validates native metadata, indices, decoded positions, uniform20,
padding and numerical assumptions once. The driver validates the root, complete
directory extent and every entry's range/non-aliasing before recording a draw;
it does not decode all vertices each frame or safely import arbitrary bytes.
The checked directory is immutable through completion. A second bounded read
emits its commands after preflight; no directory-sized low-memory allocation is
needed. Batches contain at most 16 entries, including when crossing 128-byte or
32-KiB boundaries. `GpuTexture` holds a fixed base/allocation and POT/mip fields.
Render encoding checks the complete allocation. Assets stay resident across CBs;
binding a Texture neither uploads it nor invalidates its physically tagged cache.

Commands use a 128-word (256-byte) low-memory staging buffer. Objects use bounded
encoding scratch and are uploaded per snapshot; directories use 32-word scratch.
Window scopes retain both maintenance fences. The staging buffer is flushed in
batches and at finish, rather than switching DSEG3 for each command/word.
Capacity preflight includes alignment, Pass drain, END and final 128-byte padding.
Rejected operations leave the previous command/object cursors and staged prefix
unchanged. An I/O failure holds the CB under the sticky memory-fault rule above.

Dedicated local staging is Global qwords264..271, Render272..275 and Target276..279;
Instance is uniform block20 (qwords320..335). Each changed Global/Render emits
WAIT(all), DMA_LOCAL, WAIT(data), SET. Deduplication compares the actual encoded
object, including physical immutable texture identity, not the source pointer.
Each CB reinstalls its own state; a nonempty draw needs both bindings.

Each draw first DMA-copies its Instance and waits for data, then loads meshlet0
into page0 and waits. For each current page, the next record is prefetched into
the other page before DRAW; WAIT(data) precedes consuming that next page. DRAW
retirement establishes V/T completion, allowing page/uniform reuse while the
previous R/I/backend continues. Global/Render changes still require full render
drain. No cross-Mesh prefetch or transparent draw reordering occurs.
Clear-only wraps this same begin/end/finish Builder.

## Affine and Camera helpers

`gpu_transform.rs` uses existing fix32, vec3/4, fdot, frcp and fsincos operations.
All matrices are row-major signed Q16.16. A Pose is 18 u16 words containing
translation XYZ, Euler XYZ and scale XYZ as low/high pairs; `gpu_pose_set`
accepts vec3 values. Angles are radians, rotation is Rz*Ry*Rx. Translation is
bounded to +/-64, angles to +/-4, positive per-axis scale to [0.5,2].
`gpu_affine_identity/trs/compose` produce twelve matrix coefficients in 24 words;
compose supports aliased output via bounded scratch. Coefficients are bounded
to +/-4 and translations to +/-64; invalid output leaves the destination intact.

`gpu_camera_view` accepts a rigid unit-scale Pose and forms its transpose rotation
and negative rotated translation. `gpu_instance_trs` forms Camera*Model position
and CameraRotation*ModelRotation*inverse(ModelScale) normal matrices in 42 words.
It handles nonuniform positive scale analytically; it does not compute a general
4x4 inverse. Special-function approximation remains that of the CPU FPU.

Camera axes are +X right, +Y down, +Z forward. The GPU's fixed projection uses
pixel-center rays `(2*x-399,2*y-239,512)`, or screen `(199.5+256*X/Z,
119.5+256*Y/Z)`. There is no configurable perspective matrix or near-plane fan
clipping. `gpu_bounds_set` creates a 13-word AABB/scale_s descriptor;
`gpu_instance_bounds` checks its eight corners after the actual signed scale_s
shift, requiring every component in [-131071,131071] and forward Z positive.
Bounds inputs are limited to +/-64. This conservative helper is optional for the
native matrix path: the asset/caller must still establish its corresponding
scale_s, normal-quantization and source bounds. It does not alter metadata or
guarantee precision for an arbitrary imported asset. Shifting by scale_s loses
that many raw position bits; depth compensation remains the existing GPU rule.

## Validation boundary

Bounded compiled-CPU tests compare command/object bytes against the host GPU
encoding, exercise legal/illegal boundaries and failed-operation atomicity,
snapshot mutation, full-capacity tails, DSEG3 crossing/restoration and fault
ownership. Control-only peers inject busy, maintenance failure and transport
error; these tests do not execute rendering.

The native RTL fixture executes the compiled application through real CPU,
D-cache, SystemControl and the production Device4 GPU command composition.
It builds high-memory inputs, performs two consecutive Clear submissions,
checks true write terminal completion and compares both planes, Front, padding
and neighboring memory. It includes held CPU response, FST, GPU CE and memory
backpressure; a real cache writeback error must prevent all GPU starts and
restore DSEG3. No Display scanner runs in that controlled fixture.

The drawing fixture records two Passes on the real CPU, using immutable RGB/UV
records and a fixed texture, multiple Instances, changed lighting/Render and a
transparent final overlay. It compares materialized Color with an independent
integer ray/edge and Q6-blend reference; planar Z is checked within one D16 code
of the independent value. Front, full plane padding, command padding and nearby
memory remain protected. A diagnostic second object-arena starting cursor places
Target at the first DSEG page tail and subsequent snapshots in the second page;
that placement is not an application API. Separate compiled tests cover directory
crossing and rejection without modifying the existing recording. These compact
fixtures do not qualify arbitrary interpolants, lighting or full-scene capacity.

Its memory peer models the 64-bit shared-port contract, not SDRAM pins. The
required legacy system co-simulation remains separate from this native GPU
chain; neither is a new CL3 pin/timing, full boot, present or board qualification.
Pure software changes do not create a new fitted-area or timing measurement.
