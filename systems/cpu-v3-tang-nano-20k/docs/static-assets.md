---
id: static-assets
status: active
last-verified: 2026-11-11
---

# Constant descriptions and static assets

Declare immutable external data instead of coordinating physical addresses,
capacities and descriptions by hand. The ordinary checked application build
reads the bytes, allocates storage and constructs descriptions before target
code generation. The existing packer loads bytes and descriptions before CPU entry.

```rust
mod static_assets;
use static_assets::*;
mod gpu_api;
use gpu_api::*;

#[asset(align = UI_ATLAS_ALIGNMENT)]
const FONT_BYTES: AssetBytes =
    AssetBytes::new(include_bytes!("assets/font.bin"));

static FONT: UiAtlasDesc = unsafe {
    UiAtlasDesc::from_asset(FONT_BYTES)
};

// After opening the GPU:
// let atlas = UiAtlas::from_desc(FONT);
```

The unsafe constructor binds external storage: preserve its immutable contents
and allocation throughout use. Registration still creates the runtime GPU
resource; a static description is not a live handle. Manual `GpuMemory` bindings
and `UiAtlas::from_native` remain available.

## Placement and units

Checked whole-program builds omit unreferenced static data, asset payloads and
working buffers before physical allocation. Original declarations are still
validated. References through descriptions, methods, cleanup and producer sector
relocations retain their dependencies. Collection is conservative, especially for
unresolved receiver types; importing a library does not by itself retain all its
storage. Omitted payloads remain available to validate producer relocation metadata,
but generate no COPY/ZERO records. Source edits can therefore move later assets;
always consume generated descriptions rather than retaining old addresses.
Explicit boot-project COPY/ZERO records remain roots: literal physical addresses
do not establish an automatic dependency on a compiler-placed declaration.

| Declaration | Storage | Address interpretation |
| --- | --- | --- |
| Ordinary struct/scalar static | CPU static arena | Existing 16-bit logical word address |
| Retained `const DATA: &[u8] = include_bytes!(...)` | Packed CPU bytes and slice descriptor | Existing CPU byte-slice ABI |
| `#[asset] const DATA: AssetBytes = ...` | System-approved external banks | 16-bit first sector, each sector 128 bytes |
| `static FONT: UiAtlasDesc = ...` | CPU static arena | Small description of external storage |

`#[asset]` denotes external storage and defaults to 128-byte alignment;
`#[asset(align = ...)]` specifies stronger alignment. The only attribute option
is `align`; resource format is carried by its native description, not a kind tag.
Ordinary CPU data remains a normal const/static declaration.

The system supplies banks after subtracting existing project COPY/ZERO resources.
Allocation uses deterministic first fit in source-load/declaration order, preferring
the high bank and then the low bank. Assets cannot cross banks or overwrite
framebuffers, command slots or previous allocations. Descriptions are recompiled
when placement changes; RCC receives approved ranges, not board memory-map constants.

The configured ranges are left-closed/right-open physical byte addresses:

| Allocation order | Byte range | 128-byte sector range |
| --- | --- | --- |
| High bank first | `0x4e0000..0x7df800` | `0x9c00..0xfbf0` |
| Low bank second | `0x100000..0x400000` | `0x2000..0x8000` |

Each candidate begins at the next required alignment boundary. The allocator
skips already placed assets, filling the first remaining gap large enough for
the sector-rounded extent. One asset stays contiguous within one bank. Placement
is deterministic for fixed source order and reserved resources, not an address
stability guarantee across edits. Generated descriptions and CB06 COPY records
use the same allocation result. The core-demo atlas currently starts at
`0x4e0000`; its requested 64-KiB alignment does not reserve a full 64 KiB.

External alignment defaults to 128 bytes (64 words). Explicit alignment is a
constant power-of-two byte count of at least 128 (named scalar constants and
integer expressions are accepted). Formats can require more:
textures use 256-byte bases and UI atlases use 64-KiB bases. The atlas constructor
asserts alignment and a nonzero whole number of 256-byte glyph records, deriving
the glyph count. A bad constant description fails compilation. Alignment does not
pad the font to 64 KiB.

