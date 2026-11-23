# TankRacer physical update

This application owns the raw integer single-vehicle physics port and its host
qualification tools. The reference is TankRacerCUP. Detailed source identities,
replay evidence and cycle captures belong in the external qualification output,
not in this implementation document.

## Update order and storage

The caller supplies classified contacts and invokes these stages in source order:

1. `vehicle::boost`: apply/decay boost and publish the drive inputs.
2. `vehicle::contact_stage`: compute load from **old** travel, choose the contact
   pivot, update four springs, combine their response and apply penetration lift.
   Publish `suspension[40]` to `drive[15]` before the next stage.
3. `vehicle::power_stage`: select velocity precision once, transform world to body,
   run damping/engine/gears/boost/brakes/traction/steering, correct rearweight yaw,
   transform back with the same precision and add the suspension vertical delta.
4. `vehicle::integrate_position`, then `vehicle::attitude_stage`: integrate physical
   position and run the local attitude transaction. Attitude commits on success.
5. `vehicle::finish`: publish physical pose/reference/base fields and apply the
   successful-frame `40 / dt` rate scaling.

State values are raw signed i32, stored as little-endian pairs of u16 words.
These offsets are i32 indices with exclusive range ends; placement addresses are
chosen by the caller. Stage views are refreshed from their authoritative owners.

| Array | Indices and fields |
| --- | --- |
| Attitude, 29 | `0..9` reference; `9..18` body matrix; `18..21` body position; `21..24` angular; `24..26` longitudinal/lateral drive deltas; `26..29` pivot |
| Suspension, 44 | `0..24`: four contacts, stride 6 = kind/gap/corner/travel/rate/output; `24..26` angular X/Z; `26..28` tilt; `28..34` force sum/travel sum/vertical delta/world VY/physical Y/timer; `34..38` auxiliary types/caches; `38..40` maximum/last penetration; `40` pre-response load; `41..44` pivot |
| Drive, 16 | `0..3` local velocity; `3..6` angular stage view; `6..12` track speed/boost target/engine/slip/active/gear; `12` lateral speed; `13..15` acceleration; `15` load |
| Motion, 16 | `0..3` world velocity; `3..6` physical position; `6..9` previous position; `9..12` base position; `12..16` end-of-frame rates |
| Boost, 4 | extra speed, power, power loss, signed i16 timer widened to i32 |

Configuration slot layouts are documented beside the owning kernels in
[`rcc/suspension.rs`](/systems/cpu-v3-tang-nano-20k/applications/tankracer/rcc/suspension.rs) and [`rcc/drive.rs`](/systems/cpu-v3-tang-nano-20k/applications/tankracer/rcc/drive.rs).
[`rcc/integer.rs`](/systems/cpu-v3-tang-nano-20k/applications/tankracer/rcc/integer.rs) defines wrapped multiplication, masked shifts,
arithmetic shifts and the legacy sign/magnitude truncation, including INT_MIN.
The guarded FPU polar iteration retains the exact wide fallback and success-only
matrix publication; its arithmetic policy is independent of table storage.

## Frozen-contact boundary

Inputs freeze four contact kinds/gaps, world contact Y, two auxiliary query
results/cache resets and selected surface materials. Spring forces, load, drive
and brake response, acceleration, velocity, position, angular rates and attitude
are computed by the port. This boundary excludes terrain traversal, sampling
noise/effects, external-actor collisions and articulation/render-part outputs.
The called frozen-query chain does not consume RNG. It is not a complete game tick.

## Exact trigonometric assets

The actual C entry `frame_math_initialize` and `frame_math_trig` read
`src/frame_math_tables.h`. The owning producer
[`tools/export_trig_assets.py`](/systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/export_trig_assets.py) copies those committed
integers and independently compares the bytes with the Rust loader's tables.
It does not regenerate formulas or derive production resources from fixtures.

- Sine: **20480 signed i16 little-endian entries**, including the cosine tail.
  The source contains 20480 i32 values; all fit in i16. The existing compact
  payload is 40960 bytes.
- Angle: **257 nonnegative 16-bit entries**, 514 bytes. Their u16 encoding is
  identical to the producer's i16 encoding. Entry 256 retains the terminal zero.
- Preserve every entry and the original masked indices. No mirroring or
  reconstruction is used. Both complete tables require more than one 32KiB page.

The common interface is `trig_lookup::index(Array<i16>, u16) -> fix32`, returning
an explicitly sign-extended **raw integer carrier**, without Q16 conversion.
The module provider selects one backend at compile time:

| Backend | Meaning of the argument |
| --- | --- |
| [`rcc/trig_lookup.rs`](/systems/cpu-v3-tang-nano-20k/applications/tankracer/rcc/trig_lookup.rs) | Low-memory i16 table view, retained for fixtures/oracle checks |
| [`rcc/trig_asset_lookup.rs`](/systems/cpu-v3-tang-nano-20k/applications/tankracer/rcc/trig_asset_lookup.rs) | Low-memory standard `AssetBytes` root view; reads through the existing immutable `gpu_window_asset_read` scope |

