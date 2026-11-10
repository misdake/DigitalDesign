# Explicit GPU API: prepared scene values and native conversion

The preparation helpers write caller-owned CPU spans. They allocate no arena
slots, retain no object handles, submit no commands and perform no GPU waits.
The resource adapters own allocation, failure reporting and external-memory
ownership. The shared `rcc/gpu_schema.rs` defines all prepared field offsets;
counts and offsets are 16-bit CPU words, and external extents use 128-byte
sectors. These helpers do not implement the public resource factories.

Numeric vec3 reads and writes touch exactly six words: the low then high halves
of each Q16.16 x/y/z lane. They use direct lane/word intrinsics without nested
scalar helper calls. They do not access an eight-word vec4 span; offsets retain
the ordinary unchecked 16-bit Array indexing behavior.

## Source-world transforms

`Vec3` stores three `fix32` fields. `Mat3x4` has three named `Vec4` rows and
occupies 48 bytes; `Mat3x3` has three named `Vec3` rows and occupies 36 bytes.
These ordinary values do not embed intrinsic vector registers or owning
handles. Scalar fields convert to intrinsic vectors only during computation.

Projection preparation uses a six-word temporary output and returns bool.
`gpu_scene_projection_focal` checks the existing native far/ray limits and
quantizes the two Q14 ray components once. `gpu_scene_projection_fov` accepts
radians and derives focal pixels as 120*cot(vertical_fov/2) or
200*cot(horizontal_fov/2), with a representability check before division.
Failures leave the temporary unchanged. Its getter returns a Projection value
containing focal, far and the cached two ray components. Camera projection
updates copy these values and set ray_k to 8192; they do not repeat reciprocal
or quantization work. Error reporting belongs to the public adapter.

Camera preparation stores an unreflected source-world affine view. Euler pose
uses Rz*Ry*Rx and the inverse rigid view R-transpose with translation
-R-transpose*eye. Look-to/look-at normalize the backward and reference-up
directions before their cross product; zero direction or an unrepresentable
parallel basis fails before writing the view. The look-at constructor also
initializes projection and zero padding. View getters and setters preserve
the same source basis, allowing an unchanged round trip. Caller-supplied views
must be rigid transforms.

Model TRS preparation writes world=R*S and normal=R*inverse(S), with translation
only in world. It computes the three inverse scales once. Zero scale fails
before writing; input values and intermediate arithmetic must fit the native
fixed-point domain. Explicit matrix input uses the caller's supplied normal
matrix. The normal matrix is tightly packed at words 24..41. Pure preparation
zeros words 42..47. A public Model uses private word 42 to own a second48-word
arena allocation; words 43..47 remain zero. Its setters preserve that owned handle
across pure preparation and mark its derived data dirty. There is no retained
Euler copy. `Model::world()` and `Model::normal()` return the exact current
source matrices without modifying source or derived-cache state. The normal
getter reads all nine tightly packed Q16 coefficients.

## Six-argument draw conversion

`gpu_scene_draw_prepare(desired, model, material, view, textured, native)` reads
the 48-word desired Camera/State snapshot, independent of the original arena
objects. It writes only Global words 0..31, Render words 32..47 and Instance
words 64..127 in the shared 256-word scratch. Target words 48..63 and all words
128..255 remain unchanged. Inputs must be disjoint from output and are unchanged.

At this sole conversion boundary, H=diag(-1,+1,+1) and
K=diag(focal/256,-focal/256,-1):

- Position coefficients are K*H*view*world.
- Normal coefficients are H*view_rotation*world_normal.
- Light direction is H*view_rotation*world_light.

The Q14 light encoder retains the existing bounded correction of the largest
component and checks the exact integer squared norm, within 32768 of 2^28.
Global uses the prepared far/rays, the final material/light coefficients,
lighting mode 3, the material shininess code and epoch zero. Global padding is
zero. Instance contains twelve position coefficients, nine normal coefficients,
Q16.16 one at words 42/43, material RGBA at words 44/45 and zero tail padding.
This stage performs no command emission or binding comparison. A failed
conversion may have changed its scratch output; the adapter must reject it.

