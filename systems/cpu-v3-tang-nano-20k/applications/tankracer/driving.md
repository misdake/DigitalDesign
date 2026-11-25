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

## Collision working region

The normal driving entry reserves one aligned 64-KiB `DRIVING_WORLD` buffer.
DSEG2 holds the 28,800-byte collider array and stays selected throughout the
trajectory. DSEG3 holds 7,680 property bytes, 8,192 membership bytes, 384 range
bytes and eight origin bytes. Their total is 45,064 bytes, retaining the existing
row strides and per-cell capacities. The small deformable-ID list stays in CPU
memory. The root restores its incoming DSEG2/3 mappings before completion.

The recipe sets `ApplicationSource.stack_top` to word 0x8000 and reserves an
8-KiB heap starting at word 0x2000. Statics, heap and all call frames remain below
the two working windows. `target.stack_top` in the artifact describes this
compiled entry; the boot map's cold-entry SP remains the system default.

Scene loading, queries and scenery ticks select the property/grid page. Grid
insert/remove operations restore the incoming page on success and failure.
Single Fall/Bounce terrain calls borrow DSEG3 and restore the caller's stage;
the four-contact car sampler keeps its existing grouped terrain binding.
For an empty directory entry or a point outside the map, `ground::World::bind`
reselects the previously validated terrain page before preserving the original
cached-cell query behavior. Collider-backed deformable origins and bases remain
valid in DSEG2 while terrain uses DSEG3.

Frame and scene qualification functions use CSEG6; car physics and the
application entry use CSEG7. Parent methods, auxiliary geometry/motion and mesh
ownership use CSEG5. The producer assigns these library copies to the parent
segment; other application entries retain their own placement.
Vehicle preparation uses CSEG4; pickup flight, collection and light-bank code
uses CSEG3. Their ordinary far calls retain the same data owners.
Existing ordinary-ABI far calls preserve DSEG,
arguments and shared state. The normal CB06 includes the additional code COPY.
Boundary guards check the unused ends of both working pages. Dedicated checks
also exercise external-grid capacity/work-limit failure with a different incoming
page, and out-of-map terrain reentry with unchanged seed and query result.

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
   records from current base/reference poses and primary contacts. Ordinary
   scenery response runs first, using live geometry, properties and class
   methods. Its query/reflection phase publishes one selected hit. The caller
   dispatches the ordinary or resource-aware Fall/Closer/Pickup
   method on that result, retaining the same slot and error prefixes. The owner then calls
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
8. The caller advances synchronous effects. Pickup trajectories fade the shared
   pickup banks before `impact::tick`, using the live vehicle anchor and pool.
   The driving owner calls `impact::tick`
   with the same world seed, pool and vehicle cooldown used by ground response.
9. `finish` rebuilds the contact hull directly from strided primary points, marks
   readiness and rescales `40/dt` rates. Track poses feed the next tick.

The module's `tick` remains a convenience for contact/power, integration and
finish. The normal driving owner invokes the phases separately and inserts
car/deformable/ground response and impact animation through ordinary calls.
Scenery response retains its position before ground response.
The scenery trajectories run the original ordered car/scenery tick
pass, including activation at its actual frame boundary, then drain Stop and
destruction requests. `scene_lifecycle::Store` shares the current entity records,
component rows, collision grid and lifetime lists. Peer and scheduled-object
resolution use these entity records, so a released ID stops resolving on the
next frame. Unrelated actors are frozen in the qualification's initial snapshot.
The normal owner currently exercises one model configuration in two
slots; full racer/model loading and the mixed entity scheduler are separate.

The level's `scene_parent::State` owns lifetime, runtime, mesh slots and the
ground world in their final locations. Only then does initialization bind the
runtime/effect seed and resource component pointers. Each frame mutates these
same owners directly. Thirteen authored parent views bind object properties,
Fall/Closer state and auxiliary bindings without copying the persistent state
between stages. All 37 motion reservations and newly materialized fragment
geometry share one 32-KiB external bank; initial private surface geometry retains
its CPU allocation. The parent-aware drain first runs Stop, then selects ordinary
or resource-aware destruction using the same live identity and queued event.

Ground response reads the previous published body pose for damage quadrants,
the external reference basis for impact offsets, and the base-position anchor
for the last-corner damage point. These are distinct live views. The player body
uses its original instance-owned mesh and selected damage vertices; successful
dents repair incident normals.

## Pickup state and collection

`powerup` implements the authored activation distance, parachute fall, terrain
normal, settling rotations, blink counter and off-camera spin/normalization.
`scene_pickups` executes the immediate scene actions on the same property row,
collider, grid and auxiliary owner. It publishes the external basis on error as
well as success, and performs `finish_frame` after the immediate action.

The property page holds all thirty 108-byte pickup records at word 8200: fourteen
flight/control integers, an optional twelve-integer parachute pose and the
authored inventory kind. Including the existing properties/grid and padding,
the occupied prefix is 19,640 bytes. The thirty 26-word auxiliary bindings stay
in fixed DSEG2, followed by 59 words of geometry, light tuning, pickup amounts
and two inventories. The fixed-page prefix ends at word 16367, before its final
guard. Active flags, mesh IDs and gains remain addressable while auxiliary
updates borrow DSEG3 for motion/geometry. Each pickup view binds these rows
directly when selected; the scene retains one resource owner.

