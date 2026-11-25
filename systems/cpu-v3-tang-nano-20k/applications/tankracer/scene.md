# Scene collision storage

`rcc/scene_query.rs` reads the initial authored collision world from immutable
assets. It retains original collider coordinates, bases, bounds, radii, entity
IDs, collision properties, class methods and candidate order. It uses the same
integer narrow phase as the contiguous-array query. Rendering LOD and geometry
quantization do not alter these collision records.

The source producer decodes the original archives through the reference's
complete default ENGLAND construction. It exports initial collision metadata;
the construction RNG and full live world update remain separate integration
boundaries. The default scene asset is 38,624 bytes: 240 records, 24 class
tables, 64 ordinary candidate lists and seven deformable IDs.

## Format

The little-endian u16 header has sixteen words:

| Words | Meaning |
| --- | --- |
| 0/1 | Magic `0x5343`, version 1 |
| 2 | Total word count, at most 65535 |
| 3/4 | Object/class counts |
| 5/6/7 | Record, class-directory and cell-directory word offsets |
| 8/9 | Deformable-list offset/count |
| 10..13 | Two raw i32 grid-origin components |
| 14/15 | Grid X/Z shifts |

The cell directory has 64 `[offset,count]` pairs. The class directory has
`[maximum_slot.lo, maximum_slot.hi, method_count, method_offset]` records.
Method IDs are u16; 65535 represents an absent method. Method zero remains a
valid method. `method` reports found 1, absent 0, beyond maximum 2 and malformed
class/slot 4. The dispatcher owns current-slot changes and method execution.

Each object uses 76 words. Its first 60 encode `object_query::Collider`'s thirty
raw i32 values. Class, weight, threshold, activity and flags add ten words; six
signed mesh-bound components add six words. Object IDs index these fixed rows.
Records and method tables remain external. The CPU retains the 32-byte header
and 256-byte cell directory. Query scratch holds 32 candidate IDs, one record
and the narrow phase's bounded local values.

Asset reads restore the incoming DSEG3 mapping, including cross-page records.
No pointer into that window escapes. Ordinary queries select the clamped start
cell; deformable queries select the original global list. Both visit candidates
in source order. Each candidate loads one collider and reuses the original
broad/narrow phase. Query statuses are miss 0, hit 1, arithmetic 3, bad ID 4 and
asset/mapping error 7. Misses retain the incoming hit geometry and metadata.

## Live geometry

`scene_world::Runtime` holds the shared tick owner, deferred queues, current-slot
and RNG views, and ordinary/special memberships. Fall and Bounce borrow that same
owner. The normal entry creates its RNG view from the stable ground world's
`seed` field; contact, motion and later vehicle phases share the underlying value.
`runtime-lists.bin` supplies two u16 counts followed by the original ordinary and
special 32-bit ID lists. List counts are mutated in place across method calls.

`rcc/scene_world.rs` owns views of caller-supplied mutable collider/property rows
and spatial lists. `Scene::load_world` initializes them from the same source asset,
preserving candidate order and restoring DSEG3. Its capacity/read statuses are
4/7. `World.count` enters as collider capacity and becomes the loaded count;
callers use the world only after a successful load. Class tables remain in
the immutable asset; subtype state belongs to the corresponding live method.

Each collider occupies 60 u16 words. Offsets are entity 0, position 2, pose basis
8, optional external basis 26, bounds 44, radius 56 and basis-present flag 58.
All scalar components are raw i32 pairs. Scene updates write those rows directly;
queries copy only the current candidate to bounded narrow-phase scratch.
The separate property array has 16 words per object: class 0, weight 2,
threshold 4, activity 6, flags 8, then six signed i16 mesh bounds at 10..15.
Methods write these live rows. A later response or class lookup reads the current
values; it does not reload the initial record. The row arrays use the same stable
object index and share `World.count` as their capacity/load-count contract.
Ordinary queries read the current start-cell list. Deformable queries read their
independent global list. Invalid 32-bit IDs fail before narrowing or row access.
No window pointer or pre-update collider snapshot is retained between queries.

The ENGLAND qualification entry reserves 28,800 bytes for all 240 colliders,
7,680 bytes for their mutable properties,
8,192 bytes for 32 IDs per cell, 384 bytes for row ranges, eight bytes for the
origin and 32 bytes for the deformable list. Capacities are caller choices;
the original scene contains 150 ordinary memberships, at most 21 per cell.
The normal loader checks every live collider/property word and every membership against
the immutable source before running the original map queries. Persistent module
sequences also cover translation, pose/external rotation, removal/reinsertion,
self exclusion, the separate deformable list and full-width invalid IDs.

