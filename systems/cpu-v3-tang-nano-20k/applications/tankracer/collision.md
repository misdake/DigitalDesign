# Collision and body arithmetic

`rcc/collision.rs` implements reflection, asymmetric pair impulse exchange,
direction selection and trajectory approach. Its normalize, unsigned distance
metric, matrix transforms and yaw rotations preserve the reference operations.
These are arithmetic kernels; scene candidate selection and ordered world
effects belong to their callers.

Distance uses **unsigned** absolute magnitudes and logical quarter shifts;
normalization uses **signed** wrapped magnitudes and arithmetic shifts. They
intentionally differ at extreme values. Normalization retains the small-length
wrapped multiply/divide path and the large-length divide path at 262144.
Matrix dot products wrap before shifting, while reflection truncates individual
products before summing. Vertical velocity is preserved by wall response but a
tilted pair basis can create a vertical exchange impulse.

Pair exchange preserves its asymmetric strong branch: the other car receives
the original local X/Z impulse, while the caller receives the transformed,
possibly capped impulse. The capped branch damps the other X twice. The source
normal and velocity snapshots remain distinct from mutable pair outputs.

## Exact 1.7 conversion

The original binary64 constant is `17/10 - 1/(5*2^52)`, multiplied in x87
extended precision and truncated toward zero. For integer magnitude `m <= 2^31`,
the displacement from `17*m/10` is below `10^-7`. With a 64-bit significand,
extended rounding is bounded by `1.7*m*2^-64`, less than `8.5/4096` of the
downward displacement `m/(5*2^52)`. Thus rounding cannot cross the rational
integer for a nonzero multiple of ten.
The non-integral rational values are at least 0.1 from the next integer, so
their truncation is unchanged. Positive nonzero multiples of ten lie just below
the rational integer and must subtract one. Apply the sign afterwards.

Write `m = 10*q + r`, with `0 <= r < 10`. Then
`floor(17*m/10) = 17*q + floor(17*r/10)`. The target computes this decomposition,
subtracts one when `m != 0 && r == 0`, applies the sign and keeps the low 32 bits.
`INT_MIN` uses `q=214748364, r=8`. Intermediate low-word multiplication is valid
because all subsequent additions and final narrowing occur modulo `2^32`.
This reproduces the integer reference over the full i32 domain, with zero
arithmetic approximation error and without a target i64 operation.

## Body kernels

`rcc/body.rs` owns recoil/pitch, the two player lean springs and contact-hull
refresh. Articulation skips every state write when no gun mesh is bound. It
commits spring state before sequential vertex validation/writes; repeated vertex
indices retain the last write. Lean clamps the old position before integration,
so the published position can exceed the clamp. The hull orders contacts 1/3/2/0,
retains signed vertex narrowing and wraps its u16 radius addition.

Hull displacement simplifies exactly in the wrapping ring:
`(sample + (current - previous)) - current = sample - previous`.
Gap subtraction and sign/magnitude shift follow this cancellation. This is an
algebraic identity, including signed overflow, rather than an approximation.

## Ordered object queries

`rcc/object_query.rs` provides the original segment/box test, clamped grid-cell
selection, ordered obstacle/deformable candidates, strict contact-corner overlap
and nine-slot tank queries. Collider records contain entity, position, pose and
optional external bases, bounds and radius. Tank records add slot/activation,
readiness, overlap bounds and four world contacts. All records are caller-owned;
querying allocates only bounded local scratch.

`obstacle_candidate` exposes one narrow phase with a caller-prepared delta and
length, so the world can stream a collider from immutable assets. `query_list`
uses the same function over contiguous records. List counts and row indices use
u16; missing object indices return status 4 before reading that row.

Segment codes are miss 0, crossing 1, inside 2, arithmetic error 3. A miss can
publish its projected point; only crossing writes a normal. Inside is inclusive,
whereas contact-corner overlap uses strict X/Z bounds and ignores Y. Candidate
plane division checks `INT_MIN/-1` before division, preserving the error prefix.

Scene queries choose the first accepted candidate, not the closest. Obstacle
inside tests may backtrack along the segment; deformable hits retain local point
and normal coordinates. The obstacle world-normal projection must face the
incoming segment. Scene misses preserve the incoming hit record.

