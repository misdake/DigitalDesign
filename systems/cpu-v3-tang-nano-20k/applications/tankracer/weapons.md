# Weapon and explosion kernels

The RCC weapon kernels use the same raw integer and Q12 matrix operations as
the vehicle update. Their inputs are live views of authoritative state, terrain,
vehicle poses and the shared random seed. Resource tuning and material IDs come
from the original race loader. The six projectile kinds retain their source
values: Shell 0, Concussion 1, Homing 2, Ufo 3, Debris 4 and Warp 5.

## Data ownership

| Record | Storage |
| --- | --- |
| Projectile | 610 u16 words: 30 extended i32 control fields, optional nine-i32 reference basis, one sprite and fifteen trail records |
| Projectile pose | Nine i32 basis entries and three i32 position entries, 48 bytes |
| Shared flight | Five i32 control fields and two sprites, 108 bytes |
| Sprite | Eight i32 fields, packed UV and RGB bytes, u16 rotation and reserved padding, 44 bytes |
| Trail | Active u16, i16 time, three i32 velocity components, i32 angular rate, sprite and u32 channel, 68 bytes |
| Mine | Nine i32 fields: owner presence/ID, kind, extended u8 count, repeat/armed flags, born/last-contact u32 frames and elapsed time, 36 bytes |
| Explosion | 676 u16 words, 1,352 bytes: header and fragment UV, main sprite, ten smoke rows and fifteen contact-fragment rows |
| Temporary effects | Available slot, timestamps, line handles and twelve geometry/color/phase rows, 548 bytes; borrows the common line pool and seed |

Optional IDs are read only when their presence flag is set. The projectile
reference basis has the same rule. Narrow fields preserve their source byte or
signed-word wrap; time, velocity and position preserve wrapped i32 arithmetic.
Views contain addresses and scalar descriptors, rather than copies of persistent
state. A scene owner supplies stable storage and retains the RNG between calls.

## Projectile sequence

`projectile::launch` commits owner and kind before pitch validation, computes
the launch velocity from actual tuning and owner motion, then publishes the pose.
Homing drift reaches shared flight state only after launch succeeds. Sprite
initialization is separate because scene activation precedes it in the factory.

`projectile_flight::advance` performs kind-specific UV/rotation work, steering,
retirement/flash handling, carrier attachment or free flight, vehicle sweeps,
terrain cadence, age update and trail emission/fade in source order. It returns
Flying, Trails, Retire, Expired or Impact. An impact carries the original origin,
optional ground mark and both local and world vehicle contact records.

The vehicle table is borrowed for the duration of a call. Sweep and target
selection visit racer slots in order. Sweeps account for target motion, preserve
inside-start backtracking and require an incoming surface direction. Shared glow
state changes once per source frame where required. Trail noise uses its own
fixed sequence; it does not consume gameplay RNG.

`projectile_ricochet` retains shield deflection, the roll-zero peer-redirection
branch and the concussion ownership exception. RNG consumption, signed distance
selection and the writes preceding missing-owner/primary errors match the source.

## Mine sequence

`mine::contact` distinguishes sustained overlap from a gap in contact using
actual frame numbers. The first gap arms the mine; a subsequent call reports an
armed hit. `mine::tick` retains the signed-byte count comparison, repeat interval,
elapsed reset and expiration threshold.

`mine::align` queries the real terrain normal, computes both local angles before
changing the matrix and applies the X/Z rotations. Scoped terrain access restores
the incoming DSEG3 mapping.

`mine::point_hit` writes angular impulse and velocity damping before requiring
the external basis. On success it applies lateral impulse, clamps negative health
and publishes collision size. The CUP branch has no health subtraction here.

## Vehicle hit response

`tank_hit::coefficients` selects the actual Shell/Homer player or peer gains.
Multipliers and shifts retain their complete unsigned resource words; arithmetic
uses the source's low shift byte and wrapped products. `tank_hit::contact` updates
the cooldown, normal-derived tilt, yaw, slowdown and local lateral impulse in
order. It then advances saturated face-hit counters and the damage smoke socket.
Repeated player hits call the existing damage kernel on the same private body or
turret vertices and repair incident normals. A successful dent advances the
part's four-word u64 presentation revision. Missing external bases and missing or
invalid private meshes retain their earlier state writes.

`tank_hit::apply` includes Warp sidegrip changes, its additional late peer slowdown
and shared temporary-effect activation. Prefix gains and matching drive-config
columns are caller-owned views. The timer commits after effect activation succeeds.
The scene owner supplies the existing AI notification boundary separately.

`temporary` directly owns four effect slots and twelve line geometry rows. It
preserves unsigned timestamp selection, oldest-slot reuse, active-timer refresh,
the different start/tick RNG order, line allocation aliases and failures partway
through row updates. Its core layout matches the existing vehicle prefix's
temporary expiry view. Reset clears timestamps while retaining geometry/handles.

## Explosion sequence

`explosion::initialize` receives the factory's first random draw and consumes the
next draw for angular velocity. It initializes the main sprite without clearing
retained smoke or fragment storage. `finish_spawn` publishes visibility and empty
shared band/ring references after the owner's slot and retirement work succeeds.