## Connected contact qualification

The normal scene entry also runs twelve contacts against four real plain Fall
objects selected by successful queries in the original full map. It loads their
subtype initial state and the original complete tick flags/order/queues through normal
assets. Each call runs the live query, reflection/weight response, immutable
class-slot lookup, actual Fall hit and nested EnableTick. Repeated contacts see
the preceding flags/activity immediately; the response uses the incoming velocity
while the hit receives the post-response velocity. Successful dispatch restores
the shared slot. Device playback succeeds externally and does not consume RNG.

The independent producer calls unmodified `obstacle_collision::obstacles` and
`scene_objects::collision::Collision::method` in the reference. Each contact checks
286 i32 values: hit geometry, velocity/counters, subtype state, live properties,
slot/RNG, entity flags and the complete padded tick order. Sixty-four subsequent
Fall ticks reuse those exact live rows, sampling the real map, rotating and
rebounding, removing memberships and queueing a real Stop request. Their 687
i32 values include full collider/state/property rows, tick order, both queues
and all 64 cell counts with the complete ordered membership payload. The source
dispatches the unmodified class tick with its actual vehicle-peer bindings.
Both instruction and native CPU/cache/SDRAM paths run after normal installation;
functional GPU/Display do not compete for memory. The fixture supplies independent
segments and incoming velocities; it does not claim a continuous complete car or
world tick, remaining scene methods, SPI loading or hardware timing. Queued Stop
requests remain queued in this qualification; full-world drain is a later phase.

The additional qualification assets are `hit-initial.bin`, `hits.bin` and
`fall-ticks.bin`. The first sixteen u16 words contain magic `0x4846`, version 2, selected-object
count, tick-flag count, tick-order count, breakable class, then five raw i32 values:
vehicle entity, minimum/maximum turn rate, RNG and current slot. Sixteen noise
words, four raw-i32 Fall tuning values, nine raw-i32 peer matrix values and two
queue counts complete the 60-word prefix. Full flags, tick order and both queue
lists follow as i32 values. Four `[object_id:u16,state:5*i32,cell:3*u16]` rows
finish the file; cell fields are X, Z and presence. Each 597-word contact record contains the selected-row index,
raw i32 dt/limit, segment endpoints, velocity and local forward speed (25 words),
then 286 expected i32 values. Each 1377-word tick record contains a selected-row
index, raw-i32 dt and 687 expected i32 values. The original pending notification,
flags and source RNG/noise are restored directly from the constructed state.
`cells.bin` contains all 64 original 32-KiB terrain pages, aligned to the normal
mapped-asset boundary; these are collision geometry, not a new acceleration table.
Exact sine/angle assets use the shared scoped lookup backend. These fixture
records remain distinct from the scene format.

## Runtime order and retirement

`rcc/spatial.rs` maintains live candidate rows in caller-owned storage. Each
of the 64 rows has a word offset, count and capacity. Insertion clamps the
center cell, tests the surrounding 3x3 cells with the original strict X/Z
overlap and prepends the ID in each intersected row. Removal visits that same
neighborhood and removes only the first matching ID per row. Duplicates retain
their original order. Completed row mutations survive a later work-limit or
capacity failure. A zero work limit fails even when the previous cell is absent.
This mutable grid is separate from the immutable initial asset directory.

`rcc/ordered.rs` holds caller-owned ordered 32-bit IDs. Append, prepend,
first-match removal and sentinel work limits preserve original list order.
`rcc/lifecycle.rs` shares one flags array between live ticks and deferred queues.
The world dispatches methods with ordinary calls between `prepare` and `resume`.
Preparing saves the live successor before execution; disabling that successor
advances it, and appending after the end waits until the next pass. Failure
preserves the cursor and completed writes.

Deferred stop/destruction lists are unique LIFO sequences sharing bit 32.
A pending stop can suppress a destruction request. `pop` consumes each item
before the caller invokes Stop then, for a destruction item, Destroy. Both empty
terminal pops spend work budget. Notifications added during destruction wait
for the next drain. Buffers have caller-supplied capacities; capacity failures
are explicit, while normal state transitions retain the reference ordering.

`rcc/crush.rs` advances the existing deformable records and siren, then queues
retirement on that same scheduler. It compares red as a signed byte, narrows
fading colors, detaches before queueing and retains the declared record buffer.
Detached nonempty records fail after the siren/channel prefix. Modules are
checked against persistent reference lifecycle and Crush calls independently
of the initial-scene asset loader.

