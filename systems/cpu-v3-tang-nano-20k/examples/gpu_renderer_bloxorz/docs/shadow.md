---
id: bloxorz-ground-shadow
status: implemented
last-verified: 2026-11-03
---

# Ground shadow

The application draws the Web box shadow through an immutable unit-triangle mesh and ordinary public `Model::set_matrices` / `Pass::draw` calls. Its CPU geometry consumes the already prepared player world matrix; floor, switch, well and player rendering use the same session state. The shadow is drawn after receivers and before the player, with depth test LessEqual, depth writes disabled and SrcOver alpha 87/255, color (10,13,18). Its plane is y=393/65536. The unit mesh has a +Y normal, RGB white and no face culling. Its independent Y matrix column preserves that normal for each nondegenerate X/Z fan.

The CPU clips the eight box corners and twelve edges against y>=0, projects along (.5,1,.5), forms a convex hull, then clips it against each receiving unit cell. The box half-size is .49. Void and inactive bridges do not receive; the start opening excludes its cell during entry, and the goal excludes its cell during drop/land/close. Each cell emits a triangle fan; absolute doubled areas below 2/65536 are omitted. Sutherland–Hodgman derives the previous index from the current index. Near-zero crossing denominators scale both operands before reciprocal, and the interpolation parameter stays within [0,1].

No dynamic mesh upload is required. One Model is reused while each draw publishes its own Instance128 coefficients into the command buffer. Those coefficients remain valid through the normal submission/capture lifetime even after the Model changes for the next triangle. Camera changes continue through the existing renderer preparation cache.

## CPU geometry lifetime

The application caches up to 10 world-space triangle affine records, each six F16 coefficients. Exact world matrix coefficients, level generation, bridge mask and the start/goal receiver flags form its key. Level cells are immutable between `level_reset` calls; every reset advances that generation. Projection direction, half-size, shadow height and material are program constants. A hidden player invalidates the cache. Changed animated transforms rebuild it; a cache hit still publishes ordinary native Instances with the current Camera. Hull, receiving-cell and triangle counts share `SHADOW_INFO`; there is no second cached count.

A pose with more than 10 triangles streams all its draws and disables reuse for that pose. This preserves the geometry envelope without enlarging the static cache. Integer resting first-stage/second-stage poses need at most ten triangles before receiver exclusion. The scratch buffers hold eight 3D corners, twenty projected-candidate slots, a twenty-one-point hull work chain and two twenty-four-point clipping polygons. The triangle cache reuses the dead corner/projected-candidate slots after hull construction: four records in corners and six in candidates. Hull and clipping storage remain live and independent. Scratch, key, validity and counts occupy 435u16; the level generation adds one word. Native descriptor arenas remain caller-owned and retain their normal preflight capacity checks.

For an X/Z-axis rotation of the .98×(1,2,1) box, either projected span is below three world units. At most 4×4 cells can intersect the hull. A half-space cut of a box retains at most ten corner/crossing candidates: a nondegenerate cut has at most seven faces and ten vertices, and slightly relaxing the cut preserves the closed-side sign pattern at coincident endpoints. Its two monotone chains need at most twenty working entries; the twenty-one-point hull reserves one extra entry. The application only supplies rigid/cardinal roll and scale transforms and checks this candidate bound and each hull write. Four rectangle cuts give at most fourteen polygon vertices or twelve fan triangles per cell: 192 triangles is a deliberately loose streaming bound, rather than an observed pose. Both twenty-four-point clip arrays retain a check before every write. The larger candidate array also holds six cached triangles after projection dies. Each emitted triangle uses one 128-byte Instance. The existing command and push arenas each hold 32768u16.

## Verification boundary

The independent fixture contains actual Web world matrices and clipped polygons for standing, entry, tipping and falling. Bounded instruction-CPU tests use the normal shadow module and the functional GPU/Display, decode actual DMA/Instance commands, and check each receiver's published area and boundary. Floating triangulation counts are diagnostic: coverage is the contract. Cache verification compares changed world poses, bridges, goal receivers, level reset and hidden/visible states against forced fresh geometry. A separate scoped native CPU test compares complete color/depth/Display against its identical instruction package; it excludes physical GPU/Display contention and scanout latency.

These matrix samples qualify the shadow expression and publication path. Session animation timing and native material/RGB565 differences have their own scope; the shadow tests do not establish complete Web PBR equivalence or board behavior.