Collection changes inventory, removes grid membership, calls the actual slot-25
auxiliary method, clears the active flag and retains a parachute pose when
needed. Successful external sound/haptic operations precede light allocation;
inventory cooldown handling follows it. Each failing call keeps its already
written prefix. Ammo follows the source byte wrap and signed-byte clamp: values
128..255 retain their original behavior. Collection of shield/turbo/mine weapons
caps the contact cooldown only when it exceeds 300.

`quad_pool` and `pickup_light` share a 13,544-byte external page prefix: two
408-integer banks, quad records and the exhausted-quad record. A bank stores
ten rays and ten rings, their geometry/styles/handles, follow motion and fade
state. The shared line pool's existing 32-byte CPU tail owns quad 159's last six
integers and two trailing colors. Quad 160 directly aliases those two colors,
line 0 and line 1; quad -1 has its distinct record. There is one live copy of
each value. Including counters and the existing lines/overflow/tail, the shared
state is 16,816 bytes. Guards bound the external prefix and page.

Activation projects the bank through the original camera and updates the pool.
Subsequent fade advances follow motion, radii, ray angles/colors and ordered
handle retirement. Repeated rendering does not advance this simulation state.
The light stage borrows DSEG3 once per call and restores the incoming page;
collider, binding and tuning views remain fixed while it runs. Normal loading
streams asset data through a 64-word CPU buffer into the selected stage.

The driving qualifier retains the chosen pickup after the car in the original
tick order. Its first tick activates and inserts it; the next vehicle update
collects it. Three trajectories require actual mine, turbo and shield inventory
plus allocated quads, then compare all thirty flight records, both inventories,
the complete shared pool, fragment resources and world memberships. Other
actors remain frozen in these focused inputs. Lap enter/leave and player fire
are separate integration boundaries.

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
the body has 28 vertices and 33 faces. A record checks 2,291 i32 values: the
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
a 24-word setup row, including the initial dispatch slot, scene/check flags,
update count and an optional queued destruction event (object index plus one,
zero for absent; zero-based frame index). The event is separate from the
vehicle's force-surface control at i32 slot 29.
Immutable radius/collision/overlap bounds form a separate
13-i32 binding. State and control records are stored per trajectory so each
resource fits an approved asset bank. The owner selects the resource once per
case and reads local record offsets. These comparison streams are qualification
data, separate from game assets and acceleration tables.

The authored collision world uses the same scene/class encoder as the scene
owner. Each case loads the original constructed 63-slot resource owner. Seven
ENGLAND deformable components resolve their authored resource IDs into that
stable owner; hit and sampling views share its 238 writable vertices. Read-only
access refreshes both views, while hit dispatch requests mutable access before
the deformation method. This retains the original resource revision increment,
including the failure prefix of a valid released slot. The owner stays in its
final location until all component views retire, then frees its payloads before
the terrain scope closes.

The live surface packet stores forty u16 words per component followed by flags.
The 518-word packet contains configuration and state; vertices belong to the
resource owner. Independent RAM module fixtures retain the vertex-bearing
encoding. The row contains object index/counts, buffer offsets, hit configuration,
sample clamps/edges/cap and initial states; i32 fields are little-endian word
pairs and can start at odd word offsets.
ENGLAND has no attached glow records. Hit dirty/last-vehicle/sound timer and
sampling dirty/record attachment remain distinct. Each authored-scene record
also checks 749 i32 surface values in a separate stream, including all current
coordinates. A separate 28-word stream checks all seven u64 resource revisions
after each scene update. The checker also verifies that both vertex views still
address the owner's current CPU-resident geometry. Loading and execution have
separate stack frames while the scene and per-case owner remain alive throughout
each trajectory. Catalog source descriptors stay independent of its mutable
slot owner, so initialization can move that owner once before publishing
component and shared-seed pointers.

`parent-bindings.bin` describes the six auxiliary Fall objects and one Closer.
An eight-word header contains magic 0x4150, version 1, count, 26-word row stride,
row offset and three reserved words. Each row holds object/family, model source,
name key, face capacity, motion offset, current/private mesh IDs, activation,
current face count, gravity, five unsigned multiplier/shift pairs, reference-alias
flag and one reserved word. Activation/current-mesh views address the CPU buffer;
the initial private ID and face count initialize the stable parent actor fields.
The separate 37-row motion layout contains object, face capacity and
word offset. Original source and name keys refer to the constructed catalog.