Tank queries use slot order, gate on self activation and peer readiness/disabled
state, and deliberately ignore peer activation. Inside/overlap changes metadata
only. A tank miss clears metadata while preserving the incoming point/normal.
The collision dispatcher remains responsible for synchronous responses/effects.

## Deformable surfaces

`rcc/deformation.rs` samples and compresses caller-owned live mesh vertices and
records. Sampling transforms the world point, applies the original sequential
bounds clamps and strict vertex window, then combines maximum height and edge
weight. An attached record activates before the next contact; two consecutive
samples observe its updated state. Detached records retain their contents.

Hit processing publishes dirty/last-vehicle state first. Each compressed height
narrows to i16 before the floor test; flag validation and lateral drift follow
that write. Sound cooldown and the two shared LCG advances retain source order,
including silent external playback. `rcc/random.rs` supplies the shared LCG.

The contact sampler accepts an optional live surface. Surface writes survive a
later terrain-query failure even though the contact adapter restores its own
sampling, response and RNG transaction. Mutable buffers must remain valid across
terrain and trig asset-window changes; callers own their storage and selection.

`rcc/deform_contact.rs` connects the actual car segment query, class slot 21,
compression and ordered post-method sparks. `tools/car_deform_oracle.rs` calls
the original private car entry using two real ENGLAND mesh types and the actual
immutable class table. Branch fixtures cover player/mode/speed gates, unsigned
cooldown, invalid impact ring/corner, missing vertex flags, a miss clearing the
prior attachment, and a sound event consuming shared RNG. The target fixture
uses one candidate and the full class table; persistent driving separately
checks the complete seven-object deformable list. Method failures retain the
slot and committed mesh prefix, and invalid spark resources retain status -4.

## Scenery contact response

`scenery_contact::respond` is the ordinary car entry: build the physical-position
segment, query current memberships, reflect/blend velocity and update the live
impact accumulator/collision size before resolving class slot 24. It dispatches
plain Fall and Bounce methods against authoritative component buffers and the
shared tick flags/order, slot and seed. Null methods retain their normal success
behavior. The caller supplies a fresh hit record; its metadata and failure
prefix remain available after the call.

Component bindings use eight u16 words per object: kind (1 plain Fall, 2 Bounce),
state/config word offsets, alias/mesh flags, current cell X/Z/present, reserved.
Fall state/config use five/four i32 values. Bounce uses eleven state values and
eleven configuration values: the ten original gains and its independent
settling threshold. The record, state and configuration views address stable
CPU-owned storage across nested asset-window reads. Work limits and trig views
are explicit parts of the component context.

`scenery_contact::tick` resolves slot 12, then runs the corresponding live
Fall/Bounce update with the current terrain, peer basis and shared runtime.
Bounce publishes its updated cell even on a later error. `drain` consumes the
original LIFO queues and resolves Stop slot 5 using the current entity-class
table. Successful in-range calls restore the saved slot; errors and out-of-range
calls retain their source prefix. A destruction event reaches its destructor
boundary after Stop; further destruction and additional gameplay bindings
currently return -5. The complete world owner supplies those remaining methods.

Ordinary-entry qualification calls the original private car entry and real scene
dispatch, using original ENGLAND records, class tables and terrain. It covers
high/low speed, reverse motion, mode 4, active objects, missing tick storage and
zero work limits, then several actual ticks and Stop processing. Each stage
compares matrices, live properties, component state, cells, queues, tick order,
RNG and car counters; the final comparison includes every tick flag. The module
fixture isolates the selected candidate and memberships. The normal scene
application also exercises the common entry on its complete mutable world.

`scene_contact::obstacles` connects the ordered live-world query to this response.
It reads the hit object's current property row and collider radius, then returns
miss 0, synchronous method boundary 1, threshold-only reflection 2, arithmetic
error 3 or invalid object 4. The owner calls the selected gameplay method at
status 1 before proceeding to the tank/deformable/ground phases. This function
does not execute class methods or change the shared slot/RNG. Six-call reference
sequences use an explicit qualification method probe to change activity, flags,
weight and threshold, then change class and move the collider; later queries and
responses observe those writes without restoring initial asset values.

