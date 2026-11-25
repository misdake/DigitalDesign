# Real-map physical driving

`rcc/driving.rs` connects the integer vehicle kernels to ordered terrain sampling.
`Car` holds views of authoritative attitude, spring, drive, motion, boost and
track buffers, and owns persistent ground, articulation, lean and contact-hull
state. The gun view addresses the car's writable vertices. `Config` holds
immutable views; the effect's turret-Z offset belongs to the live car.
`ground::World` owns the shared seed, sixteen route-noise values and the scoped
collision mapping. Geometry results are computed by the target each tick.

The normal qualification owner binds two persistent cars to contiguous control
buffers and separate gun/body meshes. Each car has its own cooldown, collision
timer, velocity deltas and damage flags. The cars share one terrain/noise context,
RNG and line/impact pool. Caller-owned views publish a peer's collision changes
immediately, before its own scheduled update begins.

## Tick order

1. Apply/decay boost using the existing vehicle prefix.
2. Transform four primary contacts with the previous track poses. Query original
   candidates in point order. Add truncated surface bumps and, while its incoming
   timer is positive, the surface-effect bump. Add the attached live deformable
   surface contribution before publishing the gap and flags.
3. Compute load from old travel, then advance the surface effect and shared RNG.
   Select materials, querying auxiliary contacts when required; resolve primary
   misses through the original fallback metric and flags.
4. Prepare spring response and pivot. For each point, compute force, perform a
   pending auxiliary query when force/cache require it, then update that spring.
   Combine response, apply penetration lift and publish contact state.
5. Run powertrain. `contact_and_power` returns here, before external responses.
6. The caller runs external responses. The driving owner refreshes the two query
   records from current base/reference poses and primary contacts, then calls
   `tank_response::cars` in racer order. For authored-scene trajectories,
   `deform_contact::respond` then queries the live deformable list, reflects/blends
   velocity, resolves slot 21 in the actual class table and compresses the same
   vertices used by the next contact step. Ordered corner sparks follow the
   method and consume the shared seed. It subsequently calls
   `ground_response::step` on live car views: ordered fallback reflection/yaw,
   synchronous sparks and last-corner track/body damage.
7. `integrate_and_pose` integrates position, applies the attitude transaction,
   advances articulation and gun vertices, and updates eligible player lean.
   Rebuild both physical track poses and publish body/base positions.
8. The caller advances synchronous effects. The driving owner calls `impact::tick`
   with the same world seed, pool and vehicle cooldown used by ground response.
9. `finish` rebuilds the contact hull directly from strided primary points, marks
   readiness and rescales `40/dt` rates. Track poses feed the next tick.

The module's `tick` remains a convenience for contact/power, integration and
finish. The normal driving owner invokes the phases separately and inserts
car/deformable/ground response and impact animation through ordinary calls.
Ordinary scenery response retains its position before ground response.
The normal owner currently exercises one model configuration in two
slots; full racer/model loading and the mixed entity scheduler are separate.

Ground response reads the previous published body pose for damage quadrants,
the external reference basis for impact offsets, and the base-position anchor
for the last-corner damage point. These are distinct live views. The player body
uses its original instance-owned mesh and selected damage vertices; successful
dents repair incident normals.

Track origins use the body basis and averaged shifted front/rear travel. Pitch
comes from the original angle table and rotates that basis afterwards. Dot
products wrap before arithmetic Q12 shifts; rotation uses truncation toward zero.

The contact adapter stages ground state and the 44 spring integers because the
original adapter commits them and RNG only on success. Query errors retain that
state and restore the incoming seed. Boost precedes this boundary. The lower
sampler updates a caller-owned deformable surface directly: its record/dirty
writes survive a later contact error, matching the source world adapter. The lower
sampler exposes original partial writes: coordinates precede queries, misses
preserve raw height/flags/previous gap, and load follows all four primary queries.

## Noise and surface effect

Initialization consumes sixteen shared LCG values:
`seed = seed * 1664525 + 1013904223 (mod 2^32)`. Noise stores the low sixteen
bits of `seed >> 4`. Sampling indexes
`((abs_wrap(x) >> 9) + (abs_wrap(z) >> 9)) & 15`, subtracts 127 and applies
wrapped products with truncation toward zero.

An active effect samples with its incoming timer, subtracts `dt` through a signed
i16 timer and consumes one RNG value even on expiry. It updates phase, the
frame-dependent impulses and pose. Expiry restores pose, clears activation and
publishes a player-slot notification. Each contact transaction seeds the effect
with the car's incoming recoil/pitch rates and turret offset. Successful contact
commits the resulting rates/offset back to that car. Ordinary sampling and expired effects
consume no RNG. The caller owns the notified world slot.

## Storage

Ground state contains 103 raw i32 values: four 17-value point records, two six-value
auxiliary records, eight effect values and fifteen aggregate values. A point
stores local XYZ, world XYZ, kind/gap/previous gap, raw height, travel, flags,
fallback normal, surface and source-node tag. Aggregates are load, airborne,
special/flagged counts, extra-present, four extras, flagged-ground count, two
materials, fallback mode, feedback and optional clear-slot (`-1` for absent).

Collision assets contain consecutive 32-KiB pages and a 64-entry cell-to-page
directory. Missing entries are 65535 and preserve nonresident query status. The
producer packs cells reached by primary and queried auxiliary points, then
exports their directory. Adding visited cells preserves the runtime format.

