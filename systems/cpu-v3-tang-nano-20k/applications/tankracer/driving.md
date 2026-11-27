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

## Complete physics qualification

`bench-tankracer-driving --scope complete` loads the same normal application and
selects continuous two-car driving, opposing cars and an authored Fall contact.
The reference adapter runs the original vehicle prepare and private
`Execution::physics` in order, with the parent Tank's intervening field
publication. Player input/route dispatch remains outside this entry.
Successful device feedback is external; authored auxiliary models are resolved
through the existing resource service. Ordinary-contact coverage includes the
creation of a real auxiliary mesh when the contact produces no velocity delta.

The target executes real contact/power, ordinary/car/deformable/ground collision,
complete `car_motion::integrate_and_pose`, all of `car_effects::tick` and successful
`car_motion::finish` on the same persistent owners. Each vehicle update checks
the existing physical/world snapshots and the full additional effects/pose page.
The source frame contains independent controls only; the target recomputes
sampling, collisions, poses, particles, pool allocation and shared RNG.

The prepare phase and immediate effects borrow the same temporary-line core
and line pool. Prefix hot rows remain in the fixed collision page. Preparation
maps the complete effects page once, publishes the pose timer, blast protection
and current sidegrip, then restores the caller's mapping. Boost inputs reach
power integration after prepare. The `complete-prepare` trajectory activates
boost, advances spin and expires a temporary effect before physics; at three
ticks it also requires traction smoke and its synchronous impact batch.
The smoke contact-index view addresses suspension column two (`corner`), with
the existing six-column point stride.

The additional image uses 10,468 little-endian u16 words (20,936 bytes) within
the existing 32-KiB `DRIVING_PICKUP_EFFECTS` allocation. Offsets below are in words.

| Offset | Content |
| --- | --- |
| 0 | Pickup banks, quad records and exhausted quad, retaining the pickup layout. |
| 6772 | Existing pickup boundary guard; padding extends to word 6800. |
| 6800 | Canonical quad count, followed by padding to word 6810. |
| 6810 | Two 956-word car rows: extra poses, spray, marks, smoke, lights, debris owner, pose configuration/input and light/prefix fields. |
| 8722 | Shared debris banks. |
| 9086 | Shared shield banks. |
| 9896 | Shared temporary-line state. |
| 10170 | Sound-draw output and camera position/forward. |
| 10186 | Spray styles/kinds, smoke sockets and pickup tuning. |
| 10358 | Two 28-word rows: middle-distance height, stored drive input and original model hull bounds. |
| 10414 | Two 26-word rows: input flags, saved/live turn limit, yaw lock, turret state and driving flag. |
| 10466 | Shared analog steering latch. |

The line pool and impact state keep their existing owners. `car_effects::Shared`
borrows those owners and the quad pool by typed views, including live counts.
Turret and body smoke poses use distinct views. Contacts retain their 17-column
stride; track matrices retain their 12-column pose stride. A sixteen-byte local
travel array supplies the pose kernel from the four suspension rows. Ready is
a single canonical boolean word. The packed physical diagnostic's acceleration
columns are refreshed from the live drive acceleration before motion.

The complete tail selects the effects page once and restores the incoming DSEG3
mapping. Lookup-table reads preserve this binding. The surface-effect slot clear
runs at its contact boundary. Normal frame assertions and the final page guard
cover the same owners. These cases retain one model configuration and selected
active scenery; full startup, enclosing Player/Tank dispatch and mixed gameplay scheduling are
separate integration boundaries.

`--scope effect` isolates two cars whose surface effects expire during complete
prepare/physics updates. The ground owner borrows the shared shield page and
bank view. Sampling clears the selected tail immediately after effect expiry,
before material, fallback and suspension queries, and restores the terrain page.
The shared write survives a later contact failure while the vehicle's pending
sampling/response and RNG remain uncommitted. A requested slot outside 0..2
returns sampling status 6 before subsequent queries. `shield_page = 65535`
retains standalone sampling with successful external writes. `contact-effect`
compares the original sampler's partial state, RNG and tail writes on diagnostic
planes; its cases distinguish failures before expiry from auxiliary failures
after the shared write.

## Complete Player preparation

`--scope player` runs the original complete Player method on the reference side,
including Tank preparation, physics and post-physics publication. The target
executes those stages on the same persistent World. Its trajectories fire Shell,
Homing and Warp, advance the race clock, exercise horn edges and hide/reveal the
vehicle while continuing through authored contacts. Route crossing, lap queues,
turret/fire, auxiliary brake,
progress and first-map weather precede the existing Tank branch. Ordinary object
collision remains active independently of whether the qualifier runs the outer
scenery scheduler.

`player_world` binds the existing input, collision, poses, inventory, prefix,
resource and shared RNG owners. Sixty resident words hold two route/racer/weather
rows. The context asset contains actual ENGLAND route tables, initial lap bindings,
backup bases, tuning, camera and weapon resources. Mutable lap fields live in
the World properties bank. Loading completes the original
resource-preparation methods without consuming RNG and selects their matching
mesh catalog and name/source bindings. The regular constructor qualification
retains its original resource catalog.

Weapon kind/slot metadata borrows columns one and two of the existing eight-word
component rows. Six 32-KiB banks reserve two projectile banks, one mine bank,
two explosion banks and one UFO bank. They share identities, dynamic collision
rows and ordered lists with scenery. Shared flight occupies effects words
10468..10521. Two seven-word Player tail rows occupy 15232..15245; blast state
occupies 16000..16373, with adjacent guards. The typed impact-batch extent is
checked against the first tail guard before either range is installed. Constructor
scratch and line storage retain their existing separate ranges.