`rcc/scene_contact.rs` is the ordinary-call boundary between the live scene
query and its selected gameplay method. The caller supplies the current object
properties, hit normal, vehicle velocity and two collision counters. Reflection
uses the original 1/2 local-Z scale and 1024/1024 final scale. It publishes the
reflected velocity before checking the object's threshold. Below that threshold,
status 0 returns immediately with the reflection retained.

Otherwise the response blends each component with the previous velocity using
the object's weight over 128, preserving wrapping products and truncation toward
zero. The car runtime's impact rule then updates the accumulator and collision
size from the incoming speed, object class/activity/flags and radius. Status 1
selects the synchronous method call; status 2 is an arithmetic error. An error
or a threshold-only return does not reach that method. This phase consumes no RNG.

The response is qualified against unmodified `obstacle_collision::obstacles`
with its matching player-class path. A method probe records only whether the
dispatch boundary is reached. Three-call sequences cover signed weights,
threshold edges, breakable state gates, mode changes and counter overflow;
gameplay methods are qualified separately.

## Damage and synchronous effects

`rcc/damage.rs` keeps four body and four turret one-shot flags and two saturated
track-wear counters. Wear/flags commit before mesh validation. A dent modifies
its selected vertex and then repairs incident face normals in face order using
the signed normalization and wrapped Q12 cross product. Other faces retain
their normals. A later invalid face preserves the earlier vertex/face writes.

`rcc/impact.rs` owns four ten-particle batches. Emit releases the reused batch,
resolves direction/offset, consumes two LCG draws per particle and allocates
actual line records. Pool exhaustion retains the batch and all twenty draws.
Animation increments cooldown, honors the original same-frame early return,
updates narrowed distances/lines, then releases batches older than 60 ms.
The context's seed and the vehicle's unsigned cooldown are direct caller-owned
two-word views. Terrain sampling, scene hits and sparks can share one world seed
without copying it through an effect descriptor. Each random draw and cooldown
write publishes immediately, including on a later failure.
`rcc/effect_pool.rs` owns the line allocator with an explicit shared exhausted
tail view: handle -1 aliases the last quad's tail; handle 100 addresses the
overflow record. Count/marker desynchronization and invalid-handle prefixes
follow the source. The caller owns the backing line/tail buffers.

`rcc/ground_response.rs` runs contacts in order with those synchronous services.
Kind 4 emits its impact before reflection; kinds 4/5 reflect and update yaw;
kind 3 negates X/Z once. Damage follows the last recorded corner, and ground
delta publishes only after success. Errors preserve all earlier pool, RNG,
mesh, normal and velocity writes. Integer-only arithmetic introduces no new
approximation.

## Tank response

`rcc/tank_response.rs` operates on parallel geometry/state views and nine ordered
slots. It queries the sweep from `start` to `2*start-previous`, preserves the
mode 4/5 peer snapshot before alias validation, then resolves strict contact
overlap. Source cardinal vectors have magnitude 1024. Low-speed overlap performs
a separation before testing approach/speed; the zero-config limit is 65536
iterations. Higher-speed overlap selects lateral separation or pair exchange.
A swept hit without contact overlap can still exchange impulses.

Impacts precede velocity changes; track wear/dents follow the selected facing
codes in self/peer order. Only a successful response advances the caller's
collision timer by `4*dt`. State and effects written before failure remain live.
Statuses are miss 0, resolved 1, arithmetic 2, pool 3, resource/alias 4, damage 5
and separation limit 6. The adapter publishes the peer and successful hit as
separate values. Cases exercise both callers and alternating two-call sequences,
including multi-iteration separation and a limit error after committed effects.

`tank_response::cars` is the live car entry. It checks the caller's presence,
disabled state and visibility before traversing the nine racer slots. A slot
alias is rejected before querying candidate geometry, and a missing live peer
fails at that slot. Peer visibility does not gate its collider; readiness and
disabled state retain their original candidate gates. The entry uses the
original 65,536-iteration separation limit. It returns the source status: success
0, resource/damage failure -1, arithmetic -2, work limit -3 or impact binding -4.
It does not publish the generic mode-4/5 peer snapshot.