`AssetBytes` describes four words: `first_sector`, `sector_count`, `byte_len_lo`
and `byte_len_hi`. Sector count rounds the occupied extent up to 128 bytes;
length retains the exact input count. The loader zero-pads the final reserved sector. Sector pointers cover `[0, 8 MiB)`; the
exclusive end is not a pointer. Allocations require a representable nonzero
sector count. The CPU static arena remains 16 KiB; blobs do not consume it.
A `const AssetBytes` description is inlined and occupies no CPU static storage;
use `static` only when a stored descriptor is useful. For example, `FONT_BYTES`
above has no allocation while `FONT` stores the three-word atlas description.
No compact-pointer ABI, runtime tag or GPU command format changes.

`AssetBytes::new` is accepted only in an `#[asset]` const or static initializer. Producers
still convert fonts/textures/meshes to native bytes. A source-placed producer
resource uses `placement: "source"`; existing fixed-destination records use
`address`. Native bytes do not inherently identify a texture's dimensions,
format or mip layout; those still require appropriate metadata.

For texture/mesh constructors and named alignment constants, see
[SDK values](/systems/cpu-v3-tang-nano-20k/docs/sdk-values.md).

## Producer-generated native descriptions

`tools/native_asset_source.py` exports `Sources(output)`. A producer supplies
native payloads plus their logical metadata, then calls:

```python
assets = Sources(output)
assets.mesh("RABBIT", mesh_bytes, directory_offset, meshlet_count, sector_pointers)
assets.texture("TILE", texture_bytes, 64, "Rgba4444", mipmaps=True)
assets.atlas("FONT", atlas_bytes)
manifest = assets.write("native_assets.rs")
```

The writer creates one blob and one `AssetBytes` declaration per resource,
requests its format's minimum alignment, and emits ordinary Rust const
`MeshDesc` / `TextureDesc` / `UiAtlasDesc` values. Applications import those names
and register them with `Mesh::from_native`, `Texture::from_native` and
`UiAtlas::from_desc`. They do not repeat directory offsets/counts, format/mip
fields, glyph counts or physical addresses. Payload layout still belongs to the
native producer; `gpu_mesh_layout.native_record` and
`gpu_texture_layout.pack_levels` are the shared encoders. Native atlas glyphs
use the shared project font producer.

The returned manifest has `resources` with `placement: "source"`, byte lengths,
SHA-256 identities and declaration symbols. The application recipe supplies its
normal producer source hashes; preparation captures those inputs as before.
The core demo uses this writer for its actual font. A generated description is
immutable metadata, not a live GPU resource or a complete mesh-pointer validator.

Mesh packages may contain physical sector references. Producers leave a zero
u16 placeholder and emit `sector_relocations` records:

```json
{"asset":"RABBIT_BYTES","offset":0,"target":"RABBIT_BYTES","addend":128}
```

`offset` and `addend` are bytes. The system linker patches the little-endian
word at `asset + offset` with `(target.address + addend) / 128`, after all assets
have been placed and before loader COPY packaging. It validates the complete
record set before modifying payloads: known symbols, even in-payload source
words, zero placeholders, no duplicate writes, in-payload sector-aligned targets
and representable sector addresses. The generic RCC allocator knows none of
these native-format details. Texture and font blobs need no fixups.

The exported source snapshot retains original blobs and producer metadata;
normal packaging and `application.repack.conf` use the same finalized, padded
load bytes. Independent repacking never reapplies relocations. Moving a package
between approved banks changes its pointers and descriptions, not its pixels.

## Static working buffers

```rust
mod gpu_api; use gpu_api::*;
mod static_buffer; use static_buffer::*;
mod surface; use surface::*;

#[static_buffer(align = 4096)]
const SCRATCH: StaticBuffer = StaticBuffer::new(8192, 128);
#[static_buffer]
const COLOR: Rgb565Surface = Rgb565Surface::allocate();
#[static_buffer]
const DEPTH: DepthSurface = DepthSurface::allocate();
#[static_buffer]
const COMMANDS: CommandMemory = CommandMemory::allocate(65536, 65536);
#[static_buffer]
const GRID: UiGridStorage = UiGridStorage::allocate();
```