Each Player snapshot contains its total word count and component count, sixty
route/racer/weather words, eight inventory words, 54 shared-flight words,
fourteen Player tail words, five overlay words, 54 terrain-mark words,
100 explosion-world words, 374 blast words and six camera-impulse words.
Words 677..724 contain two 24-word rows: twelve hit counts, four-word body
and turret revisions, and two expanded drive-grip integers. Mutable turret
vertices and face normals follow, with their extents supplied by the immutable
model metadata. A 68-word mine summary follows: expanded global count, list
length, padding and 32 expanded object IDs. Only active list entries are owned;
remaining entries are padding. An 86-word UFO summary follows: occupied and visible counts, two padding words,
32 checkpoint IDs and nine visible vehicle IDs. The default pair has a 1,059-word base. Sorted
component entries then contain object ID, kind, and a complete 640-word
projectile, 48-word mine, 706-word explosion or 86-word UFO record. Only the initial
route/inventory state is loaded; later snapshots are streamed through the bounded
answer cache after the update. Identical per-case reference resources share one
asset descriptor. Their source files and bytes remain independently audited.

`--scope ufos` connects live UFO projectile attachment and carrier lookup to the
same component directory, then exercises a complete player transfer. The world
walk follows the actual ordered Tick list: a hidden Player has no update, and
SHOW can append it again during the current pass. The outer frame drains the
shared queues after all live actors. Vehicle collision queries read the current
disabled flag, visibility and external matrix.

The version-2 Player context appends UFO class/resources and compact checkpoint
presence/progress plus route order to section 15. Header words 46..53 identify
these ranges. Loading checks their contiguous extents and fixed capacities.
The effects bank keeps point rows at word 10524 (240 by four words), route
order at 11484 (128 expanded IDs), occupied checkpoints at 11740 (32 IDs),
visible vehicles at 11804 (nine IDs), and counts/padding at 11822 (four words).
Guards bracket the region. These ranges precede the existing line pool at 12400;
the six weapon banks and external reservation size stay the same.

The UFO component updates in DSEG2 beside live vehicle poses and compact route
data in DSEG3. Nested target methods borrow and restore the collision mapping;
vehicle spin, collision and race flags retain separate authoritative addresses.
A 16-byte travel gather per car supplies the complete route-restoration pose
kernel. Source snapshots cover approach, fade, route restoration/reservation,
hidden ticks, visibility restoration, overlap-controlled release, departure and
queued retirement. This continuous entry currently binds racer targets; object
target methods retain their independent qualification.

`--scope clock` advances the original raw-time countdown/release clock before
visiting the live racer slots, then clamps the physics interval to 10..100 ms.
This frame stage precedes the ordered entity pass and advances even when the
Player is temporarily absent. Its global race-active state follows countdown
completion independently of a vehicle's driving flag.
The release frame sets driving without carrying countdown overshoot into lap
elapsed or the GO timer. Clock state occupies twelve resident bytes. The five
updates cover countdown, release, a 250-ms interval and a 5-ms interval; racer
elapsed retains raw time while Player/Tank uses the clamped interval. Start-stage
bits are derived from the clock. Their reference control columns are unused.
The source clock/racer snapshots are checked after every update. Complete-effect
snapshots read their driving aliases from the same authoritative racer rows.

This entry qualifies Player-to-Tank updates, post-Tank gun revision and horn
state, and ordered projectile flight alongside ordinary scenery. Finish takeover,
the remaining mixed actor/impact paths and complete race startup remain distinct
integration boundaries.

## Vehicle construction

`vehicle_startup::construct` takes immutable assembled-model metadata and the
authored matrix/position. It clears one caller-owned row, installs the shadow
hull and primary contacts, initializes prefix/control state, then calls the
existing pose kernel once for a Tank or twice for a Player. It consumes no RNG
and allocates no heap. Missing shadow vertices return -4 before writing; pose
errors return -2 with a private candidate row. The caller publishes only success.

The row has 1,760 u16 words (3,520 bytes). Nine rows occupy 31,680 bytes.
Every block uses little-endian i32 pairs unless noted. Physics fields have one
owner; the powertrain block contains its nine persistent integers, rather than
another copy of angular velocity or acceleration.

| Word | Content |
| --- | --- |
| 0 | Complete motion/poses, contacts/travel, hull, ready and rates; the hull uses twelve raw i16 vertex coordinates and one u16 radius. |
| 339 | Powertrain: local velocity, track/boost target, engine, activity, gear and load. |
| 357 | Four spring rows and the remaining response fields. |
| 437 | Primary/auxiliary sampling, surface effect, materials and cached query metadata. |
| 643 | Physical boost followed by the existing fifty-word per-car prefix layout. |
| 701 | Entity/class/player, presentation counters, input, brake, route/race, turret, inventory, collision and blast fields. The 95 defined integers end at word 891. |
| 921 | Spray, marks, smoke, lights and debris ownership state. |
| 1661 | The 27-integer pose configuration, including the instance gun pivot. |
| 1715 | Pose input; the remaining row words are zero padding. |

`vehicle_startup_assets` writes a 200-word model/spawn record. It contains pose
configuration at 0, primary-local points at 54, four shadow vertices at 78,
reset/live grip and alternate controls at 90, spawn at 102, independent
constructor inputs at 126, shadow count/radius at 144/145, and gun word count
and original vertices at 146/147. The producer bounds the vertex extent.

The normal `--scope startup` entry constructs two fresh cars and continues their
complete prepare/physics updates. It uses model/spawn records to initialize the
cars and their gun geometry. Initial vehicle-state snapshots are excluded from
this loading path; seed/noise assertions and the surrounding world fixture keep
their normal roles. Cold publication adapts a successful row to the existing
driving views. Constructor scratch occupies the unused effects-page suffix at
word 10,624, within the existing bank. Full world initialization and scheduler
ownership remain separate from this constructor boundary.

## Middle-distance vehicles