Ordinary scenery frames use a semantic snapshot stream produced by
`tools/scenery_frame.rs`. Each packet starts with 24 u16 words: magic `0x5346`,
version 1, total word count, object count, component metadata offset, identity
offset/count, tick-flag offset/count, list offset, grid offset, changed-geometry
offset/count, then i32 slot/seed/next and u16 generic/obstacle class IDs; the last
three words are reserved. Offsets are packet-relative u16 word offsets.
Eight-word component rows refer to the live state/configuration payloads.
Only populated Rust components have such payloads; resource offsets describe
the encoding, not game state. Complete canonical identity records, tick flags,
ten ordered lists and grid membership follow. Geometry rows that differ from
the initial scene carry an object index and all 76 words. Every remaining row
is still checked against the immutable scene resource, including properties.
This reduces repeated qualification data while retaining full geometry coverage.
All offsets are counted in words, including packets with odd lengths.

The qualifier keeps only the sixty independent input words during an update.
`driving/frame_check.rs` reads frame and surface answers through a bounded
64-word cache, including partial final blocks and nonsequential field checks.
The source format and checked fields are unchanged. Cache reads preserve the
current DSEG3 mapping; source answers never feed a physical update.

Initial loading uses one short-lived 640-word buffer. It reads each car's
295-value section in turn, then checks the independent seed/noise section.
Additional state fields read directly into their final CPU views. Lifetime
loading uses a bounded 256-word temporary buffer. These are qualification
snapshot loaders, separate from authored game initialization.

Impact and whole-world capture each use their own short-lived 2,576-word stack
buffer; the two checks run sequentially. Whole-world comparison scratch is
64 words. Changed collider/property packets are read in separate 60/16-word
parts. The four unused impact phase probes are explicitly zeroed before writing
live fields. Persistent vehicle, identity and world storage stays live throughout
these checks. Explicit non-inlining keeps loading/capture scratch out of the
continuous update frame. No additional heap or external bank is needed.

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
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build bench-tankracer-driving --reference $reference --steps 32 --out target/driving-app
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/driving_check.py target/driving-app target/driving-check --runtime target/release/cpu-v3-display-sim.exe
```

`tools/driving_checker_check.py` exercises the comparison path itself. It changes
one source-answer bit in a fresh CB06 copy, keeping code and game inputs intact,
then requires the expected diagnostic. Cases cover both cars at a cache boundary,
the final seed and surface partial blocks, a changed scene property, a shared
resource revision and a parent fragment's velocity. These
deliberately modified packages are diagnostic fixtures, not qualified artifacts.

```powershell
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/driving_checker_check.py target/driving-app target/driving-checker-check --runtime target/release/cpu-v3-display-sim.exe
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

Two short retirement trajectories queue destruction after an actual Fall or
Bounce collision. One uses an authored object and the other marks its initial
object as transient, exercising slot recycling. They run three or four frames
even when `--steps 2` is requested: the middle-frame drain must release the ID,
remove the collider and retire components, then the next frame must preserve
those results while the car continues. The exporter asserts these outcomes in
the original world before producing answers.

Nine additional scene trajectories cover every authored auxiliary Fall and the
Closer, plus collision-to-destruction sequences for bench and Closer. Original
car contact methods receive models from the prepared original resource service;
device audio/feedback succeeds externally. Test setup freezes unrelated actors
and retains the selected Closer's authored ticking state after the car. The
reference asserts actual fragment activation and parent ticks. Destruction cases
also verify the following frame's absent identity, component and grid membership.

Parent answers use variable-length word packets: total words, resource slot/free
counts and free order, name count, seven authored name bindings, then seven
object records. Each record includes presence, activation, current/private mesh
IDs and face count. Present motion carries its eight-word header and sixteen
words per face. A private resource carries sharing/hint/radius, full revision,
vertex/face counts, current vertices/flags and the three independent center
columns. The checker streams these through the existing 64-word cache while
selecting the common motion/geometry page. It verifies retirement of the typed
parent view when the source component disappears.

The default qualification uses 14-frame car trajectories and scene branches
capped at four frames. Additional parent branches share the fixed asset budget;
the step option controls qualification length rather than game timing.

Use `--steps 2` and `driving_check.py --backend native` for bounded native CPU
qualification. The simulator's native CPU/cache execution uses functional
GPU/Display. SPI, shared rendering traffic, the remaining
vehicle effects and render-part poses are separate
integration boundaries. Deformation arithmetic also has independent boundary
and failure-prefix module fixtures.

Pickup module qualification and answer-checker faults:

```powershell
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/powerup_assets.py --reference REFERENCE/Cargo.toml --output target/powerup-reference
target/release/tankracer-physics-check.exe target/powerup-check target/powerup-reference 0 powerup
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/pickup_light_assets.py --reference REFERENCE/Cargo.toml --output target/pickup-light-reference
target/release/tankracer-physics-check.exe target/pickup-light-check target/pickup-light-reference 0 pickup-light
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/pickup_checker_check.py target/driving-pickup-app target/pickup-checker-check --runtime target/release/cpu-v3-display-sim.exe
```

The `pickup` driving scope selects the three collection chains, each clamped to
two through four frames so activation precedes collection. Kernel
fixtures cover all authored ENGLAND pickups, flight/visibility/blink branches,
inventory byte boundaries, light start/fade/projection, reset and pool aliases.
Each original call's result and committed state is checked. The fault checker
changes one answer bit in flight, collected inventory and an allocated quad;
it executes the modified package independently of artifact identity validation.