`StaticBuffer::new(bytes, alignment)` requests external storage, rather than a
CPU array. The checked compiler follows ordinary `const fn` calls, including
application-defined composite descriptions. Each request reserves complete
128-byte sectors in one approved bank. Empty, overflowing, unavailable or
misaligned requests fail compilation. Alignment is a power of two of at least
128 bytes; an optional `align` expression raises every allocation in that
marked declaration, without weakening a format's own minimum. Composite fields
are independent allocations, not a contiguous physical struct.

Immutable const/static declarations may be marked. References to a declaration
reuse its allocation; separate calls to an allocating helper request separate
storage. An allocating call outside a marked declaration is rejected. The same
ordered first-fit allocator places assets first and buffers second, avoiding
fixed resources and all previous allocations. Buffers are sector-rounded and
initialized by CB06 ZERO, without adding a Flash payload. The exported
`application.buffers.json` lists each request's owning declaration, byte address,
reserved byte count and effective alignment; a composite may own several rows.

Rustc checks the original initializer's types, resource visibility, unsafe and
const legality in an unused const witness. Resolved private descriptions are
constructed by compiler-generated const bindings at their owning module/library
facade; reserved names cannot be supplied by application code. The existing
module projection and checker rules remain unchanged. No target-time allocator
is introduced.

`Rgb565Surface` and `DepthSurface` each request the padded 200-KiB physical plane
with a 64-KiB base alignment. `UiGridStorage` requests the native 50-by-15 byte
grid (768 reserved bytes). `CommandMemory::allocate` requests command and push
storage independently, each at most 64 KiB. Use `GpuMemory::from_buffer` to bind
a placed extent, and register GPU resources at runtime. Descriptions do not
provide exclusive access or replace completion/Front ownership rules.

The core demo owns its working set in `rcc/working_set.rs`. `gpu_board` remains
an optional helper for legacy board presets; the public API does not depend on
it. Boot display and legacy fixed reservations remain unchanged.

## Includes and constant evaluation

`include_bytes!` accepts one literal path. Relative paths resolve against the
actual selected source file, including imported modules. Synthetic modules need
absolute paths. Missing inputs/origins are errors. Included bytes are tracked and
snapshotted as build inputs, so changes invalidate compilation and artifact identity.

Includes and placement metadata are resolved first. Rustc then checks types,
ownership, visibility, unsafe operations and const legality. RCC subsequently
evaluates the supported pure subset and generates target code. This uses the
existing checked GPU build path and adds no runtime checking mode.

Constant expressions support structs/updates, tuples, arrays/bytes, forward
constant references, fields, indexes, lengths, casts, integer/boolean expressions
and short-circuit conditions. Pure nongeneric `const fn` and associated constructors
and by-value methods
support immutable let bindings, a tail value, conditions and single-condition
`assert!`. Unsafe syntax does not enable compile-time device/pointer/heap access.
Named fixed arithmetic, scalar rounding/conversions and special functions are
also available; see [SDK values](/systems/cpu-v3-tang-nano-20k/docs/sdk-values.md).
A scalar `static fix32` occupies two consecutive words and uses the same constant
initializer serialization as fixed fields.
Known pure constructor calls are also folded in ordinary code. Dynamic calls
retain their target implementation. Static assertions are always checked;
runtime `debug_assert!` is removed for release and retained for `gpu_debug`.
Retained assertions use halt signal 0xffff on failure.
Evaluation has a 100,000-step budget and a 64-call/dependency depth limit.

Wide `usize`/`u32` sizes can be used during constant evaluation, with explicit
casts when constructing target-sized fields. Compile-time-only helpers need no
runtime ABI. Runtime-used functions retain the existing target ABI and syntax
constraints. Mutable locals, loops, early return, match, arbitrary macros,
generic const functions and general references are outside this evaluator.
Unsupported expressions fail explicitly; this is not full Rust CTFE.