`contact` commits the owner, X velocity and normalized local attachment before
requiring the body radius. Its fragment selection preserves projectile kind,
spin gating and all random draws, including the two draws preceding the spin
test and the final pair after the last fragment. Fragment rotations are retained.

Each frame runs `begin_tick`, the scene's shared band/ring updates, then
`finish_tick`. The first stage advances contact fragments, animation and motion;
its retirement result returns before shared effects or smoke consume RNG. Smoke
uses the resolved live owner pose when available and preserves the original
fallback origin and retained lift. Narrow colors, sizes and rotations wrap at
their source widths. The ordinary `tick` entry retains the source's missing-blast
error when shared references require a scene owner.

The `explosion` checker compares initialization, contact and short animation
sequences against the authored vehicle and resource tables. It checks complete
smoke/fragment state and RNG, signed time and packed damping edges, partial
contact errors and retirement before a missing shared-blast reference. The test
stack remains below the DSEG windows. Shared blast ownership and whole-scene
scheduling have their own integration boundaries.

## Shared blast slots

`explosion_blasts` owns a 748-byte state: signed timestamps, next-slot and count
fields, two bands and two twelve-ray rings. It directly borrows the common quad
and line pools. Allocation selects the original oldest nonzero timestamp, retires
the previous owner's handles and publishes the new slot in source order. The
returned owner identifies the explosion whose band or ring reference must be
cleared by the scene. A later failure preserves earlier releases and count writes.

Band construction clears its row; ring construction retains alpha, rotation and
unvisited rays until each ray is written. Quad exhaustion releases only the
handles allocated earlier in that attempt, retaining the source's slot metadata.
The existing pool implements handles -1 and 160 and the last-record aliases.
Ring initialization and ticking keep their distinct RNG sequences. Expiration
releases before consuming more random values.

`project` writes up to 48 derived raw quads to caller scratch, with additive style
and source UVs. It retains Q12 truncation before the camera transform. Projection
leaves the seed, slot state and allocated initial quads unchanged. Camera-facing
geometry can therefore be prepared independently of simulation. Its maximum
output is 3,072 bytes; the common effect pools remain shared with other effects.

The `explosion-blasts` qualifier compares all slots, pool records and aliases,
returned eviction identities, RNG and derived geometry after each operation.
It includes zero/negative/tied timestamps, repeated release, cross-kind eviction
before a later failure, partial allocation and extreme wrapped geometry.

## Qualification boundary

The exporter builds a temporary byte-audited copy of the reference game with a
private target-selection probe. `Prepared::load_race(..., true).finish()` supplies
the weapon resources and authored vehicle model. The reference checkout is read
only. Source and resource hashes accompany the snapshots.

The instruction checks compare state after every original method call, including
error prefixes, shared seed, sprites, trails, poses, contact actions and affected
vehicle state. Cases cover live seeking/contact, repeated frame numbers, missing
references, trail exhaustion, owner transfer, arming gaps and real map alignment.
The fault check deliberately changes individual expected words and verifies that
the runner rejects RNG, pose, trail and partial-error-state differences.

```powershell
$reference = Read-Host "TankRacerCUP Cargo.toml"
$tools = "systems/cpu-v3-tang-nano-20k/applications/tankracer/tools"
python -B "$tools/projectile_assets.py" --reference "$reference" --output target/weapon-reference
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/projectile-check target/weapon-reference 0 projectile
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/mine-check target/weapon-reference 0 mine
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/tank-hit-check target/weapon-reference 0 tank-hit
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/temporary-check target/weapon-reference 0 temporary
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/explosion-check target/weapon-reference 0 explosion
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/blasts-check target/weapon-reference 0 explosion-blasts
python -B "$tools/weapon_checker_check.py" target/release/tankracer-physics-check.exe target/weapon-reference target/weapon-fault-check
python -B "$tools/weapon_checker_check.py" target/release/tankracer-physics-check.exe target/weapon-reference target/explosion-fault-check --explosion
python -B "$tools/weapon_checker_check.py" target/release/tankracer-physics-check.exe target/weapon-reference target/blasts-fault-check --explosion-blasts
```

## Transient scene storage

`scene_world::World` retains fixed collider/property arrays and declares their
reserved row count. Appended global object IDs address a separate 32-KiB bank.
Each appended row contains the complete 60-word collider and 16-word properties;
215 rows occupy 32,680 bytes. Scoped reads/writes restore DSEG3. The fixed query
loop keeps its direct contiguous reads. A banked candidate is copied into the
same single-collider query scratch before geometry tests, retaining source order.
Ordinary response reads the selected bank's current weight, threshold, flags,
class and radius.

`scene_lifecycle::allocate_transient` uses the shared entity record and free-object
stack. It pops a reusable global ID before checking matching owner lengths, or
appends a row within the declared backing capacity. It publishes the zero pose,
class properties, live transient/cell metadata, obstacle handle and resized tick
flags in source order. A reused ID may address either fixed storage or the appended
bank. The existing release method resolves collider identity in the same layout,
then retains/removes memberships and returns the slot to the LIFO free stack.
Backing tick flags reserve two words per identity storage slot; metadata reserves
eight words per object capacity. Entity growth retains its 128-slot source policy.