`vehicle_inactive::step` runs the complete non-AI inactive Tank branch over
caller-owned views. `State` binds physical/part poses, velocity, rates, contact
sampling, suspension travel, prefix fields and effects directly. Explicit travel
and contact strides support both the constructor's 1,760-word row and the live
driving layout. `Input` borrows the instance drive config,
model hull bounds, pickup tuning and light offset, with current dt/frame/mode,
device throttle/brake and attitude numerical policy. Stored driving controls
remain in their own persistent fields. The caller supplies the terrain owner, existing prefix
shared views, effects owner and impact context on the same pool and RNG.

Boost decay, spin and temporary expiry precede the one-point terrain query.
Expiry restores both prefix grip and the instance's drive configuration. The
branch retains countdown, alternating sign, target, auxiliary caches and pose
timer. Terrain alignment precedes the bespoke power calculation. The incoming
large-velocity decision is reused for both transforms. Position integrates
before the local attitude transaction; the full u32 frame controls normalization.

Tracks, turret, gun, attachments and bottom position then follow the body without
the full pose update. Existing spray clears before pickup fade, lights, temporary
lines, debris clear and impact animation. Existing smoke clears next, followed
by shield animation and the simplified hull. Zero particle counts retain their
payloads. Ready and motion rates retain their source values.

Failure retains every preceding write. A fallback reflection publishes normal
and velocity before the source's missing non-AI route owner returns -1. A missing
left track returns -4 only at the final hull boundary. AI route recovery belongs
to the AI branch. Full parent Tank dispatch remains a separate integration step.

The complete driving entry selects full or middle-distance updates on the same
vehicle owner. It skips full prepare, four-point contact, collisions and the full
motion/effects tail for the middle-distance branch. The effects page stays mapped
through that branch; terrain queries restore it before later field access. The
selected car's live grip is bound before power calculation. Diagnostic aliases
of acceleration, angular velocity and rates are read at capture time rather than
copied between the branches. Contact-pipeline pivot and motion pivot retain their
distinct source owners.

`--scope visibility --steps 4` drives two cars through visible, middle-distance
and visible updates, with distinct stored and device controls. Control column 27
uses bit 8 to publish a visibility choice and bit 0 for its value; zero retains
the existing owner choice. The fixture changes visibility explicitly and calls
the original corresponding Tank method; camera visibility and full Player/Tank
dispatch remain separate integration boundaries.

`vehicle-inactive` compares the complete row, live drive config and shared pools,
particles, RNG and unaffected view/camera/boost state after each call. Real cell
and trigonometric assets are immutable. The 29,592-byte fixture includes shared
state and diagnostic capture within one DSEG2 bank; terrain temporarily borrows
DSEG3. The checker verifies guards, mappings and input immutability. Its fault
checker covers physical/part poses, selective particle clearing, grip restoration,
shared spin/RNG and early/late failure results.

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

The driving loader retains the reference scenery file's eleven-section format.
Its CPU image omits the class and object snapshots: both are already owned by
`Store.identities`, which remains the source for dispatch and object handles.
The loader validates the source header before transferring sections, relocates
their offsets, and validates the compact extent. State/configuration references
remain relative to their own sections. Reference exports and expected frame
states keep their original byte layout. `scenery_loading_check.py` tests that
changes to diagnostic snapshots have no effect, while changes to canonical
identity/component state and invalid source sections are detected.

Lifetime lists use generated offsets in one CPU allocation. The free-object
stack reserves the full fixed-object capacity, rather than sixteen entries.
Lap and sign membership reserve their initial map counts because updates only
remove entries from these two lists. Deferred enter/leave queues retain the
original identity capacity. With the ENGLAND inputs these capacities are
240/54/26/256/256, occupying 1,664 words in total. The loader validates each
source list count before writing its allocated range.

The 100-line effect pool occupies words 12400..13999 of the existing effects
bank, beyond the constructor scratch ending at word 12384. Its descriptor,
overflow record and exhausted alias stay in CPU memory. `effect_pool::Pool.page`
selects the DSEG2 line bank for record access and restores the incoming mapping
on every return; 65535 retains the caller's resident layout. Inputs in a DSEG2
window that will be replaced are retained before the switch, including a span
that starts before the window edge. CPU/DSEG3 inputs retain their normal aliases.
Quad 160's cross-pool alias uses the same raw line access. Driving comparisons
and loading select the line bank around their contiguous transfers.

The focused impact qualifier runs the same original cases with resident and
banked records. Separate checks cover window-edge inputs, overflow records,
Quad 160's span across two line records, rejected handles and full-pool early
returns. Raw driving trajectories export vehicle bindings with their initial
state; full physical trajectories keep their existing bindings and answers.

The four impact batches follow the lines at word 14000. Each current target
record occupies 211 words, ending at word 14844. A typed `Ptr::as_array::<Batch>()`
view addresses these nested records directly; initialization writes each row
in place. Loading checks the compiled row stride against the remaining bank
capacity before writing. The layout is private to the target program; canonical
reference captures keep their original serialized representation.

`impact::State.page` selects DSEG2 for a complete emit/tick and restores it on
every return. Vehicle/context views remain resident outside that window. Lines
and batches share a page, so the inner pool calls retain the existing selection
throughout the particle loop. The resident convention remains 65535. The focused
qualifier runs every original case with resident rows, banked lines, and banked
lines plus batches, including early and partially committed failures.

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
uses CSEG3. Complete motion, poses and immediate effects also use CSEG3. Their
ordinary far calls retain the same data owners.
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
8. Complete physics cases run spray/marks, pickup fade, impact animation, smoke,
   shared debris, ground lights, temporary lines and shield in original order.
   The narrower component trajectories retain their selected effect boundaries.
   Pickup trajectories fade the shared
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

`damage::Mesh.page` tags private geometry with its DSEG3 bank; 65535 keeps
resident geometry. A hit reads the live pose, flags and selected vertex first,
then binds the geometry once for the dent and all incident-face repairs. It
restores the incoming page on both success and invalid-face errors before the
caller updates revisions or other vehicle fields. Projectile damage selects
`Damage.effect_page` only for DSEG2 effect rows; 65535 retains the caller's
mapping when its temporary-effect state already uses DSEG3. Each part owns its
geometry tag, so the two poses and the shared effect pool stay independent of
the private vertex/normal bank.

