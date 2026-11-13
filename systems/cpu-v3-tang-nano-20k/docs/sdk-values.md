---
id: sdk-values
status: active
last-verified: 2026-11-11
---

# Constructing SDK values

Start with independent source values. Constructors derive encoded fields and
common defaults; raw structs remain available when every field is intentional.
These helpers construct values, without allocating GPU objects or recording work.
Use the [static asset guide](/systems/cpu-v3-tang-nano-20k/docs/static-assets.md)
for placement and lifetime, then register descriptions with the ordinary GPU API.

## Numbers, colors and transforms

```rust
// Before: the reader needs to decode a Q16.16 bit pattern.
let half = fix32::from_words(32768, 0);
// After: constant quantization; no per-frame division.
let half = fix!(0.5);
let quarter_turn = fix32::degrees(90);
let pose = Transform::translation(Vec3::new(fix32::zero(), fix32::zero(), fix32::from_int(-3)))
    .with_rotation(Vec3::new(fix32::zero(), quarter_turn, fix32::zero()));
let speed = fix!(0.08);
let color = Rgba8::rgb(80, 120, 160).with_alpha(128);
let pixel = Rgb565::rgb(8, 8, 24);
```

`fix!` accepts one unsuffixed decimal literal: integers, fractions, a leading
minus, digit separators and scientific notation, with an optional trailing comma.
Examples are `fix!(0.125)`, `fix!(-1.25)`, `fix!(1_000)` and `fix!(1e-3)`.
It works in const/static descriptions and ordinary code. The SDK exports a real
Rust macro: rustc checks the original invocation, then RCC lowers it to exact
`from_words` constants using the same const decimal parser. No floating-point
intermediate, runtime parser/division or target-profile check is added. Quantized
values outside signed Q16.16 fail compilation in both profiles. The macro accepts
literals; use ordinary fixed-point operations for runtime values.

`from_ratio(i16, u16)` requires a nonzero denominator; `degrees(i16)` converts
integral degrees to radians. Their arguments must be compile-time constants in
RCC. Both quantize to nearest Q16.16, ties away from zero, through the same
implementation as rustc's host shim. `one()`, `zero()`, `from_int()` and
`from_words()` retain exact meanings; raw words remain appropriate for deliberately
pinned bit patterns. FPU arithmetic itself retains its existing rounding rules.

`Vec3` provides `new`, `zero`, `one`, `splat` and the three unit axes; `Vec4`
provides `new`, `zero` and `splat`. Matrices provide `from_rows` and `identity`.
`Transform` provides `new`, `identity`, `translation`, `rotation`, `uniform_scale`
and `with_position` / `with_rotation` / `with_scale`. Angles are radians, scale
is source-world scale; `Model::new` derives native world/normal matrices at runtime; explicit
`world_matrix` / `normal_matrix` preparation is described below.
`CameraDesc::new` takes eye, angles and a projection.

`Rgb8` and `Rgba8` take byte channels. `rgb` gives alpha 255; `with_alpha` changes
only alpha. `Rgb565::rgb` truncates to the high 5/6/5 bits. `Rgba8::to_rgba5551`
truncates RGB to 5 bits and uses alpha's high bit for native UI. The font producer
has the matching `ui_font.rgba5551` helper. Texture RGBA4444 remains a different format.

## Preparing fixed scene values

```rust
const POSE: Transform = Transform::translation(Vec3::new(fix!(0), fix!(0), fix!(-3)))
    .with_rotation(Vec3::new(fix!(0), fix!(22.5).to_radians(), fix!(0)));
static WORLD: Mat3x4 = POSE.world_matrix();
static NORMAL: Mat3x3 = POSE.normal_matrix();
const PROJECTION: Projection =
    Projection::from_vertical_fov(fix!(60).to_radians(), fix!(199));

// Runtime registration; the matrices have already been prepared.
let model = Model::from_matrices(WORLD, NORMAL);
let camera = Camera::new(CameraDesc::new(Vec3::zero(), Vec3::zero(), PROJECTION));
```

The same `const fn` interfaces accept runtime inputs. `world_matrix` uses
Rz * Ry * Rx, scales its columns, then adds source-world translation.
`normal_matrix` uses the inverse-transpose of that linear part, including
nonuniform/negative scale; zero scale is rejected. Native reflection remains at
recording. `Model::from_matrices` creates the same runtime model/cache lifecycle
as `Model::new` and avoids calculating TRS first. Existing live model updates
and cache invalidation remain unchanged.

Projection supports focal pixels, vertical FOV and horizontal FOV, with radians
and the existing far-plane/ray-basis limits. The public constructors and legacy
prepare helpers share one pure kernel. Invalid const parameters fail compilation;
invalid dynamic values halt with the pure assertion signal 0xffff. Numeric
construction does not write the GPU fault record. GPU allocation, recording and
execution failures keep their existing structured GPU diagnostics.

`frcp`, `frsqrt`, `fsin`, `fcos` and `fsincos` support constant evaluation. The
shared rustc/RCC const model uses higher-precision intermediates and rounds to
Q16.16; target runtime keeps native LUT instructions. Low bits may differ, and
subsequent fixed operations retain ordinary Q16.16 rounding/overflow semantics.
There is no target floating-point runtime or extra hardware/table allocation.
`fix!(22.5).to_radians()` quantizes once in const evaluation; runtime multiplies
by the Q16.16 radians-per-degree coefficient, so its precision can differ too.
`duration::seconds_to_milliseconds(u16)` converts 0..65 whole seconds without silent
u16 duration overflow and is usable in const or ordinary code.