The allocation qualifier starts from race-loaded worlds and compares every
collider, property row, identity, flag, membership list, cursor and RNG after
append/release/reuse. It includes growth with automatic and requested IDs, reused
rows inside the fixed prefix and failure after a free-list pop. Optional identity
payload and absent object-cell payload are captured by presence, as in the
reference's typed state. Banked query checks also poison obsolete fixed locations,
use a separate page for the live candidate grid and guard the final complete row.

```powershell
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/allocation-check target/weapon-reference 0 scene-allocation
python -B "$tools/scene_allocation_checker_check.py" target/release/tankracer-physics-check.exe target/weapon-reference target/allocation-fault-check
```

## Weapon construction and launch

`scene_creation` executes the actual class allocate/initialize slots against the
shared identity, collision and lifetime owners. Projectile, mine and explosion components
use a two-word-per-object directory and caller-reserved external banks. A projectile
bank holds 25 1,280-byte records; a mine bank holds 341 96-byte records; an explosion
bank holds 23 1,412-byte records. Each record
has a 30-word scene header followed by its existing state payload. Header words
0..4 hold object ID plus one, signed sort position and an optional mesh handle;
word 5 is the u16 visibility distance and the remaining words are reserved.
Additional banks extend capacity without
changing global object IDs. Allocation clears the selected record; release returns
that record to the same pool after retiring the shared identity and scene row.

Initialization follows World -> Generic -> Obstacle class methods: the basis and
external basis become identity, Generic sets sort position to -8, and Obstacle
clears the initial collision cell. Successful nested calls restore the shared
dispatch slot; failed constructors retain their identity and prior writes.

`spawn_projectile` connects creation, launch, external-basis publication, activation,
ordinary-list removal, special-list insertion, sprite initialization and Homing
mesh/glow setup in source order. During launch, a fixed collider stays in DSEG2
while its component occupies DSEG3. For appended colliders, their bank occupies
DSEG3 and the component uses DSEG2. The caller restores both mappings before
lifecycle calls. Pose and state are direct views; terrain/trig reads restore their
borrowed window. Projectile deactivation queues the actual Generic parent method.

The shared allocation checker also runs `scene-creation` using original race-loaded
constructors and factory calls. It checks complete world/identity/list state,
component payloads, scene headers, shared flight and RNG after every call. Cases
include thirty concurrent launches across banks, reuse in the fixed prefix,
all six projectile kinds, CUP tuning, missing resources/classes, arithmetic
failure before launch completes and ordinary-list failure after activation.
Null allocate-slot fixtures also retain the source's allocated identity/class
and subsequent initialization failure, with the active interface preserved.

## Player fire

`scene_fire::apply` connects inventory fire to the shared projectile/mine
factories and immediate target selection. Its `Car` contains views of the live
inventory, contact-response cooldown, collision feedback, articulation rates,
recoil mode, boost control and spin state. `Fire` borrows the existing launch
and mine-placement descriptors, shared boost feedback and spin count/pulse,
racer geometry and the output shot. Poses, velocity and physical position remain
in the car owner. Audio is an external successful operation after the cooldown
gate and collision-feedback writes.

A cooldown of at least one returns before gameplay writes. An eligible player
adds 75 to impact accumulation and 50 to the unsigned collision size. Inventory
0/9 fires Shell, 3 Homing, 4 UFO, 5 Concussion and 10 Warp. Inventory 1/2 places
the corresponding repeating mine. The factory runs before ammunition changes.
Successful projectiles publish recoil mode, subtract 600/900 from recoil/pitch
rates and set cooldown 1000. Successful mines set cooldown 300. Ammunition wraps
as u8; reaching zero selects the default weapon. Default shells retain ammunition.

Inventory 7 starts the boost camera/application sequence and publishes feedback
`[-16, 1300, 110]`. The following prepare phase applies the configured physical
gain through the real boost method. Inventory 8 calls the existing spin start
with the same mode, shared count/pulse and RNG. Both consume one ammunition byte
and set cooldown 300. Other inventory values set cooldown 300 immediately.

After successful projectile application, target selection resolves its live
component and calls the existing Homing/Warp selection kernel with the current
vehicle entries and racer order. A selection failure preserves the earlier
factory, ammunition, recoil and cooldown writes. The caller's shot output
commits only after this selection succeeds. Factory errors retain collision
feedback and the factory's original partial state. This entry covers the fire
operation and immediate non-AI selection; the frame owner controls its input,
prepare and scheduler boundaries.

The `scene-fire` qualifier calls the original private player fire operation and
its actual post-fire selection on FullRace resources. It compares complete
scene/pool/flight state, inventory and narrow-field wrap, boost/spin/shared RNG,
recoil, collision feedback, private output and live racer inputs. Cases include
all inventory types, cooldown gates, real terrain mines, class/resource failures,
late selection failure, target order, unsigned collision wrap and repeated calls.

```powershell
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/fire-check target/weapon-reference 0 scene-fire
python -B "$tools/scene_allocation_checker_check.py" target/release/tankracer-physics-check.exe target/weapon-reference target/fire-fault-check --fire
```

## Explosion construction and resource retirement

`scene_explosions::spawn` uses the actual explosion class and shared constructor.
The first random draw follows construction and precedes activation. It then
appends the special drawing membership, publishes the collision origin and
initializes the banked animation. A 200-byte world record owns the fifteen
presence/entity/frame slots, signed count, overflow, fog and flash fields.
At capacity the smallest unsigned birth frame selects the replacement; removal
and deactivation complete before its slot is replaced. Final visibility and empty
blast references are published last. Failure retains the committed prefix.