## Falling-object motion

`falling::strike` implements the common initialized-Fall hit phase through its
authored EnableTick activation. Its subtype state contains angular velocity,
rotation, turn rate, slope limit and `last_vehicle`. A separate view references
the world's property row directly, sharing flags/activity with collision response
and motion; there is no property copy to commit after the method. It preserves
hit/activity guards, angular-rate writes, reverse
flags, activation, the shared current slot, turn-rate limits and later flag
clearing. Activation failures retain the preceding writes and slot; a subsequent
hit observes that live state. Device sound uses the successful external stub.
The caller proceeds to auxiliary, debris, attached-name and feedback calls when
present, in original order. The default map has 84 plain Fall objects whose
method ends after the common phase; the remaining Fall resources require those
subsequent bindings. Qualification invokes a real plain object's original method,
including bounded work, missing tick flags, duplicate enable and extreme rates.

`rcc/falling.rs` updates caller-owned angular velocity, rotation, turn rate and
slope limit, the same live property flags/activity, and the external matrix. The
source's wrapping products, truncation toward zero and signed rebound shifts
are retained. There is no numerical approximation.

The owner validates the method's work limit, peer binding and external basis
before calling `advance`. Its returned action exposes two synchronous world
boundaries. A terrain request carries the exact X/Z position after orientation;
the owner samples terrain and calls `slope` only if the query succeeds. A
retirement request occurs after the final activity/rate changes; the owner
removes the old spatial memberships and queues Stop before calling `finish`.
An error at either boundary preserves completed writes and skips the final X
rotation. The owner publishes the changed external matrix even on failure,
copying it to the pose matrix only when the source's value-alias condition holds.
Auxiliary and debris methods follow this motion phase in the scene dispatcher.

`falling::tick` connects those boundaries to the live `scene_world::World`,
shared `scene_world::Runtime` and resident ground map. Its `Step` carries the original
ID, dt, work limit, prior membership cell, resolved peer basis, effect flag and
tuning/tables. Width comes from the current signed mesh bounds. The owner resolves
the peer at its actual use point; missing peers fail only in the active alignment
branch, before the external-basis check. An inactive tick performs no rotation.
Alignment keeps its own branch even when it clears the alignment flag; X rotation
belongs to the falling branch. Terrain or retirement failures retain the earlier
matrix/property writes and still publish value-aliased pose matrices.

Statuses are success 0, invalid work limit 2, work exhaustion 3, object/capacity 4,
arithmetic 5, peer/activation binding 6, missing external basis 7 and terrain
residency 8. The caller owns map lifetime and restores DSEG3 at its scope boundary.

Qualification invokes the unmodified Fall class through a fully constructed
ENGLAND scene. Four-step sequences check matrices, rates, flags, scheduler state,
all 64 candidate rows and requested terrain coordinates. The module harness
supplies independent original-terrain answers; it does not time normal target
asset loading or claim integrated auxiliary/debris behavior. Arithmetic failure
after orientation and removal failure after an earlier cell mutation are covered.

## Bouncing-object contact

`rcc/bounce.rs` transfers the incoming vehicle velocity to the live scenery
state. Its 11 raw-i32 subtype fields are velocity XYZ, spin, ground, timer, age,
airborne, reverse spin, special and raised. Activity references the live property
row used by collision response, including writes before activation failure.
`Motion.impact_threshold` comes from `PlacedObstacle::impact_threshold`; it controls
landing and rolling stop, independently of the collision row's hit threshold.
Timer and age keep their source u16/i16 representations. The ten configuration fields are elasticity,
friction, height factor, signed hit-spin multiplier/shift, signed slow-spin
multiplier/shift, deflection, gravity and height period. Narrow signed authored
values are sign-extended when creating these rows.

Relative position uses the incoming vehicle's external basis when present.
Velocity splitting, deflection yaw and matrix publication retain the source
wrapping arithmetic and signed shifts. `reference_alias` represents the authored
matrix ownership: an aliased external basis follows the replaced reference,
while a custom basis rotates independently and a missing basis stays absent.

The hit publishes velocity, matrices, airborne and activity before EnableTick.
Activation failure preserves that prefix and the current slot, while age, spin
and RNG still have their previous values. Later hits observe this active state.
An accepted speed strictly above 250000 consumes one shared LCG draw after spin;
the threshold and signed `INT_MIN` magnitude behavior are retained. Seed and slot
are direct caller-owned views, with no copied stream or callback bridge.

