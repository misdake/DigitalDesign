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

`Grid.page` is 65535 for ordinary CPU arrays or the DSEG3 page shared by origin,
membership rows and ranges. External properties share that page; colliders keep
a stable mapping while another owner borrows DSEG3. Queries and scene loading
select the scene page. A returned candidate list borrows that selection until
the caller finishes reading it. Grid mutations restore their incoming page on
every returned success or error. The persistent driving owner uses this contract
with the [collision working region](driving.md#collision-working-region).

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

`scene_lifecycle::dispatch` also resolves the actual deactivation and
auxiliary-completion slots. An obstacle's deactivation first invokes its
Generic parent at the current slot, then removes its collider, first ordinary
list occurrence and first special-list occurrence in that order. A parent
destruction request therefore commits before membership removal can fail.
Completion's DestroyRequest method performs a nested deactivation dispatch;
success restores the caller's slot, while failure retains the inner slot and
all completed queue/list writes. These methods schedule retirement; they do
not release components or IDs themselves. Shared metadata is read directly,
including objects with auxiliary resources.

The lifecycle qualifier checks actual Fall, Bounce and Closer class tables,
duplicate memberships, missing objects/classes/handles, missing flags and small
work budgets. `bench-tankracer-retirement` includes normal-loader cases for
deactivation and completion alongside Stop/destruction and slot recycling.

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

## Mutable mesh ownership

`rcc/scene_meshes.rs` keeps stable u16 resource slots in caller-owned metadata.
Each slot has thirteen logical words plus source, index, mutable-payload address
and payload-page tag. The caller reserves record, free-stack and name-binding
arrays. Mutable spans use the ordinary target heap or a configured external
region and remain at the same address until release. `slot_words` bounds one resource, rather than reserving that many words
for every slot. Payload and slot capacity, name capacity and heap failure are
checked before publishing an allocation. Allocation exhaustion returns -4.

The logical record is `[flags,hint,radius,materials,revision[4],geometry_words,
center_lengths[3],center_stride]`. Flags are live geometry 1 and shared 2.
An absent center length is 65535; a present empty column has length zero.
Mutable storage contains vertex XYZ, face flags and three strided center columns.
Triangle indices and attributes retain their immutable source identity. A model
supplies explicit index and current-column views. RAM models retain their full
original geometry root; catalog models use the descriptor described below.

`bind` preserves the first case-insensitive name binding. `private_copy` copies
current vertices, flags and centers, keeping hint, radius and material association.
A copy of a modified private mesh remains independent after its source slot is
released and reused. A new slot starts at revision one; LIFO slot reuse increments
its preceding wrapping u64 revision. Shared meshes are protected from private
release. Release frees the mutable span and clears geometry, centers and materials,
while preserving radius, hint and revision. Mutable resolution increments an
existing slot before checking its live payload. Invalid resource access returns -1.

Catalog-backed shared slots initially retain cold columns. A private copy streams
those columns directly into its final allocation. Resolving the shared geometry
itself materializes the same columns once, after which copies observe its current
values. The descriptor and index roots remain stable throughout. Source metadata
alone is staged during loading; vertices and flags do not pass through a second
full geometry buffer. External cold imports use a 64-word staging buffer because
the asset reader temporarily maps the same DSEG3 window. Allocation failure during resolution leaves the slot unchanged
apart from the original mutable-access revision increment.

`finish` frees every materialized shared or private span and resets the owner
counts. The caller retains ownership of metadata, immutable index storage and the
cold asset. Session teardown finishes the resource owner before retiring those
backing allocations. Original geometry fields are retained for rendering and
qualification: counts, signed XYZ, triangle indices, flags, then nineteen attribute
words per face (source offset, normal, color, sides, UV, material, material offset
and billboard fields). Material tables use loader-assigned keys, with zero for the
empty table.

The resource oracle compares full geometry and metadata after aliases, mutations,
private copies, release/reuse, version wrapping, empty geometry and independently
missing center columns. A bounded-heap check verifies -4 with unchanged slot/name
metadata on allocation failure. Auxiliary-contact qualification continues to use
the same owner and persistent payloads.

```powershell
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/mesh_store_assets.py --reference $reference --output target/mesh-store-reference
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/mesh-store-check target/mesh-store-reference 0 mesh-store
```

## Constructed mesh catalog

`rcc/mesh_catalog.rs` imports all constructed ENGLAND resource slots and original
name bindings. `tools/mesh_catalog_assets.rs` exports the authoritative original
resource owner, including current private geometry, centers and full revisions.
Initial IDs are retained. Lazy auxiliary model prototypes extend the same source
directory and use the original first-binding rules.

The version-one little-endian word format is:

| Region | Contents |
| --- | --- |
| Header, 16 words | Magic 0x4d52, version, slot/source/name counts, index words, maximum mutable span, directory offsets, payload and total u32 word offsets, name-key count |
| Source, 8 words each | Vertex/face counts, shared-index offset, logical geometry words, u32 packet offset, center stride, reserved |
| Shared index pool | Deduplicated immutable u16 triangle indices |
| Initial slot, 14 words each | Source ID followed by the thirteen logical resource words |
| Name, 2 words each | Normalized name key and original resource slot |
| Cold packet | Eight model words, complete original geometry, three padded center columns |

The eight model words are geometry length, hint, radius, material key, center
stride and three independent center lengths. Runtime source descriptors use six
words: counts, u32 cold packet offset, shared-index offset and geometry length.
The source descriptors and indices are CPU-addressable; full immutable attributes
stay in the external asset. Cold reads preserve DSEG3 and validate the asset extent.
`Catalog.models` holds the independent read-only `Sources` view: asset, source
count, maximum span, descriptor array and index array. `Catalog.store` is the
unique mutable slot owner. Moving that owner into final level state leaves the
source view usable; callers bind mutable component views after the move.
`mesh-catalog.json` retains material names/spellings and auxiliary-model source IDs
for offline consumers; target code uses the corresponding integer keys.

The ENGLAND catalog has 63 initial slots and 72 source descriptors. Its fixed
128-slot metadata, source descriptors and shared indices require 10,616 bytes.
Initial materialized columns require 5,932 bytes; shared columns remain cold.
The whole-owner qualification creates all 37 authored auxiliary copies, changes
private and shared geometry, detaches another copy and recycles a released slot.
That sequence reaches 104 slots with 17,172 bytes of materialized columns.
These payload figures exclude target-allocator headers and loading/check scratch.
The normal fragment/resource qualifier reserves an 8-KiB heap beginning at
0x2000 words; its upper boundary is 0x3000, below the validation stack. The
application recipe sets `ApplicationSource.heap_words`; other recipes retain
the compiler default. Heap boundary tags are part of the reservation even when
only a small fraction of payload capacity is allocated.
A model descriptor needs eight scratch words; direct column reads fill the final
allocation. Full game-world residency and fragment motion have separate budgets.

The normal fragment application checks every initial resource and every resource
after the mixed operations, including immutable attributes, all name bindings,
free-slot order and revisions. It then tears down/reloads the owner and checks
fresh shared/private resources and payload allocation totals. Capturing a cold
shared slot streams its immutable columns without materializing it. This proves
constructed resource import and ownership; parent scene contacts and the complete
mixed gameplay scheduler retain their own integration boundary.

### Shared geometry and motion bank

After loading the initial CPU-resident private meshes, `use_arena` directs new
materialized columns to the unused tail of the level's 32-KiB motion bank.
`rcc/mesh_storage.rs` uses a sorted free list, one length word per allocation
and adjacent-block coalescing. The region has one additional free-list head word.
Allocation failure changes neither the free list nor resource metadata.
Existing CPU payloads retain page tag 65535. External payloads carry their bank
page; releasing a slot uses its actual storage origin. `finish` frees both kinds
before resetting the allocation preference.

`view` resolves pointers without keeping a bank mapped. A direct consumer selects
the payload page with `select`, consumes the view, then restores its incoming
DSEG3 page. Fragment activation and motion instead keep their shared motion and
geometry bank selected for the whole operation. Their entry checks reject a
geometry binding in another external bank. CPU/external and same-bank copies
are direct loops; this owner does not support copies between two external banks.

ENGLAND's 37 motion reservations occupy 16,720 bytes. Their private geometry
occupies 10,272 bytes; including allocation headers and the region head, they
fit in 27,068 bytes. The catalog's additional mutation/copy/reuse stress sequence
uses 11,240 external payload bytes in 40 blocks, for 28,042 bytes including motion
and allocator metadata. CPU payload remains 5,932 bytes. These figures describe
this auxiliary stage, not the entire physics world's residency.

`--scope residency` selects bounded allocation, window-end access, exhaustion,
coalescing, motion-prefix preservation, full resource-catalog checkpoints and
representative parent chains. It checks the actual CPU/external allocation
totals and restores the bank mapping at each ownership boundary. The existing
full scope also runs auxiliary activation/update/completion against this layout.

## Auxiliary mesh activation

`scene_fragments::activate` implements AUXILIARY_HIT after the contact owner has
selected the actual class method. It borrows the object's lifetime row, mesh
owner, shared RNG, grid and real terrain map. The loaded catalog supplies the
normalized name, immutable model view and model-resolution outcome. A negative
outcome retains the device error; an absent model reports -4. Model data is used
only when the inactive object's motion has not been prepared.

Work validity precedes the active-word guard. A nonzero i16 active word returns
the collider entity immediately. Preparation binds the first shared name, clones
its current geometry, then releases the preceding private handle. A release
error preserves the new allocation. The published private handle and a fresh
face-state array precede terrain lookup; fresh motion retains the original false
prepared flag and lifetime 2000. Powerups set lifetime 1500 before sampling.
The caller reserves enough face words for catalog models; a target capacity
failure reports -4 at the allocation boundary.

Height sampling uses the same ENGLAND cell queries as driving. Missing terrain
and arithmetic/residency failures retain preceding resource and state writes.
The required private/motion bindings are checked next, then mutable geometry
resolution increments its revision before auxiliary configuration is accessed.
Activation uses incoming velocity only for a nonzero vehicle entity, transforms
it through the object's optional external basis and consumes the shared stream
in the original scatter order. The visible mesh, active word and radius 5120
publish before ordered spatial removal. A late removal failure preserves this
activation; a subsequent hit takes the active guard. Result publication occurs
only on the guard or complete success, and the enclosing contact slot is retained.

The oracle executes the original collision method on authored objects with a
local persistent mesh arena and actual terrain. It checks complete resource
geometry, face state, RNG, collider, lifetime flags and all grid members after
each call. Setup includes prepared bindings, released/shared old handles, missing
configuration, model I/O failures and first-name geometry mismatches. Explicit
state probes clear activity before reactivation. Fixture assertions require
repeat-hit stability and a real late spatial-limit failure. The normal CB06 entry
runs representative bench and pickup calls, including missing terrain and late
removal failure. Constructed resource import is covered by the catalog entry;
outer car collision dispatch and complete gameplay construction remain separate
integration boundaries.

```powershell
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/auxiliary_hit_assets.py --reference $reference --output target/auxiliary-hit-reference
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/auxiliary-hit-check target/auxiliary-hit-reference 0 auxiliary-hit
```

## Auxiliary mesh updates

`rcc/scene_fragments.rs` executes the actual auxiliary-update method (slot 22,
method 68) on the shared identity, lifetime, grid and runtime views. The caller
resolves the object's stable private mesh slot and supplies its current motion,
geometry, original i16 active word, optional render mesh and full u64 revision.
These are direct views of persistent storage; geometry release is distinct from
the resource slot's lifetime.

The entry validates class and object identity, checks work and component presence,
then increments the resource revision before accessing its geometry or auxiliary
configuration. The increment wraps across four little-endian words, including
when a released geometry payload makes the subsequent access fail. The fragment
kernel mutates its current vertex and center arrays. After gravity/bounce and
center validation, it resolves the six Y/X/Z sine/cosine values once per moving
fragment and reuses a 24-byte scratch span for its three or four vertices.
Each vertex retains the original separate axis products, wrapping subtraction
and signed truncation; shared or repeated indices are still visited in source
order. This is exact reuse, with no combined rotation matrix or approximation.
The scratch is reused for every face and does not grow with the mesh.
Completion clears activity,
the render mesh and collider radius before nested Deactivate (mode 0/1) or
AuxiliaryComplete dispatch. Success restores the outer slot; errors retain the
original committed prefix and innermost slot. The private mesh radius and owner
binding persist after completion. This method consumes no RNG.

The fixture exporter invokes original class dispatch on all matching auxiliary
owners from a full ENGLAND construction. Cases retain the world and mesh between
frames, compare all lifecycle/list/grid state, geometry, resource versions and
collider words, and exercise completion, missing data, low work limits and revision
overflow. Only initial setup creates and activates reference resources; native
creation and collision activation have their own integration boundary. The normal
fragment application runs representative persistent cases through ordinary CB06
and immutable scene/sine resources.

```powershell
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/scene_fragment_assets.py --reference $reference --output target/scene-fragments-reference
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/scene-fragments-check target/scene-fragments-reference 0 scene-fragments
```

## Fall and Closer auxiliary chains

`rcc/scene_parent.rs` runs the Fall/Closer methods around the shared auxiliary
activation, update and completion code. `State` owns the lifetime store, runtime,
mesh resources and terrain cache. The caller retains the spatial grid and supplies
an `Actor` containing direct views of the selected collider, properties and parent
component. These views refer to the same rows used by queries; calls do not copy
vehicle or world state. Inputs include the already selected contact method and
current peer/player poses. Audio and feedback are successful external operations.

Fall's hit commits flags and angular velocity, selects activation slot 4, then
publishes its remaining parent state. Its actual auxiliary-contact slot 25 runs
next; activity becomes zero only after that call succeeds. Fall clears its method
result on failure as well as success. Closer enables ticking when its flags require
it, invokes the same auxiliary slot, and changes to the fragment phase only after
success. Nested failures retain their committed prefix and innermost slot.
Fall resolves activation from its current object-property class; Closer resolves
it from the entity's class. A missing identity class therefore does not prevent
the Fall contact method's property-based activation.
The ordinary Fall hit and the class entry use the same begin/finish phases.

The tick entry selects the actual class and slot 12, validates its identity, and
executes Fall motion or the Closer waiting/closing/fragment phase. Fall's motion
accepts direct body views through `falling::tick_body`; its original world entry
uses this same implementation. Closer publishes its matrix and authored reference
alias even if rotation normalization fails. Auxiliary updates resolve the current
private resource binding, retain slot lifetime separately from geometry lifetime,
and invoke slot 22 before the original completion policy. A released valid resource
slot receives its failed-access revision increment; an out-of-range handle does not.

Destruction follows slot 2, the authored Obstacle/Generic parents, and slot 3.
Owned private geometry is released before auxiliary bindings/configuration are
cleared and before spatial removal. A late removal failure retains that cleanup.
The Fall/Closer component retires when the Generic release path removes its owner,
and factory record accounting finishes after the release slot. Repeated destruction
of an absent identity succeeds without repeating resource work.

Qualification executes the original methods on ENGLAND's six auxiliary Fall owners
and its Closer. Persistent sequences cover repeat hits, fragment completion,
model I/O failures, low work limits, released/invalid resources, Closer waiting and
closing, and repeated destruction. Each call compares complete identities, ordered
lists, grid membership, parent state, collider/properties, all local mesh resources,
face motion and shared RNG. These class-chain cases use a local resource arena;
the separate catalog entry covers full constructed resource import. The normal
CB06 entry runs representative chains with externally mapped motion and immutable
sine/angle/inverse-cosine resources. Debris/attached Fall methods and the complete
mixed-world vehicle/weapon scheduler remain separate integration boundaries.
The continuous [driving entry](driving.md) also runs all seven parent objects
against the constructed resource owner, including resource-aware bench/Closer
destruction and the next frame. Its stable level state shares identities, seed,
collision rows and the common auxiliary bank with the car. Component kind 4
identifies Closer state for complete scene capture and final release.
The default fragment entry also runs the existing kernel/resource catalog cases.
Build with `--scope parents` to select only the representative parent chains;
the checker reports that section separately and does not claim unexecuted cases.

```powershell
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/parent_auxiliary_assets.py --reference $reference --output target/parent-reference
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/parent-check target/parent-reference 0 parent-auxiliary
```

## Qualification

### External fragment motion

The level owns one 32-KiB, 32-KiB-aligned `StaticBuffer` for fragment motion
and auxiliary mutable geometry.
`scene_fragments::MotionBank` tracks its DSEG3 page and used word count. Each
reservation contains an eight-word header followed by sixteen words per face,
in authored object order. ENGLAND's 37 auxiliary owners require 8,360 words
(16,720 bytes), including every header. This is mutable runtime state; immutable
model geometry and compact mesh indices retain their resource-catalog ownership.
The bank uses neither the CPU heap nor persistent per-object CPU face arrays.

`reserve` checks the remaining capacity before changing storage or the cursor,
zeroes the reserved span, and returns its word offset. A return of 65535 means
the reservation failed. The caller publishes the descriptor only on success.
`motion` forms header/face views for a successful reservation; its count is the
current live face count, distinct from the object's reserved capacity. Level
reload starts a new cursor and initializes its reservations again. Individual
completed objects retain their motion reservation for subsequent activation.

An object's `motion_page` selects this bank; 65535 selects direct CPU-memory
views used by focused qualification. Activation and update save the incoming
DSEG3 page, select the object's motion, and restore the incoming page on every
returned success or failure. Activation reselects motion after its terrain query.
Resource and sine reads preserve the current mapping. Terrain's cached-cell
fast path checks the actual DSEG3 page as well as its cached page, so a repeated
query remains valid after another owner uses that window. No seed or arithmetic
state changes during these ownership transfers.

The normal fragment entry reserves all authored spans and verifies their offsets,
initial contents and bounded-allocation failure. It then executes existing
activation, reactivation, update, completion and committed-failure cases with
their motion in the external bank. Prepared reference setup is copied once by
the qualification helper; runtime construction reserves its initial rows directly.
A targeted repeated-cell query checks terrain reentry after a motion-bank switch.
The bank supplies level-wide storage and the parent Fall/Closer entry uses it
for both motion and private columns. The complete mixed-world scheduler remains
a subsequent integration boundary.

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

## Lap gates and restoration

`scene_laps::step` processes the live lap-object list or flushes the delayed
enter/leave queues. `Context` borrows six words per object: signed add/remove
lap, entered flag, resolved shape presence, pending Crush presence and actor
kind. Surface attachment flags are direct views into their live owners.
Fall restoration reads sorted object IDs with nine original basis columns.
The caller supplies camera XYZ in the original one-eighth world scale.

An add-lap match takes precedence over remove-lap. Objects whose wrapped X/Z
camera distances are both strictly below 19,200 queue their entity IDs;
others execute the actual class Enter/Leave immediately. Deferred entries
resolve the current entity binding when flushed, including ID reuse. The work
counter is shared across enter and leave queues. Removing a queue entry also
advances the cursor once; removing the tail restarts at the first entry.

Ordinary actions mutate ordered membership and the live spatial grid. Crush
actions mutate deformable and special lists, then activate or queue Stop when
records are attached. Pickup, train, flock and background actors retain their
class activation and list/grid rules. Successful actions publish `entered`
after dispatch. Errors retain prior list/grid writes and the active dispatch
slot. Null and above-maximum interfaces preserve their usual slot behavior.

After lap events, off-camera Fall objects with a basis backup, flag bit zero
set and zero activity restore that basis and flags to 32 before grid insertion.
An external basis equal to the old pose basis follows the restored value;
other external bases retain their value. A failed insertion retains the pose
and property writes. This path consumes no RNG and restores borrowed DSEG pages.

`scene-laps` compares complete reference class calls on FullRace ENGLAND,
including persistent lap sequences, delayed queues, attachment/pending states,
near-boundary wrapping, stale/reused identities, limits and committed failures.
`scene_allocation_checker_check.py --laps` checks selected answer mutations.
The mixed race scheduler calls this module at its own lap and camera boundaries.