Common Detach and ReleaseId dispatch first calls `scene_creation::release_explosion`.
It releases band then ring, clears each successful reference and finds the world
slot by the collider's entity. An evicted object's deferred release therefore
leaves its replacement's count intact. A later failure retains earlier released
handles and leaves the identity and component live. Successful common retirement
then frees the component slot through the same directory used by other weapons.

The complete quad/line records and shared blast state occupy 14,300 bytes in one
effect stage bank. Pool descriptors and the small explosion-world record remain
in CPU memory. Resource release maps this bank in DSEG2 and the explosion record
in DSEG3, restoring both mappings on every return path. Collider identity is read
before entering that stage. Callers provide live typed views of the shared pool
descriptors, so count changes remain visible to the following update.

The `scene-explosions` qualifier covers construction, sustained capacity pressure,
birth-frame wrap, fixed-row reuse, shared-resource release and committed failures.
Complete scene rows, identities, lists, component state, effect pools, RNG and
window restoration are compared after each original call.

## Weapon explosion factory

`scene_explosions::spawn_weapon` completes the ordinary explosion factory first,
then creates the shared effect for its projectile kind:

| Kind | Shared effect | Main sprite |
| --- | --- | --- |
| Shell, Homing, Ufo | Ordinary explosion | Visible |
| Concussion | Flipped band, outer radius 20,480 | Visible |
| Debris | Band, outer radius 15,360 | Visible |
| Warp | Ring kind 2 | Hidden after successful effect publication |

The start frame is the unsigned explosion-world birth frame; the separate signed
stamp controls shared-slot selection. Both retain their original zero and wrap
semantics. Returned eviction owners are cleared before the new band/ring reference
is published. Pool exhaustion can return an empty handle with committed slot
metadata, which the factory preserves together with its visibility changes.

Optional contact selects the live vehicle only after shared-effect creation.
`HitVehicle` borrows velocity and spin; body presence and its immutable source
radius accompany those views. The contact kernel writes the owner and normalized
attachment before requiring that body resource. Existing objects and effects
remain live on a later failure. Concussion commits flash time 50, activates the
flash and clears overflow only after contact succeeds; fog retains the factory's
capacity result.

The `scene-weapon-explosion` qualifier checks all six kinds, contact and spin,
missing vehicles/body/materials/pool, shared-slot eviction and partial allocation,
exhaustion aliases, independent clocks, capacity pressure and contact followed by
complete explosion ticks. Vehicle, camera, material and contact inputs are checked
as read-only alongside complete scene, component, RNG and effect-pool state.

## Explosion Tick and natural retirement

`scene_explosions::dispatch` resolves the actual class slot and executes method 93.
The update requires simulation execution, then finds an attached owner by entity
in live racer order before changing animation or RNG. It borrows the vehicle's
current base position and reference basis. The lookup remains valid when the
vehicle entry exists independently of its identity-table attachment.

The tick reads only the collider's twelve-byte origin into CPU scratch. It keeps
the explosion record in DSEG3 while fragments, main animation and motion advance.
Band then ring updates borrow the shared effect bank in DSEG2. Each successful
release clears the current reference immediately; two small owner records wait
until smoke has completed. Only then does the scene clear those owners' matching
references in order. A failure preserves all earlier writes, including a band
release before a failed ring update or smoke before an invalid owner is found.
The sine lookup restores the component mapping after each borrowed access.

Early animation retirement precedes shared-blast updates and smoke RNG. The scene
removes special drawing membership, calls Deactivate, then releases its remaining
resources and world slot. Deactivate queues destruction; Stop/drain later removes
the scheduled tick and releases the component. The current tick cursor therefore
remains live through this method. All paths restore the incoming DSEG2/3 mappings.

The `scene-explosion-tick` qualifier compares short sequences with live owner
views, missing execution/pool/owner boundaries, wrapped time, partial release,
natural retirement and a newly spawned object in the appended collider bank.
It also checks that the vehicle inputs remain unchanged. Declared null methods
restore the previous dispatch slot; an upper-slot no-method result retains the
requested slot, matching the source interface boundary.

```powershell
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/explosion-factory-check target/weapon-reference 0 scene-explosions
python -B "$tools/scene_allocation_checker_check.py" target/release/tankracer-physics-check.exe target/weapon-reference target/explosion-factory-fault-check --explosions
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/explosion-tick-check target/weapon-reference 0 scene-explosion-tick
python -B "$tools/scene_allocation_checker_check.py" target/release/tankracer-physics-check.exe target/weapon-reference target/explosion-tick-fault-check --explosion-tick
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/weapon-explosion-check target/weapon-reference 0 scene-weapon-explosion
python -B "$tools/scene_allocation_checker_check.py" target/release/tankracer-physics-check.exe target/weapon-reference target/weapon-explosion-fault-check --weapon-explosion
```
Pool tails and incoming DSEG mappings are checked on every call.

```powershell
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/creation-check target/weapon-reference 0 scene-creation
python -B "$tools/scene_allocation_checker_check.py" target/release/tankracer-physics-check.exe target/weapon-reference target/creation-fault-check --creation
```