The matrix kernel reads the three Camera rows and the world rows with existing
four-lane imports. Three overlapping imports at Model words 24, 30 and 34 read
the normal matrix without crossing word 41. It transposes in FPU registers and
finishes normals before loading world rows; one position basis is live at a time.
Five four-lane exports and one scalar write cover exactly the 42 coefficient
words. Each original three-product dot narrows once before translation addition,
projection multiplication or the row-zero normal negation; arithmetic precision
and wrapping remain unchanged. Scalar and six-word vector helpers carry explicit
best-effort inline hints. No compiler policy or cache dependency changes.

## Model-owned derived matrices

Every public Model owns a source allocation and a derived allocation, each 48
CPU words. Its affine resource remains one private 16-bit handle. The derived
allocation contains final position coefficients 0..23, final normal coefficients
24..41, dirty 42, the captured four-word Camera revision 43..46 and zero padding 47.
This is one result per Model, not a global last-result cache: drawing another
Model cannot evict it. Multiple views of the same Model require recomputation
when their view revisions differ.

New Models start dirty. Successful `set_pose` marks only that Model dirty.
`set_matrices` compares all24 world and18 normal source words, including both
halves of each Q16.16 value. An exact match preserves the current dirty/cache
state; any changed word marks that Model dirty. The plain matrix aggregates use
the same canonical row layout as those source spans. Camera construction and successful view setters assign a fresh
nonzero 64-bit revision in private Camera words 32..35. Projection changes assign
one only when the focal value changes; far/ray changes leave matrix dependencies
unchanged. `Pass::set_camera` copies the revision into reserved record control
words 16..19 alongside the existing numeric Camera snapshot. Later Camera changes
cannot change that Pass's inputs. At draw time a different snapshot revision
marks that Model dirty; no full Model/Camera value comparison is performed.

The light/material/render validation and Instance initialization run for every
draw. A dirty Model then computes the 42 coefficients directly into its own
allocation using the same scalar expression order as the pure conversion. The
revision is captured and dirty clears only after the calculation completes.
Clean Models skip that calculation. Every draw copies the coefficients into the
current native Instance and writes its current material and fixed fields.
Global/Render data, padding, binding comparison and push publication are unchanged.
Empty draws and failed preparation do not compute or clear dirty; a later mesh
or CB failure does not undo already calculated CPU values.

The revision counter is never reset by arena/context reuse and fails before
wrapping, so Camera Drop/reuse cannot revive an earlier snapshot identity. Model
Drop frees both allocations; construction releases its source allocation if the
second allocation cannot fit. A Model now consumes 96 arena words (192 bytes),
96 bytes more than its source descriptor alone. The existing arena reservation
and slot classes do not grow. Camera/CB revisions use their existing reserved
space, and the global revision counter adds eight bytes. GPU commands retain
only immutable push copies, not pointers or borrows into a Model's derived data.

## Material and render state

`gpu_scene_material_init` fills one 16-word slot, including zero padding.
RGBA8888 occupies two words with R in the lowest byte; specular RGB888 occupies
two words with its unused highest byte zero. Responses remain Q16.16. Shininess
uses the existing 17 native codes for exponents
4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 32, 40, 48, 56 and 64.
Nearest-exponent selection chooses the lower exponent on a tie.

Field setters alter only their corresponding prepared fields. The material
getter returns an independent ordinary value. Uniform alpha never changes
blend, depth write or unlit state. Debug builds reject negative responses
before changing a slot; release builds treat these input premises as the
application's responsibility.

`gpu_scene_state_init` stores a normalized world-space light, two Q16.16
intensities, packed depth/blend controls and a filter code. It clears all
padding. A zero direction fails without writing; normalization rescales by
powers of two before the square length, including signed-minimum and very
small nonzero directions. Debug builds additionally reject negative
intensities. Depth, blend and filter setters preserve the other controls.