The vehicle timer and successful-stage velocity delta use caller-owned views.
The timer advances after resolving the selected pair. The delta publishes only
after the complete entry succeeds, including hidden/disabled/miss returns; any
error keeps the incoming delta while retaining earlier effects and velocities.
Both entries share the same overlap, separation, impulse, impact and damage
kernels. They remain distinct call contracts for the world owner.

## Blast impulse

`rcc/blast.rs` applies the original distance bands, force, optional health
damage, normalized impulse and angular response. State, velocity, angular rates
and the shared seed use caller-owned views. The returned status selects either
an out-of-range return (0), subsequent device feedback (1), or arithmetic
failure (2). Protection time is retained for the caller's hit eligibility test.

Health and its timer commit before color division. A zero denominator or
`INT_MIN/-1` preserves that prefix and consumes no subsequent random values.
An accepted hit consumes twelve LCG advances, including the discarded first
signed random value and independent sign/product draws for pitch and roll.
Zero force still consumes these draws. Visibility gates pitch/roll additions;
hit-axis and player-marker publication retain their separate input gates.
All shifts, wrapping products, narrow colors and integer division remain exact.

Persistent three-hit fixtures compare the unmodified `blast::apply`, including
distance-band boundaries, out-of-range returns, zero force, health errors,
angular overflow and all combinations of damage, axes, marking and visibility.
This qualifies the impulse kernel; projectile traversal, blast eligibility and
world dispatch are separate call sites.

## Mutable mesh fragments

`rcc/mesh_break.rs` activates and advances auxiliary triangle fragments directly
in caller-owned geometry. State has four i32 header values (retired count,
ground height, prepared and lifetime), then sixteen words per face: three i32
velocity values, three i32 angular values, i32 life, i16 extra vertex and u16
mode. Mesh views address strided vertices, triangle indices/flags, three center
columns and the radius. The loader reserves these spans and keeps word offsets
within the CPU's signed pointer range.

Activation transforms the captured incoming velocity, applies the configured
push, then initializes every face in source order. Scatter deliberately draws
twice: the first wrapped product selects the sign branch and the second product
is shifted. Angular scatter takes two RNG draws for each sample. Pair detection
preserves the carried extra-vertex tail and its missing-tail error. Geometry
flags, random consumption and completed faces remain committed on work failure.

Updates apply gravity once per update, then the source friction/bounce policy
across all six velocity/angular words. Each visible face moves its existing
vertices and center directly; a paired head also moves the tail's extra vertex.
Pairs spend one loop step and retire two faces. Missing geometry or center
columns fail at the original access, retaining earlier writes. Return values
are active 0, complete 1 and original negative errors. Resource allocation and
virtual completion belong to the owning scene methods.

```powershell
$reference = Read-Host 'TankRacerCUP Cargo.toml'
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/mesh_break_assets.py --reference $reference --output target/mesh-break-reference
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/mesh-break-check target/mesh-break-reference 0 mesh-break
```

The oracle calls unmodified `mesh_break::activate` and `tick`. Authored cases
obtain each auxiliary model from the actual constructed ENGLAND resource owner;
they retain its vertices, triangle indices, centers and parameters. Additional
cases cover integer limits, degenerate pairs, missing columns, invalid geometry,
partial activation/retirement and repeated updates. The RCC instruction checker
compares every mutable face, vertex, center, triangle flag, header, radius and
seed with a bounded run per case. This is module qualification; loading and
scene/factory calls have their own integration boundary.

`bench-tankracer-fragments` runs a bounded representative set through the normal
CB06 application path. Inputs include authored bench/barrier/gate/pickup geometry
and committed work, pairing, center and retirement failures. Its generated
buffers reserve the actual input, output, mutable geometry and shared scene sizes.
It also retains a live auxiliary owner across repeated class calls, including
completion dispatch, resource-version wrapping and errors after earlier writes.
The module checker and normal owner share qualification decoders/captures;
production fragment kernels remain independent of that format. Trig lookups use
the ordinary immutable asset window with raw i16 sign extension, and every case
checks restoration of the incoming DSEG3 mapping.