## Banked projectile flight and reflection

`scene_projectiles::advance` runs the selected flight phase on the shared
component and collision owners. Its caller supplies the already-resolved carrier,
live car query views, authored resources and existing `ground::World`. The
projectile component stays in DSEG2 while terrain borrows DSEG3. Only the twelve
pose integers use temporary storage, totaling 48 bytes; the 1,220-byte component,
shared flight state, overlay and car records retain their authoritative locations.

Position and matrix writes survive an advance failure. The external collision
basis is published only after success, matching the original caller. Returned
ground and vehicle impact packets retain their mark, local/world hit and origin.
The synchronous impact stage consumes that packet before another entity advances.
Attached-UFO lookup and the complete impact dispatcher are separate call boundaries.

`scene_projectiles::finish` handles Trails and Retire. Retire dispatches the real
Deactivate slot, removes the original object ID from the special list, then
fades trails. Nested errors keep the active slot and committed queues. The
original object ID remains valid for this ordering even when a class method has
released its identity or component; a subsequent missing component returns -4.
Flying, Expired and Impact do not perform this retirement phase.

`scene_projectiles::reflect` first checks the live car, component, spin and kind.
Homing/UFO or an inactive spin retains the impact before requiring simulation.
A real reflection calls the existing integer reflection kernel with the common
seed and racer-slot table, changes ownership and pose, and retains the external
basis published before reflection. Its return is zero for an ordinary impact,
one for a reflected shot, or the original negative error. All incoming mappings
are restored. The common non-reflection path skips the pose scratch copy.

## Terrain impact marks

`projectile_marks::State` is 108 bytes: wrapping count, head, next and twenty-four
raw quad handles. Each add first queries both diagonal corners with the full
terrain-query semantics, including exclusion flag 16 and arithmetic failures.
`ground::World::query` scopes the cell mapping around those calls. The height-only
API has a different vertical-plane rule and cannot replace these queries.

An accepted mark evicts the oldest handle only when count equals twenty-four.
The four points use the two sampled heights and their truncated average; the
shared allocator then applies the original Q3 coordinate conversion. Style uses
Material blend, RGB 128 and the original 1..63 UV rectangle. Exhaustion preserves
the queue changes already made before allocation. Raw handle aliases share the
same quad/line state as other effects.

The scene-flight qualifier compares complete identities, collision rows, component
banks, ordered lists, shared flight state, overlay, RNG and returned impact packets.
It includes cross-bank launch followed by flight, pose failure prefixes, reflection
guards, carrier transforms and class-driven retirement. The mark qualifier checks
the real ENGLAND terrain, queue wrap, geometry/style, pool pressure and aliases.
Both use bounded instruction simulation; whole-world impact scheduling remains
a separate integration boundary.

```powershell
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/projectile-scene-check target/weapon-reference 0 scene-projectile-flight
python -B "$tools/scene_allocation_checker_check.py" target/release/tankracer-physics-check.exe target/weapon-reference target/projectile-scene-fault-check --projectile-flight
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/projectile-mark-check target/weapon-reference 0 projectile-marks
python -B "$tools/weapon_checker_check.py" target/release/tankracer-physics-check.exe target/weapon-reference target/projectile-mark-fault-check --projectile-marks
```

## Area projectile update

`scene_projectile_impact::dispatch` executes the actual class interface for
Concussion and Debris projectiles. Its selected Tick connects flight, shield
reflection, marks, explosion construction, emitter transfer, flash and ordered
racer response. Trails and retirement use the same scene owners. Null and upper
interface slots retain their source restoration behavior. Its caller selects
this handler for Shell, Homing, Warp and the two area-projectile kinds.

`begin` performs the common synchronous impact prefix. Projectile resources are
required before a ground mark; terrain and the effect pool precede mark queries.
Simulation is required after the mark, retaining an accepted mark on that failure.
Explosion creation precedes emitter transfer and the projectile's retired flag.
The shared component header stores emitter presence in word 7 and its full entity
ID in words 8..9. Non-AI blast notification retains the ordered peer validation
before primary-player flash selection.

Flash uses racer slot zero's base position in XZ, with radius 163,840 for Concussion
or 81,920 for Debris. The blast then visits all nine racer slots in order, including
duplicate entries. It excludes the projectile owner and positive protection timers,
and screens base-position XZ against radius 30,720 or 20,480. The actual impulse
uses the separate physical position and the existing three-dimensional blast
kernel. Visibility publishes angular enable before that kernel's range return.
Health uses the original damage-disabled path, retaining its negative-health
clamp. Player collision-size marking and the common seed remain direct views.

Every player passing the outer filter publishes camera duration 350 or 300 with
magnitude 450 after a successful blast call, including an out-of-force-range
return. Errors retain earlier marks, explosions, flags, camera writes and random
draws. The caller-owned action and car views keep the same data throughout the
update; only the original three-integer projectile velocity argument is staged
across nested explosion construction.