`gpu_scene_light_response` multiplies each light intensity by its material
response in Q16.16, then rounds once to native Q8. The resulting coefficients
must lie in 0..256 in both profiles. Both products are checked before either
native output word changes. The directional coefficient scales both diffuse
and specular response; there is no separate specular intensity.

## Texture descriptions and final Render32

`TextureDesc` references A's shared `GpuMemory` value. Physical registration
owns endpoint validation; texture construction checks the texture-specific
alignment, exponent and required capacity once. POT exponents are 0..10.
`TextureDesc.format` is `TextureFormat::Rgb565` or `TextureFormat::Rgba4444`,
independent of `full_mip`. Both use the same16-bit storage and filters.
Each mip occupies at least four sectors; larger levels use the native 16x16
column-macro footprint. Full-mip descriptions cover all levels down to 1x1.

A view copies the entire 16-word prepared description. It contains no owner
reference and survives reuse of the source description. This does not extend
the lifetime of its external payload. A color view records fixed400x240 shape,
64-KiB base alignment, exponent 9, no mip chain and the allocation's actual
sector count; the physical image spans 1600 sectors.
The prepared flags word keeps mip in bit0 and fixed shape in bit1, with
RGBA4444 in bit2; the16-word description does not grow. Fixed color views
remain RGB565. The raw internal `GpuTexture`/`GpuRender.texture_flags` packs
mip in bit0 and RGBA4444 in bit1, replacing the old mip-only word.

`gpu_scene_render_prepare` writes only scratch words 32..47. Fixed400 shape
occupies Render word 1 bit 7, equivalently bit 23 of its first 32-bit word.
RGBA4444 occupies that first32-bit word's bit24, alongside existing mip/filter
bits. Recording equality includes format, so a format-only change installs
new Render state and follows the same drain boundary.
Trilinear filtering of that shape fails before modifying output. The RGB
bypass branch reads no view or texture payload and installs a deterministic
base-zero, exponent-zero, nearest-filter description. Specular color and
explicit depth/blend/unlit controls retain the native encoding. All unused
Render words are zero; other scratch regions remain untouched.

## Persistent UI grids

An atlas describes 1..255 glyphs at a 64-KiB-aligned base. Each glyph requires
256 bytes; a small atlas needs only its actual glyph capacity. A grid requires
six sectors: 750 valid row-major bytes for 50x15 cells, with readable padding
through byte 767. Grid construction copies the atlas description by value.
Native UI preparation writes the grid sector and atlas 64-KiB page, then zero
padding, into the caller's 16-word destination.

Fill and rectangle writes modify the actual external grid through A's
`gpu_window_read_words` / `gpu_window_write_words`. They use one 25-word stack
row and never create a persistent grid mirror. Fill writes exactly 375 words.
Rectangle writes read only boundary words containing a preserved neighboring
byte, pack two glyphs per word, and write at most 25 words per row. A's window
helper splits transfers at DSEG3 boundaries and restores the mapping. The nine
padding words are unchanged. Short input fills the remaining rectangle cells
with transparent glyph 255; input beyond the rectangle is ignored.

An empty rectangle is a no-op. In release, nonempty rectangles and glyph IDs
must satisfy the application's bounds premises. Debug builds check rectangle
bounds and all used glyphs before the first write; their predicates disappear
from release. Glyph 255 is always transparent. Window failures return false;
the helpers do not promise rollback of earlier external writes.

CPU/GPU ownership remains explicit: the caller must retire prior GPU use before
changing a live grid, and submission publishes CPU stores. These helpers neither
invalidate GPU caches nor publish globally after each row.

## Fixed depth projection

Projection stores focal length, physical far and ray scales. The driver records
far directly as positive Q16.16 below200; D16 depth has eight fractional bits.
There is no selectable depth-shift API, Device4 capability query or per-surface
precision provenance. Clear, Load, depth comparisons/writes and existing command
retirement and target ownership retain their usual boundaries.