The `tank-hit-paged` qualifier runs the original contact cases with poses and
temporary effects in one bank and private meshes in another. It checks the
same canonical fields as `tank-hit`, incoming mappings, bank guards and the
unchanged resident geometry aliases.

## Complete post-collision motion

`car_motion::integrate_and_pose` binds physical position, previous position,
velocity, angular rates, acceleration, pivot, articulation, lean, contacts and
the full `vehicle_pose::State` directly. It validates positive dt before stores,
integrates physical position and then runs the source attitude transaction.
The 116-byte local attitude transaction is required by the source commit rule:
failed normalization preserves integrated position while leaving the previous
body matrix/position and angular state intact.

After a successful attitude commit, the existing body kernel updates recoil,
pitch and selected gun vertices. Invalid vertices retain the earlier body and
mesh writes. Player lean runs under its original player/mode gate. Complete
part poses then use the updated articulation recoil/pitch, independent pose
inputs and four travel values. The sampled pose input's dependent recoil/pitch
columns are ignored and replaced at this boundary.

`finish` runs only after immediate effects succeed. It publishes the four-point
contact hull, sets ready and applies the original wrapping multiply and signed
division to all four `40/dt` rates. Contact stride and gap offsets bind either
compact contacts or the live terrain rows. Both numerical attitude policies use
the existing integer kernel and immutable original lookup tables.

`car-motion` compares complete `Motion::integrate_and_pose` and `finish_frame`
calls using the actual ENGLAND model configuration, including repeated calls
after normalization, gun-vertex and pose failures. The canonical motion view
occupies 678 bytes; the representative mutable gun mesh occupies 36 bytes.
Input/configuration immutability, an external-bank guard and DSEG restoration
are checked. `car_motion_checker_check.py` exercises selected answer mutations.
Complete physics cases select this motion/effects sequence; the narrower
component trajectories retain their existing selected pose boundary.

## Player controls

`player_control` supplies the ordered control kernels before parent Tank dispatch.
`Input` borrows the seven drive columns, two start-held/fast-start columns,
the separately owned prefix countdown throttle, and shared analog latch.
Its drive columns match `drive::Frame` at 19..26.
Device sampling supplies five key bits and two normalized axes; device access
and its failures remain outside this arithmetic boundary. The input kernel
retains reverse selection, signed ramping, digital precedence, analog latch,
twin-track mixing, countdown launch locks and the original clamping locations.
It returns driving and auxiliary-down bits for subsequent calls.

The turret kernel borrows its velocity, cooldown, centering, moving, center-lock
and two pressed fields. Its signed-i16 angle occupies the raw i32 pose column;
each angle store performs the original i16 wrap and publishes sign extension.
Cooldown equality, ordered left/right acceleration, both-key centering,
damping and sign-crossing stops preserve the original narrow arithmetic.

`Brake` directly binds the same drive columns, saved turn limit, reverse-braking
flag, live maximum turn and yaw lock through independent views. `brake` implements the fast handbrake
override and temporary-effect transition; `restore` performs gradual restoration.
Surface slide remains unsigned when comparing against the signed live override.
The saved turn limit, rotation and lateral override retain their i16 conversions.

`Collision` uses the same adjacent impact/marker counters as scenery response,
plus live ground/tank deltas and obstacle impulse. `collision_decay` keeps
unsigned magnitude comparisons, signed marker saturation and impact expiration.
It runs after input and before route/turret/fire when driving is active; race
activity controls its mutations. These kernels consume no RNG.

`player-control` compares the full shared state after each original input,
turret, brake, restore or collision call, including ordered short prefixes.
The two input flags, countdown throttle, saved turn and reverse-braking fields
use separate addresses with boundary guards, so adjacency cannot hide a stale
field offset. The same views bind the continuous driving owner.

`--scope controls --steps 6` connects sampled countdown, launch eligibility,
steering, turret and auxiliary-brake updates to full Tank prepare/physics.
`--scope fast-controls --steps 2` starts from a moving car with consistent
world/local velocity, activates the fast brake and restores its live model turn
limit. Source coverage checks require the corresponding state transitions.
The model maximum turn is selected per car before power calculation. Sampled
keys and the external start/driving gates are independent controls; derived
drive columns are overwritten by the target's own input result before physics.
Control column 29 uses high-word bit 0 to enable sampling, low-word bits 0..8
for keys, bits 9..12 for the start stage and bit 13 for the force-surface flag.
The sampling protocol is decoded before physics; column 29 then holds only
the original force-surface boolean. These trajectories qualify sampled
controls followed by Tank updates. Full Player route/lap/fire dispatch and the
outer race clock retain their separate integration boundaries.
It checks read-only controls/tuning and buffer guards as well as answer words.
The representative state occupies 128 bytes and the kernel allocates no heap.
`player_control_checker_check.py` exercises selected answer mutations. The
enclosing route/fire and parent Tank dispatch retain their separate owners.

## Player route and clock

`player_route::Routes` borrows linked checkpoint metadata, the original 8x8
cell ranges and ordered object IDs. Each point has seven i32 columns: presence,
progress, next-binding kind/entity, lap, hairpin and width. The nine header
columns hold grid origin/shifts/radii and backwards/fallback metadata. The
caller binds the metadata bank for the query phase; collider reads retain the
current DSEG mappings. Route queries use the shared live identity and collision
owners and consume no RNG.

`search` selects progress or nearest queries. Both retain the original row,
column and item traversal, unsigned work limit and stable first-hit ties.
Nearest uses all three position components and unsigned approximate distance.
Progress uses the original forward-plane and hairpin tests, signed XZ metric,
linked-point score correction and lap scaling. An unresolved next binding is
an error only when the original query actually needs that binding. Outputs
are committed on successful queries, including the original no-match result.