The shared effect stage occupies 14,408 bytes: quad/line state, blast slots and
the 108-byte mark queue. The `scene-projectile-area` qualifier dispatches original
class-table ticks on real race resources, checks all shared scene/effect state and
car blast columns, and follows impact with parent and child updates. It covers
owner/protection/range boundaries, distinct base and physical positions, visibility,
camera, emitter propagation, resource failures and interface slots. Direct
Shell/Homing/Warp contact uses the same dispatcher and the separate qualifier below.
The UFO actor has its own connected transfer qualifier below. Mixed whole-world
native frames retain their integration boundary.

```powershell
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/projectile-area-check target/weapon-reference 0 scene-projectile-area
python -B "$tools/scene_allocation_checker_check.py" target/release/tankracer-physics-check.exe target/weapon-reference target/projectile-area-fault-check --projectile-area
```

## Direct projectile update

The same class-table dispatcher runs complete Shell, Homing and Warp ticks.
After flight and reflection, the common impact prefix constructs the explosion,
attaches it to the struck vehicle, copies its emitter and marks the projectile.
The direct phase reads the live kind, age and velocity. A positive spin skips
vehicle damage; Homing still creates its explosion because it does not reflect.
Shell and Warp use the existing shield-reflection path before construction.

`Damage` supplies current vehicle views, authored shell/homer gains, cup and the
shared temporary-effect owner. The hit packet is borrowed directly. Velocity,
angular motion, collision size, inventory, cooldown, counters, smoke, one-shot
flags and mesh revisions address their existing owners. Only twelve projectile
velocity bytes cross the component-to-geometry mapping boundary. The effect pool
and temporary state use DSEG2 while the selected private geometry uses DSEG3;
trigonometric asset access restores that geometry mapping. Both incoming pages
are restored on success and failure.

Temporary context and the effect pool are required before the contact kernel.
The existing kernel applies cooldown, impulse, count/smoke updates and player
body/turret dents, then performs Warp sidegrip and temporary-line activation.
Missing external basis, private geometry or a valid temporary slot preserves
all earlier writes. The original direct-hit path checks spin rather than the
area-blast protection timer. Player camera duration/magnitude is 100/400 for
Shell, 500/400 for Homing and 50/300 for Warp, published only after success.

The combined effect stage is 14,956 bytes, including 548 temporary-state bytes.
The selected vehicle's writable geometry occupies a separate bank. The
`scene-projectile-direct` qualifier calls original class-table methods from real
race snapshots, compares all scene/component/pool state, live car columns,
private vertices/normals/revisions and temporary rows, and follows a Warp impact
with its attached explosion and subsequent parent updates. Branches include
player/non-player hits, upper/lower dents, smoke, shield reflection, Homing spin,
inventory cooldown, temporary refresh/exhaustion, signed stamps, null/bounded
interfaces and partial failures. AI and the whole-world scheduler remain caller
integration boundaries.

```powershell
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/projectile-direct-check target/weapon-reference 0 scene-projectile-direct
python -B "$tools/scene_allocation_checker_check.py" target/release/tankracer-physics-check.exe target/weapon-reference target/projectile-direct-fault-check --projectile-direct
```

## UFO construction and retirement

`scene_ufos::spawn` resolves a live vehicle or obstacle target before checking
resources and class. It reads the current base/collider position, creates through
the ordinary class table, publishes emitter, beam mesh and visibility, then
initializes the UFO point 102,400 units above that target with wrapping arithmetic.
Vehicle views provide player, slot and collision state; obstacle membership is
read from the live special list and all current spatial cells. These flags record
the target's actual state for transfer ownership. Construction consumes no RNG.

The component directory uses kind 4. A UFO record is 172 bytes: the common
60-byte scene header, seventeen i32 state fields and the ordinary 44-byte sprite.
Each 32-KiB bank holds 190 records. Allocate method 124 shares transient identity
and collision allocation; Deactivate method 126 calls the Generic parent before
removing special membership. Successful release uses the same component-retirement
boundary as the other weapon kinds. Partial constructors retain their class slot,
identity, mesh and scene writes. Player flags and view publish after activation
and special-list insertion.

The `scene-ufos` qualifier starts from the original FullRace preparation path
with one vehicle. A temporary source bridge exposes that existing loader mode;
its original implementation bytes remain unchanged. It compares actual original
factory calls, shared constructor/deactivation/release methods, all identity,
scene, component and spatial memberships, and immutable vehicle inputs. Cases
cover both target kinds, collision/display flags, wide emitters, height wrapping,
resource/target failures, null and bounded methods, repeated creation and ID reuse.
The transfer Tick and projectile attachment use the same component directory and
have their own connected qualifiers below.

```powershell
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/ufo-factory-check target/weapon-reference 0 scene-ufos
python -B "$tools/scene_allocation_checker_check.py" target/release/tankracer-physics-check.exe target/weapon-reference target/ufo-factory-fault-check --ufos
```

## UFO projectile attachment

`scene_projectile_impact::dispatch` also runs the UFO projectile Tick. Before
terrain/resource checks, `scene_ufos::carrier` resolves an active attachment
through the shared identity, UFO component and target vehicle. The carrier views
address that vehicle's current base position and reference matrix; UFO flag 2
publishes release. Retired/trailing projectiles skip this lookup. The mutable
Tick frame contains only call controls and borrowed views; carrier resolution
changes those frame views without copying persistent vehicle state.