## Loading and packaging

The frontend can transfer generated static initialization stores to the loader.
The image builder emits `application.static-data`: a dense little-endian image
of exactly `application.static-words * 2` bytes, removing those startup stores
before target compilation. Existing CB06 COPY loads that allocated prefix;
the protected unused tail is untouched. Direct compiler/simulator callers retain
startup stores unless they explicitly take over data loading.

Every export has `application.repack.conf` covering fixed and source-declared
external resources. Existing-input packaging reads adjacent static data:

```powershell
cargo run --release -p cpu-v3-boot-format --bin cpu-v3-pack -- target/app/application.v3bin --project target/app/application.repack.conf -o target/app/repacked.bin
```

Keep the code, static-data file, word count and resources from the same build.
Malformed extents fail. Older exports without static data keep ZERO-prefix and
startup-store behavior. Ordinary resource COPY still cannot write the protected
CPU static arena. CB06 and hardware loader semantics are unchanged.

## Source geometry and producer manifests

The ordinary application builder invokes `tools/asset_producer.py`, which supplies
the SDK import directory and the producer's own directory. Application producers
use normal imports, without discovering the repository through parent counts or
editing `sys.path`. For a standalone producer run from the repository root:

```powershell
python -B systems/cpu-v3-tang-nano-20k/tools/asset_producer.py systems/cpu-v3-tang-nano-20k/examples/hello_gpu/tools/assets.py --output target/hello-assets
```

`native_asset_source.Sources.geometry` accepts `Vertex` values and triangle indices.
Positions are source-world numbers, normals are normalized by the producer, UVs
are normalized values in 0..1, and optional RGB colors are integer bytes. All
vertices in one mesh use the same UV/RGB mode. The producer preserves triangle
order while partitioning at 64 vertices or 96 triangles per meshlet, chooses each
record's Q16.16 origin/binary step, and calls the existing native record encoder.
It creates the aligned directory, payload and sector relocation placeholders.
`PositionEncoding(origin, shift)` optionally preserves an explicit native layout;
ordinary callers use automatic quantization.

```python
from native_asset_source import Sources, Vertex

assets = Sources(output)
assets.font('FONT', 'HELLO')
assets.geometry('TRIANGLE', [
    Vertex((-1, -1, 0), color=(255, 0, 0)),
    Vertex((1, -1, 0), color=(0, 255, 0)),
    Vertex((0, 1, 0), color=(0, 0, 255)),
], [[0, 1, 2]])
assets.write_manifest('assets.rs', sources=[__file__])
```

`font` selects the project base atlas and tracks its source/data files.
`write_manifest` publishes declarations and `assets.json`, automatically including
SDK encoder dependencies; pass additional producer/helper files with `sources`.
Existing `atlas`, `texture`, native `mesh` and `write` interfaces remain available
for producers that already own encoded data or custom font/icon composition.
There is no runtime mesh builder or additional renderer allocation.

## Source images and mipmaps

`Sources.texture_pixels(symbol, size, pixels, pixel_format='Rgb565',
mipmaps=False, color_space='srgb')` accepts row-major RGB8/RGBA8 tuples. Size is
a square power of two from 1 to 1024. `Sources.image(symbol, path, ...)` decodes
an image with Pillow and tracks the input file; Pillow is needed only for image
decoding. Image assets belong within the producer's repository source tree for
manifest hashing. No implicit resizing or alpha removal occurs: use `Rgba4444`
for nonopaque pixels.

The default is a base-only texture. With `mipmaps=True`, all levels down to 1×1
are generated from unquantized linear, premultiplied-alpha intermediates. Final
levels are converted back to the source encoding, quantized to RGB565/RGBA4444
and passed to the existing native encoder. This preserves its smallest-first
order and padded mip tails. `color_space` describes source pixels and mip
filtering (`srgb` or `linear`), not a new GPU texture flag.

```python
assets.image('WALL', Path(__file__).with_name('wall.png'), mipmaps=True)
assets.texture_pixels('ICON', 2, rgba_pixels, 'Rgba4444', mipmaps=True)
```