`Car` binds 44 bytes of player-route state and the shared 12-byte racer row.
`cross` uses the published pre-movement position, commits checkpoint/passed
before lap-record writes and retains the signed-i16 lap increment. `progress`
publishes the score before applying fallback and staggered wrong-way updates.
`warning` always publishes previous score, including frames without a warning
test. The timer narrows to i16 at the original writes.

`clock` updates countdown, GO release and display stage in a 12-byte view, then
visits contiguous three-column racer rows in order. The launch edge enables
driving without adding countdown overshoot to elapsed race time. `physics_dt`
clamps raw elapsed time independently to 10..100. Input/countdown control,
route crossing and progress remain before parent Tank movement in the owner.

`player-route` compares every ENGLAND gate, real spatial queries, partial
failures, fallback, wrong-way state and three-racer clocks in short sequences.
It checks complete read-only metadata images, identities, colliders, controls
and bank guards. Cases use both CPU metadata and a separate DSEG3 phase bank.
`player_control_checker_check.py --routes` exercises selected answer mutations.

## Player scene stage

`scene_player::prepare` connects the primary non-AI Player stage to the shared
identity, collision, lifecycle and weapon owners. Its car fields are views of
the existing input, pose, collision, inventory and tuning data. The caller
resolves the Player Tick and finish takeover before entering this stage; Tank
visibility, preparation and physical motion follow it.

With sampled keys, input/countdown runs first. An active driving result enables
collision decay, then real gate crossing and lap events when the race route is
active. Changed laps run immediate visibility actions before the delayed queues
are flushed. Turret control follows, then fire uses the real shared factories.
Brake updates consume the same input and live model tuning. Only a restoration
branch reads the previous material's slide multiplier; an invalid material can
therefore fail after a successfully created projectile and committed inventory.
The launch-eligible output resets on entry and commits after successful input
and brake processing, including the countdown path.

Route progress follows even when sampled keys are absent or driving is gated.
The route metadata bank is selected only for a route query and restored before
lap actions or fire. First-map route-weather selection retains the source's
unsigned score, first-frame and stagger rules, including the unchanged/sentinel
fast paths and unsupported-weather error. It consumes no random values.

The authored lap directory remains fixed. A transient weapon's visibility and
mesh presence belong to its normal component/header, so the qualification
derives its extra lap-binding projection from those same fields instead of
maintaining a second mutable directory. The method return from fire is discarded
by Player; its committed world, inventory, recoil, effect and RNG state remain
part of the comparison.

`scene-player` compares this combined stage against a body extracted verbatim
from the original Player method. The reference overlay preserves every source
file's original bytes and appends that body plus its private-call entry;
source/body/bridge hashes and append readback are recorded before compilation.
Cases cover input gates, real lap transitions and queues, firing different
weapons, material failures after fire, route errors, weather errors and
successive calls after partial failures. `scene_allocation_checker_check.py
--player` exercises selected answer mutations. Parent Tank and complete mixed
world scheduling retain their separate integration boundary.

## Track particles and ground effects

`track_effects` exposes direct spray, mark and ground-light updates for the
post-movement effects stage. The caller supplies views of live vehicle inputs;
kind, contact, track and height strides are expressed in i32 columns, so the
entry points can read existing contact/pose arrays directly.

Spray has a three-column timer/side/count header and eight twelve-column
particles (396 bytes). Existing particles advance before the last free slot
is chosen. Narrow life, size, floor, kind and rotation fields retain their
source conversions. The kind table contains sixteen sign-extended i8 values;
each of six arithmetic style rows supplies spin, initial size and gain/shift.
The retained particle kind identifies its render style. Invalid kinds or
material/side indices preserve all earlier particle/count/timer changes.

Ground marks occupy 352 bytes: two active flags, movement, FIFO counters,
endpoints and 68 quad handles. Shared strided contacts and track matrices
produce the joining quad. FIFO release precedes allocation; a tail error
retains the newly allocated quad. Pool exhaustion, overflow handles and
material changes retain the source's active flags and ordered writes.

Brake/reverse ground lights occupy twenty bytes: mode, active flags and two
handles. Player visibility and non-player active/disabled gates keep their
different rules. Steady zero-throttle braking releases before allocating again.
Existing-light updates change positions only; their material and color persist.
New quads use the source additive style, color, UVs and reversed corner order.
Allocation exhaustion still publishes the source active flag and alias handle.

The complete representative spray/marks/lights state and shared quad/line pool
occupy 14,320 bytes, including 13,552 pool bytes. They fit in one 32-KiB phase
bank; the updates allocate no heap. `track-effects` compares all these fields
after each source call, including full pools, aliases, invalid handles, FIFO
boundaries, signed overflow and ordered shared-pool sequences. It also checks
input/style immutability, bank restoration and the unused bank guard.
`track_effects_checker_check.py` exercises selected answer mutations. The
remaining vehicle effects and enclosing complete Tank Tick are separate entries.

## Vehicle smoke, debris and shield

`vehicle_smoke` directly updates 700 bytes of damage/traction smoke state. The
ten-column header holds the timer, side, penetration memory, count, damage,
sockets and damage timers; fifteen eleven-column rows hold the particles.
Fields with byte or word semantics are extended to i32 at their source writes.
Incoming free slots first serve turret/body damage, then ordinary smoke. Socket
lookup follows the RNG draw and particle stores, retaining partial failure.
Severe player slip calls the existing `impact::emit` synchronously with the same
cooldown, contact points, seed and line pool used by collisions earlier in the
vehicle update. Position and corner strides bind live contact arrays directly.