Rust's ordinary arithmetic/comparison operator traits are not const here.
Custom const fixed-point helpers can use `wrapping_add`, `wrapping_sub`,
`wrapping_mul`, `wrapping_neg`, `less_than`, `less_or_equal`, `greater_than`,
`greater_or_equal` and `equals`. These lower to the same target operations as
runtime operators. Scalar raw-half/conversion/rounding methods and native vector
constructors/lane reads are also const-capable. Prefer the ordinary Vec/Mat
value types for stored scene descriptions.

## Materials and state

```rust
let material = MaterialDesc::lit(Rgba8::white(), Rgb8::white(), Shininess::S16)
    .with_response(fix32::from_ratio(1, 2), fix32::from_ratio(1, 2));
let light = Lighting::directional(Vec3::unit_z(), fix32::one(), fix32::one());
let state = RenderStateDesc::opaque(light)
    .with_depth(DepthFunc::LessEqual, true)
    .with_filter(TextureFilter::Nearest);
```

| Constructor | Initial choices |
| --- | --- |
| `MaterialDesc::unlit(color)` | Unlit, black specular, ambient 1, directional 0, S4 |
| `MaterialDesc::diffuse(color)` | Lit, black specular, responses 1/1, S4 |
| `MaterialDesc::lit(color, specular, shininess)` | Lit, responses 1/1, explicit specular and exponent |
| `RenderStateDesc::opaque(light)` | Less, depth write, Replace, Trilinear |
| `RenderStateDesc::transparent(light)` | Less, no depth write, SrcOver, Trilinear |

`with_response`, `with_unlit`, `with_depth`, `with_filter` and `with_blend` return
changed copies. `Lighting::ambient` disables directional intensity; `directional`
takes a direction plus the two intensities. Neither normalizes the source direction.
Transparent sorting and pass order stay application-owned.

## Native asset descriptions

```rust
#[asset(align = GPU_TEXTURE_ALIGNMENT)]
const TILE_BYTES: AssetBytes = AssetBytes::new(include_bytes!("assets/tile.bin"));
static TILE: TextureDesc = unsafe {
    TextureDesc::from_asset(TILE_BYTES,
        TextureLayout::square(64, TextureFormat::Rgba4444, Mipmaps::Full))
};
#[asset(align = GPU_ASSET_ALIGNMENT)]
const MESH_BYTES: AssetBytes = AssetBytes::new(include_bytes!("assets/mesh.bin"));
static MESH: MeshDesc = unsafe { MeshDesc::from_asset(MESH_BYTES, 0, 12) };
```

`TextureLayout::square` accepts power-of-two pixel sizes 1..1024, native format
and `BaseOnly` / `Full` mip choice. Both supported 16-bit formats share the same
native bank layout: each level occupies at least four 128-byte sectors, including
small levels. `required_sectors` sums the complete mip chain. The description
constructor checks the 256-byte base alignment and required capacity; `from_asset`
also checks exact byte length, excluding final-sector padding.

`MeshDesc::from_asset` takes a byte offset to the native directory and a meshlet
count. It derives sector/word coordinates, checks even offset and full directory
extent (two words per meshlet, count at most 5460). It does not decode or relocate
native meshlet records: producers must still supply valid package contents.
`from_memory` is the runtime alternative and takes a 16-bit *word* offset.
Texture `from_memory` similarly accepts an existing `GpuMemory` span and layout.
The unsafe asset bindings promise actual storage and lifetime; numeric validation
does not replace that promise.

## Board buffers and UI

`CommandMemory::new(commands, push)` groups one recording slot. `GpuConfig::single`
and `double` supply the 8192-byte default object arena; use the raw struct for a
different arena budget. The board factories provide the normal fixed slots.

`Rgb565Surface` describes a shared physical framebuffer span once. The unsafe
`from_sectors` constructor checks 64-KiB base alignment, the padded physical
framebuffer extent, and sector-address overflow. `ColorBuffer::from_surface` and
`canvas_from_surface` consume that same description; CPU/GPU/Display handoff is
still explicit. `BOARD_COLOR_A` and `BOARD_COLOR_B` are the board descriptions.
The CPU canvas module imports only the shared value/layout, without a GPU driver.

`CellRect::new` replaces repeated four-field literals; `full_grid()` is 50×15.
`UiGrid::clear()` fills skip ID 255. Existing `write_text` and `write_glyph`
handle single-line text and individual cells. `DepthLoad::clear_far()` clears to
65535; clear/load helpers remain explicit pass arguments.

All constructors above are pure `const fn`. They work in static descriptions;
constant value builders can also be chained. Runtime calls retain the target ABI
and language limits. For size-dependent constructors, a failed static assertion
is a compilation error in both profiles. Runtime layout diagnostics use
`debug_assert!`: target `gpu_debug` retains them, release removes their conditions
and branches before dependency analysis. Release retains nonempty physical spans
and sector endpoint checks. A retained failed assertion halts with signal 0xffff.
For a debug target, build into a new directory:

```powershell
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build core-demo --gpu-debug 1 --out target/core-demo/debug
```

Use `--gpu-debug 0` for a release target. Cargo `--release` selects the host tool
profile and does not select these checks.
Use the existing `assert(condition, signal)` when an application-specific signal
is needed. These assertions validate numeric layout, while rustc continues to
check ownership and resource use before RCC runs.