A first UFO contact checks the live target's protection timer. Protection below
one and an empty attachment call the same UFO factory with a zero destination
and no emitter, then publish its entity, the original local hit point and flag 4
on the projectile. This branch precedes reflection, explosion, Simulation and
effect-pool requirements. A hit already carrying flag 4 returns immediately.
Protected or previously linked contacts continue through the ordinary explosion
prefix, with the original emitter and retirement semantics. UFO does not use the
direct-hit damage kernel or shield reflection.

The `scene-projectile-ufo` qualifier uses original FullRace resources and calls
the complete original class-table Tick. It checks creation, local attachment,
a moved/rotated target, release and parent retirement, missing or wrong carrier
handles/components/targets, protection, partial construction and interface bounds.
Carrier errors retain priority over missing terrain and projectile resources.
All scene, pool, component and spatial state is compared after each operation;
car basis and base-position updates are explicit fixture inputs. Transfer actor
Tick remains a separate owner of the UFO flags and vehicle methods.

```powershell
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/projectile-ufo-check target/weapon-reference 0 scene-projectile-ufo
python -B "$tools/scene_allocation_checker_check.py" target/release/tankracer-physics-check.exe target/weapon-reference target/projectile-ufo-fault-check --projectile-ufo
```

## UFO transfer update

`scene_ufo_transfer::dispatch` runs the actual UFO Tick interface over the
existing 172-byte record. It holds the component bank in DSEG3 for the call;
nested grid, collider, class and trigonometric operations restore their incoming
mapping. Caller views remain valid while that bank is selected. The target's
initial XYZ and a MOVE payload each use twelve CPU scratch bytes. Component
state, vehicle poses, motion and lifecycle memberships remain in their owners.

`Tick.vehicles` points to one live `scene_vehicle_transfer::View`; its route
owner contains the shared occupied-checkpoint list. `Tick.visible` points to
the vehicle drawing-list owner. Per-car metadata has four i32 columns: entity,
player, body presence and unsigned-u16 body radius. The nine racer slots use
65535 for an absent racer. `shapes` contains the resource owner's current
mesh-resolution result per object, and `overlay` borrows the existing presence
and four byte columns. Counts in the list owners update in place.

The incoming flags select the phase. Approach follows target XZ, advances the
packed vertical approach and wraps the i16 timer. After 800 units, MODE(1)
precedes COLLIDABLE(0). Transfer begins after the player/non-player time threshold
and any player fade: SHOW(0), RESET and MOVE run in that order. A vehicle MOVE
restores its real checkpoint and complete poses; its returned height publishes
before checkpoint reservation. Timer/phase changes precede visibility removal.

Player transit uses the original arrival timer. Other targets move toward their
current XZ using signed downscaling, clamping and the original approximate
`length2`, with checked division and sign-crossing clamps. This approximation is
the source algorithm. Vertical approach preserves packed `0x07010003`: a signed
low-word step, low-byte damping factor and sign/magnitude shift.

An arrived target is shown before its arrival timer advances. Release requires
the timer threshold, zero fade and a clear overlap query. That query walks the
nine racer slots in order and tests wrapped absolute X/Z against eight times the
sum of the unsigned radii. It skips the target entity before checking a peer's
body. COLLIDABLE(1), RESET and MODE(0) precede clearing the transfer flags. Fade
decay follows this release test, so reaching zero permits release on the next
Tick. Departure advances speed/height, removes the reserved checkpoint and calls
the actual UFO Deactivate interface.

The final publication copies the point to the sprite. Flag 4 also rotates the
live collider around Y, publishes its position at resource-height offset and
updates its external basis. Failure keeps earlier writes and the failing slot;
success restores the caller's slot. Nested target dispatch uses the current
identity, so a successfully detached target makes the following class lookup
fail before installing another slot. These actor operations consume no RNG.

`scene-ufo-transfer` compares original FullRace actor calls with full scene,
component, car-pose/motion, route reservation, visibility, overlay, lifecycle and
RNG snapshots. It includes continuous player, non-player and obstacle transfers,
branch thresholds, large unsigned radii, overlap failures, packed arithmetic
wrap, partial restores, nested errors and departure. The actor qualifier ends at
its queued lifecycle effects; mixed scheduler/drain execution is a separate frame
boundary. Answer mutations check each affected state boundary.

```powershell
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/ufo-transfer-check target/weapon-reference 0 scene-ufo-transfer
python -B "$tools/scene_allocation_checker_check.py" target/release/tankracer-physics-check.exe target/weapon-reference target/ufo-transfer-fault-check --ufo-transfer
```

## Mine placement and memberships

`scene_mines::spawn` executes the original resource/kind/class checks, common
construction, real terrain height, identity pose, 0x9000 visibility distance and
mine owner/frame fields. Enter appends the ordinary list and inserts the current
point with the source's default radius before the mesh exists. The factory then
sets the authored mesh/radius, scales each i16 bound by eight with a 1,536-unit margin,
aligns the basis to the terrain normal, publishes the external basis and activates.
All state and pose writes address the shared banks. The terrain argument is a
typed view of the caller's existing `ground::World`.

The shared weapon owner retains the ordered mine IDs and wrapping u32 mine count.
After an append raises the count above thirty, the oldest mine's actual Deactivate
slot calls the Obstacle parent before removing its mine-order entry and decrementing
the count. Leave removes ordinary/grid membership and retains the mine-order entry.
Successful release retires the component independently of these memberships, as in
the source. Errors retain their already-published pose, flags, grid/list changes,
component data and count; the next stage sees the same prefix.