`vehicle_debris` owns two 364-byte banks. Each has eight particles, a floor and
an optional entity owner. Borrowed owner rows contain entity, active marker,
cooldown and previous-plane byte; their i32 stride is supplied by the caller.
Bank theft finds the first matching live entity and immediately clears that
car's active marker before assigning the new owner. Missing owners retain the
earlier sound RNG, cooldown and collision-accumulator writes. Behind-camera
non-player emission retains its distinct state/RNG prefix. Particle colors keep
the three discarded draws per component. `clear` resets active markers, owner
and cooldown while preserving geometry, colors, floor and previous-plane state.
Sound output publishes its consumed random value; device playback remains external.

`vehicle_shield` stores nine 180-byte orbit banks. Each contains four orbits and
one tail column. Initialization consumes five shared draws per orbit and leaves
position payloads intact. Enabled updates validate the selected bank, seek yaw
rate and pitch in source order, transform the four orbit positions and advance
size/spin with the original integer semantics. Disabled updates skip validation
and RNG. Immutable trigonometry tables use the existing asset lookup backend.

`vehicle-effects` compares all three modules, peer-owner state, the shared seed
and the complete synchronous impact/line-pool snapshot after every call. It
covers selective reset, pool exhaustion, delayed socket errors, bank theft,
arithmetic/angle failures and mixed calls on one seed. Persistent data lives in
an external phase bank while table lookups borrow and restore the other window;
input/tuning immutability and the unused phase-bank suffix are checked.
`vehicle_effects_checker_check.py` checks selected answer mutations.

## Ordered car effects

`car_effects::tick` runs the complete source effects stage: spray and marks
(subject to the special-player gate), pickup fade, impact animation, smoke with
synchronous impact emission, shared debris, brake/reverse lights, temporary
electric lines and shield orbits. Each call returns immediately on its first
failure, preserving earlier state, pool allocation and RNG changes.

`State` binds the current car's persistent particles and marks. `Shared` binds
the existing shared effect banks, peer debris rows and quad pool. The impact
context's line pool is borrowed directly by temporary effects; there is one
live count and one set of exhaustion aliases. `Input` contains sampled scalar
values and views of the authoritative contacts, transforms and poses. Pickup,
debris, temporary lines and shield follow base position; the lights use physical
position. Spray uses the reference basis while lights/shield use the optional
external basis. These distinct inputs retain their source meaning.

The `car-effects` qualifier calls the original private `car::effects` method
through an appended reference adapter. It loads the actual ENGLAND car/profile
and initializes pickup/electric effects through their real factories. Three-call
sequences compare every effect, pool alias, peer row and shared RNG, including
errors in early and late stages, an oversized owner directory and repeated calls
after failures. The external phase bank holds 26,400 bytes of diagnostic state,
including a duplicate impact-pool capture used only for comparison. Bank guards,
mapping restoration and immutable input/tuning checks bound each sequence.
`car_effects_checker_check.py` exercises selected answer mutations. Parent Tank
physics, full initialization and mixed-world scheduling remain distinct owners.

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

Impact comparison reads the line pool directly and serializes one 22-word
particle row at a time. Batch headers share that row; the four unused phase
probes are checked as zero. Whole-world comparison uses an eight-word row for
canonical identities and component records, plus the existing 64-word comparison
scratch. The row writers also serve the full diagnostic serializers. Every
original field remains checked, including inactive particle payloads and retired
identity slots. Changed collider/property packets are read in separate 60/16-word
parts. The 111-word motion reservation table lives only in its loading function.
Persistent vehicle, identity and world storage stays live throughout these checks;
the checks use the existing heap and external-bank reservations.

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

## Route restoration and part poses

`vehicle_pose::update` implements the complete original part-pose update over
caller-owned views. The logical state is 104 i32 values (416 bytes): physical,
reference and base positions/basis, body, two tracks, turret, gun, two attachments,
bottom position and recoil factor/shift. Each contiguous part pose stores nine
basis values followed by XYZ. Physical and body-position views keep their distinct
commit points, including an error on the second track. The caller supplies stable
views of each logical column; this stage owns twelve scratch bytes for one vector.

The 27-i32 configuration contains travel gain/shift and track length, both track
offsets, turret/gun offsets, both attachment offsets, bottom offset and gun pivot.
The serialized 18-i32 environment retains source timer, ambient/session flags,
phase, recoil mode, signed turret yaw, recoil/pitch, external-basis kind and its
nine custom matrix values. `Environment.timer` selects the current call's u16
timer separately from that serialized template. Travel is four raw i32 values.
Trigonometry uses the existing exact asset tables and restores its mapped page.

Body/reference publication precedes timer and ambient wobble. Each track uses its
own travel pair and removes the last applicable wobble before rotation. Turret
recoil uses the selected external basis; the gun then rotates around its authored
pivot. The unchanged turret basis is exactly the gun basis before pivot rotation,
so the pivot difference reads that basis directly. Attachments, base position,
physical commit and bottom position follow in original order. Wrapped products,
arithmetic versus truncating shifts and original angle errors remain explicit.
The result is zero or -2, and prior writes survive failure.

`vehicle_transfer::restore` walks the ordered checkpoint list backward, skipping
non-progress and occupied checkpoints. It borrows current scene colliders and
identity bindings, a point-presence/progress table and the occupied list. The
checkpoint and unsigned passed count publish at their original positions in the
search. Signed-i16 laps wrap when the selected progress crosses the circuit.
Work accounting uses unsigned comparisons and preserves each failure prefix.

After resolving source and saved positions, restore writes physical XYZ with the
6,144-unit lift, transforms the direction through the current body basis, scales
it into the angle range and rotates yaw. It calls the complete pose stage with
the live temporary timer. Only then does it clear driving input, velocity,
acceleration, angular motion and track-slip rate. Status and height have separate
outputs; a negative returned ground height is successful. Height publishes only
after the whole operation succeeds. Both modules borrow persistent state directly
and consume no gameplay RNG.