Qualification calls real Bounce methods from constructed ENGLAND for both
authored configuration profiles and an additional full-width arithmetic case.
Three-hit sequences compare all collider/state words, slots, tick order and RNG.

The normal entry loads all 29 authored ENGLAND Bounce components, preserving
their state, signed configuration, cell, matrix ownership, mesh presence and
independent settling threshold. Both configuration profiles then run real ordered
contact queries and class methods followed by 54 ticks each. Landing, rolling
stop and membership reinsertion occur in the same live world already changed by
the preceding Fall sequence. Shared slot/RNG, all five ordered lists and every
spatial membership are compared with unmodified reference calls. The check reads
RNG through both the runtime view and ground field after each operation, including
the actual hit draws. No stage copies or resets the stream.

`bounce-initial.bin` has magic `0x424e`, version 1 and component count, then nine
raw-i32 reference values, nine external-basis values, a u16 presence flag, two
raw-i32 collision counters, a u16 breakable class and raw-i32 vehicle entity
(47 words). Each 50-word component row contains an object ID, 11 raw-i32 state
fields, ten raw-i32 configuration values, cell X/Z/presence, reference-alias and
mesh flags, and a raw-i32 settling threshold.

`bounce-updates.bin` has fixed 2474-word records. Its 24-word control prefix holds
operation (hit 1/tick 2), component-row index, raw-i32 dt, start/end/velocity XYZ
and local forward speed. The following 1225 expected i32 values cover hit geometry,
vehicle velocity/counters, component/cell/properties/collider, shared state, padded
lists and the full spatial grid. These are qualification fixtures; they exercise
the concrete production kernels without claiming full-world scheduler/drain,
remaining classes, continuous car control or render contention.

## Bouncing-object motion

`bounce::tick` advances the same live collider and state with the current spatial
grid and real terrain pages. It validates work before the activity guard,
narrows age/timer, integrates position and spin, then checks the height-query
period. A zero period preserves the preceding position and matrix writes.

Airborne motion clamps landing height and applies the original rebound and
rolling threshold. Rolling compares signed X/Z velocities to the friction
threshold. On stopping, it obtains the actual surface normal, aligns an ordinary
external basis, clears activity, removes old memberships, inserts at the current
position and queues Stop. Raised objects retain their separate alignment rule.
Misses perform the authored deactivation before continuing to fallback and the
remaining forces. The age limit instead returns directly after deactivation.

Default Bounce deactivation queues Generic destruction before removing grid,
ordinary and special memberships. The current slot and all completed changes
remain on a work failure. Fallback uses the original transformed face query;
reflected X/Z velocity commits before validating its required external basis.
Gravity and friction are last. These ticks consume no shared RNG.

The owner retains and eventually finishes the terrain mapping scope. The module
checker installs all 64 original 32-KiB ENGLAND cells in explicit RAM and runs
both authored profiles through short sequences. It checks all actor/collider
fields, cell changes, five ordered lists, every candidate row, slots and RNG.
Fixtures assert landing-to-rolling, Stop/destruction, real fallback reflection
and the late missing-basis error actually occur. This is instruction qualification;
normal whole-world loading, scheduling and system timing are separate boundaries.

## Qualification

```powershell
$reference = Read-Host 'TankRacerCUP Cargo.toml'
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build bench-tankracer-scene --reference $reference --out target/scene-app
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/scene_check.py target/scene-app target/scene-check --runtime target/release/cpu-v3-display-sim.exe
```

The normal CB06 application checks every packed source record, then checks real
ordered queries against unmodified `object_query::obstacles/deformables` calls.
Inputs cover both crossing directions, inside/stationary cases, self exclusion,
deformable local coordinates and all ordinary colliding class families. Query
outputs include the full hit, properties, mesh bounds and selected method.
Missing class and beyond-maximum branches also run on target.

Use `--samples 16` at build time and `scene_check.py --backend native` for a
bounded native CPU/cache/SDRAM check. All record reads still run, spanning both
scene pages. The checker uses a 600-second host limit; native work includes
installation and exhaustive state comparisons as well as simulation. The simulator uses functional GPU/Display; this boundary measures
neither shared rendering traffic nor complete game-frame timing.

`scene.bin` is the runtime payload. `records.bin`, `queries.bin`, comparison
code and reports belong to qualification. Object methods publish geometry and
memberships to the live world. Connecting their complete dispatch and vehicle
scheduler remains a separate integration boundary.