The `scene-mines` qualifier checks real ENGLAND placement, a sampled non-flat slope,
fixed-prefix reuse, repeated leave/enter, complete destruction/release, unsigned
birth frames, missing resources/classes/terrain, invalid kinds and off-map normal
misses. Two thirty-two-drop sequences cover successful oldest retirement and a
work-limit failure after the new mine has activated and incremented the count.
Every call compares the full grid, ordered lists, identity, component/scene rows,
poses, mesh handles, bounds, counters and shared seed.

```powershell
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/mine-scene-check target/weapon-reference 0 scene-mines
python -B "$tools/scene_allocation_checker_check.py" target/release/tankracer-physics-check.exe target/weapon-reference target/mine-scene-fault-check --mines
```

## Mine frame update

`scene_mines::dispatch` resolves the actual interface and executes the original
Tick method. Time/count updates commit before requiring the owner, including an
otherwise empty action. A repeat resolves that owner through the shared identity
table to a present vehicle and reads its live physical-position view. The child
uses the same complete factory with repeat disabled and the current frame stamp.
It shares identities, component banks, collision grid and mine order/count with
its parent. Expiry dispatches the actual Deactivate interface. Nested errors retain
their active slot and every committed prefix; successful and null methods restore
the prior interface, while an out-of-range interface keeps the requested slot.

Caller-owned `Tick` and `Vehicle` records contain scalar controls and views. They
copy no persistent mine or vehicle state. Component mapping ends before nested
creation/deactivation; both paths return with the caller's mappings. The source's
post-repeat fire command belongs to the external audio boundary and changes no
physical state or gameplay RNG.

The `scene-mine-tick` qualifier starts from original race-loaded checkpoints,
including populated component banks and thirty-mine pressure. The car's physical
position differs from the initial drop/base position. Original dispatch calls
cover exact interval/expiry edges, dt wrap, unsigned birth stamps, missing owners,
detached identities, absent vehicles/resources/terrain, partial child creation,
nested oldest-retirement failure, null methods and interface bounds. Every call
compares the whole live scene and shared runtime. Active-cursor fixtures also
check that queued deactivation retains the next entity across nested calls.
The outer world's subsequent Stop/drain stage owns the successor change.

```powershell
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/mine-tick-check target/weapon-reference 0 scene-mine-tick
python -B "$tools/scene_allocation_checker_check.py" target/release/tankracer-physics-check.exe target/weapon-reference target/mine-tick-fault-check --mine-tick
```

## Mine contact and vehicle effects

`scene_mines::contact` implements selected contact method 122 on the shared
scene and current vehicle views. It updates arming first. An unarmed contact
returns before requiring explosion resources or a nonzero work limit, including
the frame-wrap and one-frame-gap cases. An armed contact reads the current
collider origin, then calls `scene_explosions::spawn_mine` before applying impulse.

The explosion attaches to the current vehicle and stores its incoming Z velocity
shifted arithmetically into velocity X; the zero mine normal leaves local
attachment zero. Normal mines select ring kind 1 and electric mines select kind 3.
The comparison retains the full unsigned mine-kind word. Shared ring eviction
clears the previous owner's reference before publishing the new ring and hiding
the main sprite. Missing pools/materials and invalid evicted owners retain the
already-created object, RNG and attachment writes.

Protection below one permits the original point impulse. Missing external basis
returns after angular and damping writes, before camera, electric state and mine
deactivation. A successful player impulse publishes the 600/1500 camera impulse.
Electric contact starts the same eight-integer surface effect used by ground
sampling: timer 6000, signed-i16 restoration of its current pose, and activation.
The shield tails are a caller-owned nine-u32 array. The player uses bank 1;
other vehicles use bank 0, clear the first matching live peer in racer-slot
order, then publish their own entity. A previous tail matching the current
vehicle clears that vehicle's activation. Peer records borrow live effect views.

Finally the mine's actual Deactivate interface runs on the shared memberships
and queues. A late failure retains the earlier explosion, vehicle and camera
writes and the failing dispatch slot. Audio is an external successful operation.
The contact descriptor owns no persistent vehicle or effect copies; its only
position scratch is twelve bytes while nested constructors remap component pages.

The `scene-mine-explosion` and `scene-mine-hit` instruction qualifiers use real
race resources and complete scene/pool snapshots. They cover pool exhaustion,
slot replacement, ordinary/electric rings, protection, ordered peers, signed
restoration, arming, late failures and contact followed by explosion ticks.
The latter calls the selected method body; outer collision selection and mixed
whole-world/native scheduling remain separate integration boundaries.

```powershell
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/mine-explosion-check target/weapon-reference 0 scene-mine-explosion
python -B "$tools/scene_allocation_checker_check.py" target/release/tankracer-physics-check.exe target/weapon-reference target/mine-explosion-fault-check --mine-explosion
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check -- target/mine-hit-check target/weapon-reference 0 scene-mine-hit
python -B "$tools/scene_allocation_checker_check.py" target/release/tankracer-physics-check.exe target/weapon-reference target/mine-hit-fault-check --mine-hit
```