`vehicle-pose` qualifies complete original `pose::State::update` calls, including
both wobble sources, recoil modes/bases, pivot/offset wrap, legacy shifts, and
first/second-track failures. `vehicle-transfer` calls the original restore routine
on real ENGLAND checkpoints, colliders and identity data. It compares every pose field and all affected
car columns, and verifies the scene, route, configuration and identity inputs
remain unchanged. Cases include occupied points, missing bindings, work limits,
passed/lap wrap, signed heights, pose failures and subsequent restores. These
entry points qualify the reusable kernels. The UFO actor combines them with the
ordered target-method sequence described in [weapon state](weapons.md#ufo-transfer-update).

`scene_vehicle_transfer::dispatch` resolves the actual vehicle class table and
executes MODE, COLLIDABLE, SHOW, RESET and MOVE on these same car views. MODE
sets the spin-disabled byte; COLLIDABLE sets collision-disabled; RESET toggles
driving. Their Value payload defaults to zero for other payload kinds. MOVE
uses ordered checkpoint restoration and returns its signed ground height as raw
u32 bits, independently of its status. These mutations require a live vehicle
component, while SHOW can queue a Stop or dispatch Activate without that component.

Common identity/tick/queue methods execute before vehicle lookup. SHOW's nested
Activate uses normal entity dispatch, so class overrides retain their actual
method semantics. Successful calls restore the saved slot and publish their
result; failed calls retain the current slot and committed state while leaving
the caller's prior result unchanged. An above-maximum interface succeeds with
zero and retains the requested slot. MODE, COLLIDABLE and RESET retain the source
behavior at a zero work limit; tick and queue operations apply their own limits.
The dispatcher consumes no RNG and borrows the live route, identity and lifecycle
owners. A bound class with an absent or session object returns -1 for a vehicle
method; a vehicle handle whose component/context is missing returns -4. Vehicle
and obstacle target methods share the UFO actor's ordered call sequence.

`scene_obstacle_transfer::dispatch` implements obstacle MODE, COLLIDABLE and
MOVE using the same class, identity, collider, component metadata and grid owners.
MODE removes the first matching ordinary/special membership and appends to the
other list, preserving duplicates. COLLIDABLE inserts into or removes from the
spatial grid. Removal retains the stored cell; a successful insertion publishes
the new cell. The caller resolves the current object's mesh in the resource
owner and supplies `shape_present`. A resolved mesh uses `(radius + 352) * 16`;
the absent shape uses 5,632 units. Radius and position come from the live collider.

MOVE publishes XYZ before removing the old grid membership, then inserts at the
new position and returns Y as raw u32 bits. A later component or traversal error
preserves the new position and any earlier cell removals. The same path supports
fixed and banked colliders; grid and collider helpers restore their borrowed
DSEG mapping. Position payload and output views must remain valid while these
helpers borrow DSEG3. Class-table null, above-maximum, base methods and output
commit retain the existing scene dispatch behavior.

The `scene-vehicle-transfer` qualifier uses real FullRace ENGLAND class/resource
tables and captures the whole scene, live car state, dispatch output, queues,
tick cursor and route inputs after each call. It includes nested class overrides,
missing vehicle/context, signed height, partial pose failure and work limits.
`scene_allocation_checker_check.py --vehicle-methods` mutates selected expected
states to verify the answer checker catches failures at their affected boundaries.
`scene-obstacle-transfer` additionally covers live grid order, duplicate inserts,
default shape radius, missing components, signed/out-of-map moves, appended
colliders, and partial grid removal before work exhaustion. The corresponding
answer mutations use `scene_allocation_checker_check.py --obstacle-methods`.

```powershell
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/pose-check target/weapon-reference 0 vehicle-pose
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/transfer-check target/weapon-reference 0 vehicle-transfer
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/vehicle-methods-check target/weapon-reference 0 scene-vehicle-transfer
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/obstacle-methods-check target/weapon-reference 0 scene-obstacle-transfer
```

## Post-physics Player publication

`player_tail` compares the four bound gun vertices before and after physics.
The snapshot retains both the optional gun and per-index validity; a changed
snapshot increments the gun presentation revision as four wrapping u16 words.
This publication precedes propagation of a physics error. Horn state follows
only successful visible primary-player updates. Hidden and non-primary branches
retain their source ordering.

Each seven-word tail row contains horn-down, horn-latched, the signed i16 horn
timer and four little-endian u64 revision words. The horn rising edge starts
1000 ms; held and released inputs preserve the original latch and signed-time
behavior. At expiry, the caller visits racer slots 2..9 in order and clears the
timer only after all referenced non-AI vehicles are present. AI anger is owned
by the later opponent phase. Audio delivery is an external successful device
operation. `--scope tail` selects the three-frame horn and hidden-vehicle
trajectories. The standalone `player-tail` qualifier also supplies controlled
post-physics geometry and errors to the original extracted tail body.

The driving qualifier packs initialization independently from later answers.
Each row contains both cars' consumed initialization slices, seed/noise and the
24 auxiliary input words. Identical packed rows share an offset in `initial.bin`;
`initial-offsets.bin` holds one u16 word offset per case. The full diagnostic
source remains in the host export. Runtime loads the same consumed words and
recomputes derived fields as before; later reference streams retain their format.

The driving owner places deferred lap enter/leave lists in property-page words
15008..16031, with guards at 15007 and 16032. Each list reserves 256 u32 IDs.
This follows the player context's bounded 10000..14999 range and precedes the
page-end guard. Both lists share the property's existing 32-KiB allocation;
free-object, lap-membership and sign lists retain resident storage.

## Ordered projectile flight

`weapon_world` calls the existing projectile flight and trail/retirement kernels
at each entity's position in the shared lifecycle pass. Its vehicle views borrow
current poses and velocities; shared flight, overlay and RNG retain one owner.
Metadata reads use bounded local scratch. The effects bank is selected for the
flight phase and restored before returning. Overlay presence and RGBA occupy
words 15248..15252, with guards at 15247 and 15253. Terrain-mark state uses
15256..15309 and camera impulse uses 15312..15317, each with adjacent guards.
All three ranges belong to the existing effects bank. The Player packet
compares these owners together with live explosion components and blast slots.

The source's complete Player call reborrows the scheduler's live factory Context,
so fire, collision callbacks, later ticks and the Stop drain observe the same
identities and queues. The scheduler's saved successor controls when an appended
projectile first runs. The initial one-entity pass defers its first flight until
the next frame. `--scope flight` selects Shell, Homing and Warp sequences; Warp
additionally queues destruction before the third pass to exercise Stop, class
destruction, release and component retirement on the same owners.

The common auxiliary drain resolves weapon components before dispatching Stop
and destruction slots. Ordinary parents continue through their existing resource
owner. Successful weapon destruction publishes identity completion only after
both destructor methods succeed.

Ground impacts call the original shared impact/explosion kernel with the current
terrain, marks, pool, component factories, lifecycle and RNG. The created
explosion inherits its emitter before the projectile impact flag is published.
Concussion and Debris then apply their flash and racer-slot-ordered area force.
Explosion method 93 runs at its ordinary entity position, including shared
band/ring updates and delayed retirement. The scheduler's saved successor also
determines whether a newly appended explosion can run in its creation pass.

Shared mark/blast operations bind the effects bank in both DSEG2 and DSEG3:
blast records use the former, while the driving Quad view uses the latter.
Explosion component access resumes only after restoring its DSEG3 bank. This
selection covers each complete allocation, animation or release operation,
including failure returns; Quad vertices do not trigger individual page changes.

The context's sixteenth section stores the explosion class, resource materials,
camera basis, crater and effect timestamp. These immutable inputs are separate
from per-frame expected state. Complete-effect verification reads the live quad
count from its shared descriptor, including allocations after the vehicle tick.

`--scope impact` selects three eight-frame downward launches: Shell, Warp and
Concussion. Each reference sequence requires an actual terrain mark and live
explosion. Their nonzero effect timestamp exercises band/ring animation; the
Concussion sequence also queues explosion destruction and checks pool release.
The baseline `--scope full` and the dedicated impact suite are built separately
to keep their diagnostic answer streams within the asset banks. Optional
`--case N` selects one case within any scope for focused native checks.

## Live projectile contacts

Vehicle contacts resolve the same current car, prefix, pose, collision and
inventory owners as the driving update. Reflection precedes impact creation;
successful reflection transfers the projectile owner and skips damage. Normal
Shell, Homing and Warp contacts create the shared explosion before applying the
original hit coefficients, response timer, impulse, hit counts, damage smoke and
temporary effects. The response timer addresses suspension word 66; it is
distinct from the collision cooldown. Warp publishes both the prefix grip and
that car's live drive-configuration pair.

`weapon_hits` builds short-lived typed views of body and turret damage targets.
The second pair is addressed through its typed array element, retaining the
record stride. Damage flags and revisions remain resident. Body geometry keeps
its existing owner; two turret rows occupy effects words 15320..15987, with
guards at 15319 and 15988. Each 334-word row contains four selected vertices,
mutable vertices, immutable face indices and mutable normals. Loading validates
the source extent and the vertex/face capacity before installing either row.
Smoke fields retain the complete phase's two-word stride. No vehicle state or
private geometry is copied back after a hit.

Track damage publishes an existing body's revision when its one-shot flag
changes, including a subsequent geometry error. Projectile damage publishes the
selected part revision after successful vertex and incident-normal repair.
The revision uses the source's four-word wrapping counter.

`--scope contact` runs ten-update Shell, Homing, Warp, reflection and authored
local body/turret damage trajectories. The target is stationary and unscheduled
in these focused cases; all its changed fields, geometry, effects and shared RNG
remain checked after each world pass. Both source vehicle identities are created
by the original factory before reserving the separate finish slot. The raw ground
fixture also exports the same player role used by its ground-response branch.
Full Session cases select their actual attitude numerical policy; standalone
physical cases retain theirs. The tenth update covers the periodic normalization
and optional orthogonal restoration. Source method bodies are preserved in the
audited temporary reference overlay. UFO-carrier delivery joins the same ordered
world pass through `--scope ufos`. The remaining mixed actor/finish/startup paths
retain separate integration boundaries.

## Continuous mines

The same lifecycle pass dispatches mine method 120. Its placement descriptor
borrows the current terrain owner, resource table and both physical-position
views. A repeating drop resolves the actual owner through the canonical identity
table and calls the existing factory at that owner's current position. Parent,
children and ordinary scenery share the saved successor, RNG, grid and ordered
lists. Child birth follows that successor rather than an extra frame-delay flag.

The ordinary collision query selects mine method 122 after its normal reflection
and weight response. `weapon_mines` binds the current car's velocity, angular
rates, external basis, blast protection, surface effect and collision-size field.
The complete shared explosion is created before the point impulse. Electric
contact updates the same ground-effect owner and shield tail used by sampling;
tails use their existing 45-integer bank stride. Peer views follow racer-slot
order. The camera impulse, Deactivate, Stop/drain and component release retain
their original order and failure boundaries. No effect or shield state is copied
back after the call.

`--scope mines` compares ordinary and electric repeated drops for eighteen
updates, including both children and a final explicit Deactivate. Two ten-update
driving trajectories place a nonrepeating peer mine before their first world
pass and reach it through the real ordinary query. Their snapshots include
arming, explosion, impulse, electric state, shared resources and retirement.
The independent four-word world-event input extends the existing create/release
fixture: kind 3 places a peer mine using its current position, and kind 4 invokes
the selected entity's Deactivate interface. Neither event supplies collision or
simulation answers. The initial component image and mine order are empty; later
records are checked against the original factory and class methods.