Production roots use `#[asset] AssetBytes::new(include_bytes!(...))`. The existing
packer supplies placement. The scoped SDK reader handles page crossings and
restores DSEG3; a window pointer never escapes. The argument interpretation must
match the selected backend. No runtime callback or GPU view/world API is involved.

## Host commands

Run from the repository root. Paths below are caller-supplied directories.

```text
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- OUTPUT
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- OUTPUT FIXTURE_DIR 10 vehicle
python systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/export_trig_assets.py --reference TANKRACERCUP_DIR --output ASSET_DIR
```

`OUTPUT` alone runs arithmetic qualification. With fixtures the grammar is
`OUTPUT FIXTURE_DIR [LIMIT [MODE [QUALIFIED_RUN_DIR]]]`; the limit defaults to 600.
Stage modes are `suspension`, `drive` and `vehicle`, reading the matching JSON file.
Omitting the mode selects `attitude.json` plus `sine.bin` and `angle.bin`.
The `*_oracle.rs` tools generate their fixtures by calling the unmodified reference
library; they are host-side reference adapters, not target kernels.

Additional host entries are `polar-kernel`, `trig-assets`, `vehicle-core` and
`vehicle-core-candidate`. The polar entry ignores its fixture/limit placeholders;
`trig-assets` takes `ASSET_DIR` as its second argument and uses the replay layout
specified in `trig_check::run`. Core entries also require `QUALIFIED_RUN_DIR`;
they enforce their source/harness qualification rules. These captures use direct
single-cycle instruction/data responses, excluding native-cache/SDRAM/GPU timing.

## Standard CB06 benchmark entry

[`physics_application.rs`](/systems/cpu-v3-tang-nano-20k/applications/tankracer/physics_application.rs)
is registered once by `applications/build/mod.rs` as `bench-tankracer-physics`.
The normal Assets producer emits three source-declared `AssetBytes` roots:
`PHYSICS_SINE`, `PHYSICS_ANGLE` and `PHYSICS_SAMPLES`. The last contains selected
frozen invocation inputs and all 109 expected output fields. Generated ordinary
module lookup selects the production trig reader. The normal image build owns
static initialization, placement, linked code and CB06 COPY/ZERO commands;
`cpu-v3-boot-assets` exports that same build. The checked explicit-gpu frontend
provides the existing scoped asset interfaces; this entry records no GPU commands.

```text
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build bench-tankracer-physics --reference TANKRACERCUP_DIR/Cargo.toml --fixtures VEHICLE_FIXTURES/vehicle.json --repeats 2 --out SNAPSHOT
python systems/cpu-v3-tang-nano-20k/tools/application_artifact.py check SNAPSHOT
python systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/native_bindings.py SNAPSHOT BINDINGS.json
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-native-check -- SNAPSHOT BINDINGS.json OUTPUT 10000000 600
```

[`native/main.rs`](/systems/cpu-v3-tang-nano-20k/applications/tankracer/native/main.rs)
loads each record from the packaged sample root, executes the complete physical
update and compares every output word on the target. The records select ordinary,
periodic polar-correction and airborne-boost branches. Repeats restore the same
frozen invocation snapshots; they do not claim a continuous trajectory. The
application accepts one or two rounds. The owning native qualification runner
requires the default two rounds: six invocations and 654 checked i32 outputs.

`native_bindings.py` verifies the same-build artifact before resolving its globals.
`native_check.rs` consumes that package through
`System::from_preloaded_cold -> startup::image -> boot::install_single_application`.
It reads back the installed table bytes and only observes CPU-visible phase
markers and state. It never writes placed resources, invocation state or cache
contents into the model. The target performs all record loading and verification.
The output retains `consumed.cb06`, both loaded table readbacks and
`native-report.json`, binding the consumed image and observation boundary.

The entry is synthetic `preloaded-post-boot`: CB06 installation precedes clocks,
then the production cold preparation runs before application timing begins.
SPI cost and real-loader terminal bank/refresh/clock/cache-state equivalence are
outside this boundary. Application execution uses the production fetch queue,
I/D caches, arbiter and native SDRAM service. Direct-core captures with one-cycle
word responses are a different timing boundary.

The tick interval runs from CPU-visible phase 2 to phase 3. It includes the tick
call/return and marker boundary, excluding record loading, target verification
and the cold prefix. First invocation follows actual cold preparation and record
loading; later invocations retain their real prior execution history. Neither is
an imposed all-cold or all-hit cache state. Machine cycles use the logic clock;
host deadlines bound the observer and are not a machine performance metric.

The qualified samples issue no concurrent Display/DMA/GPU memory requests.
Rendering/scanout contention, real contact queries, collisions and AI remain
outside this benchmark. The frame budget must fund those operations as well as
this physical update; an isolated tick is not a combined-frame qualification.