The frame extends the vehicle frame with its actor index at i32 slot 28 and
force-surface at slot 29. Both actors use the same frame number, in slot 26.
Suspension and sampling read the actor's player flag from slot 2. Derived
slots 5..18 are zero in the source controls. The diagnostic stream follows inputs
with comparison results, separate from production simulation modules. It checks
the original 109 physical, 24 track-pose and 103 ground values, then four
articulation values, four lean values, 25 hull values, readiness, turret-Z, all
gun coordinates, shared impact state/pool, cooldown, damage flags, ground delta,
body vertices/normals and the shared seed. The default gun has eight vertices;
the body has 28 vertices and 33 faces. A record checks 2,287 i32 values: the
first car's existing physical/effect/mesh state, its collision timer/delta, the
second car's full physical state, its cooldown/timer/deltas/damage/mesh, each
car's deformable material/attachment/impact accumulator, and the dispatch slot.
The shared impact capture always binds the first car's cooldown. The selected
actor can therefore modify either car without changing the capture layout.
The producer sizes its generated owning buffers to
these source-derived counts; the target receives only independent controls.
The drive view is captured after powertrain; contact load retains its pre-drive
value, before drive's eight-bit scaling.

Each case exports its actor count, entity IDs, presence and nine racer slots in
a 20-word setup row, including the initial dispatch slot and scene/check flags.
Immutable radius/collision/overlap bounds form a separate
13-i32 binding. State and control records are stored per trajectory so each
resource fits an approved asset bank. The owner selects the resource once per
case and reads local record offsets. These comparison streams are qualification
data, separate from game assets and acceleration tables.

The authored collision world uses the same scene/class encoder as the scene
owner. Seven ENGLAND deformable components bind 238 writable vertices to both
hit and sampling views. Their converter stores forty u16 words per component,
followed by vertex coordinates and flags. The row contains object index/counts,
buffer offsets, hit configuration, sample clamps/edges/cap and initial states;
i32 fields are little-endian word pairs and can start at odd word offsets.
ENGLAND has no attached glow records. Hit dirty/last-vehicle/sound timer and
sampling dirty/record attachment remain distinct. Each authored-scene record
also checks 749 i32 surface values in a separate stream, including all current
coordinates. Loading and execution have separate stack frames while the scene
and per-case vertex buffers remain alive throughout each trajectory.

Deformable query errors clear material but preserve the previous attachment.
A successful miss clears the attachment. A hit publishes it before reflection
and class dispatch; the compression method uses base position, while the query
segment uses physical position. Invalid classes leave the dispatch slot alone;
dispatch sets slot 21 and restores it only after a successful in-range method.
An out-of-range slot remains 21. Corner tests use strict X/Z mesh bounds through
the collider's pose basis. Spark RNG follows the inside/player/speed/mode gates,
and the impact accumulator advances even when cooldown suppresses an emission.

## Qualification

```powershell
$reference = Read-Host 'TankRacerCUP Cargo.toml'
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build bench-tankracer-driving --reference $reference --steps 48 --out target/driving-app
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/driving_check.py target/driving-app target/driving-check --runtime target/release/cpu-v3-display-sim.exe
```

The producer decodes original TANK1/SURFS/OBPARAMS, vehicle geometry and ENGLAND
archives. Reference answers call the unmodified ground pipeline, powertrain,
motion, pose, `ground_collision::State::step`, impact and damage code, including
`Pipeline::step_vehicle`, a writable gun and the real instance-owned body mesh.
State persists throughout each trajectory. Cases cover
start driving/braking, contacts across the cell boundary, reverse/boost, forced
surface and effect expiry with recoil/pitch feedback. Two additional trajectories
start their first contact at a real fallback-face center whose primary terrain
query misses; forward/reverse high-speed inputs exercise reflection, sparks,
shared RNG, saturated wear and one-shot dents. Their producer asserts that
fallback contacts and track damage occurred. The target initializes noise, loads through the normal
CB06 installer, checks each frame and restores DSEG3 before `0x600d`. Comparison
failure encodes the word index as `0x8000 + index`; query/status errors use
`0x7f04` and record the status. Ground, integration and animation errors use
`0x7f07`, `0x7f08` and `0x7f09`, respectively.

Two further trajectories run chasing and head-on cars on real ENGLAND terrain.
Each configured step visits car 0 then car 1, using one frame number. Both cars
complete contact/power, immediate car/ground response, integration, animation and
finish. A byte-audited temporary source copy exposes the original private
`Execution::tanks` and car-ground entries without changing their implementations.
The exporter asserts that the trajectories actually produced car collisions.
Every car update compares both cars and the shared pool/RNG. Car-response errors
use `0x7f0a`; an invalid frame actor uses `0x7f0b`. `--steps` counts frames per
trajectory; paired trajectories therefore contain twice as many vehicle updates.

Two authored-scene trajectories drive forward/reverse through real ENGLAND
deformables. They call the original private deformable entry and next-frame
surface view through the visibility-only bridge. Export asserts actual vertex
changes and nonzero surface height contributions once a second frame exists.
The sequences cover attachment, deformation, sparks and leaving the surface.
Scene setup, prior attachment and deformable response errors use `0x7f0c`,
`0x7f0d` and `0x7f0e`; `0x7f10` denotes a surface-capture layout mismatch.

Use `--steps 2` and `driving_check.py --backend native` for bounded native CPU
qualification. The simulator's native CPU/cache execution uses functional
GPU/Display. SPI, shared rendering traffic, ordinary scenery responses, complete
vehicle prefix/effects and the remaining render-part poses are separate
integration boundaries. Deformation arithmetic also has independent boundary
and failure-prefix module fixtures.