```powershell
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build bench-tankracer-fragments --reference $reference --out target/fragments-app
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/fragments_check.py target/fragments-app target/fragments-check --runtime target/release/cpu-v3-display-sim.exe
```

Add `--backend native` to the checker for bounded native CPU/cache execution
with a functional GPU. This qualification includes normal asset loading and
verification, with no GPU submissions or rendering contention. Auxiliary update
bindings and completion use the production scene entry. Stable resource
creation/copy/release and the auxiliary-contact method run against complete
authored geometry and real terrain; see [mutable mesh ownership](scene.md#mutable-mesh-ownership)
and [auxiliary activation](scene.md#auxiliary-mesh-activation). Outer car collision
dispatch and full-world scheduling are separate call sites.

## Closing barrier arithmetic

`rcc/inverse_basis.rs` implements the original inverse Q12 basis lookup. Its
output is four i32 values: pitch, yaw, roll and a nonzero-cosine flag. Failure
leaves the output unchanged. The lookup retains the explicit zero at magnitude
4096, pitch singularities, wrapping products, signed division and quadrant
selection. Its 4096-entry inverse-cosine table comes directly from the original
Rust data file; the sine table uses the common exact exporter.

`rcc/closer.rs` borrows three i32 state fields (speed, flags and phase), the live
matrix and shared scene queues. `poll` checks the original eight-frame identity
gate and strict X/Z proximity bounds before selecting the closing phase.
`advance` derives roll from the complete inverse-basis calculation, applies the
original acceleration and rebound branches, and queues Stop at the same point.
Queue failure retains preceding state changes and skips rotation. Normalization
uses a temporary matrix, preserving the committed rotation on arithmetic error.
Work limits use unsigned comparison; time, speed and wrapped matrix arithmetic
retain signed i32 semantics. Neither kernel consumes RNG.

These are arithmetic and queue contracts. The scene owner supplies the actual
phase dispatch, external/reference alias publication and fragment methods.

```powershell
$reference = Read-Host 'TankRacerCUP Cargo.toml'
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/closer_assets.py --reference $reference --output target/closer-reference
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/closer-check target/closer-reference 0 closer
```

The source oracle calls original `Matrix::angles`, `closer::poll` and the private
closing movement through an appended visibility bridge. Cases cover every Q12
pitch lookup input, singularities, integer limits, proximity boundaries, queue
failures and snapshots from repeated movement. The checker compiles the actual
RCC modules, checks transactional outputs and full queue/flag prefixes, and
places an explicit instruction bound on every case. This establishes module
correctness, separate from normal loading and world scheduling.

## Qualification

```powershell
$reference = Read-Host 'TankRacerCUP Cargo.toml'
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/collision_assets.py --reference $reference --output target/collision-reference
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/collision-check target/collision-reference 1 collision
```

The host oracle calls the unmodified Rust collision, vector and body functions.
The checker compiles the production RCC modules and runs each case with explicit
trig-table and input memory. It compares all results and error prefixes. Cases
cover both reflection scales, player/mode damping, tilted pair bases, the strong
cap, full-width integers, normalization thresholds, absent/invalid/repeated gun
vertices, lean overshoot, hull wrapping, transformed queries, candidate/slot
ordering, partial hit writes, persistent deformation samples, mesh compression,
missing flags, sound/RNG, damage, full impact-ring/pool state and ordered ground
response with the real synchronous effects. This boundary establishes module
arithmetic, independently of loading, scene effects and complete-frame timing.

Add `--car-runtime` to the fixture exporter to include the actual private
`Execution::tanks` entry. The exporter makes a temporary copy of original game
source/data, reads back the exact bytes and records their hashes. It only appends
a visibility bridge to two module files; the original implementation and checkout
remain unchanged. The bridge constructs the real execution context and invokes
that method with device operations forbidden. Cases cover caller/peer gates,
alias-before-query errors, absent entries, swept hits, both callers and alternating
same-frame calls, shared sparks/RNG, damage and success-only delta publication.
These cases qualify the live entry's contract; full-frame world scheduling and
normal multi-car loading are separate integration boundaries.
