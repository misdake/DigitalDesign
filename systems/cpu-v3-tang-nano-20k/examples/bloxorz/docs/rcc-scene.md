# RCC scene preparation

`rcc/bloxorz_scene.rs` compiles to CPU V3 instructions and uses the system's
existing `gpu_transform` helpers and Q16.16 vector/special-function operations.
It prepares camera/whole-block poses and the native Instance matrix prefix.
It does not submit a Pass or animate an input session.

## Interfaces and bounds

| Function | Contract |
| --- | --- |
| `blox_camera(width, depth, pose, view)` | Fit the reference 400×240 camera. Write an 18-word rigid camera Pose and a 24-word view matrix. Width is 1..15; depth is 1..10. Cache these per level. |
| `blox_roll_pose(out, level, state, dir, fraction, drop)` | Read the level header's width/depth and core state's X/Z/orientation. Write an 18-word model Pose. Whole-block orientations 0..2 and directions 0..3 only. |
| `blox_instance(out, pose, camera)` | Produce the 42-word native matrix prefix: twelve Q16.16 position values followed by nine normal values, low half first. |

`fraction` is a Q16.16 angular fraction in [0,1]. Timing/easing remains the
caller's responsibility. `drop` is [-3,0], used to place a pose below its normal
height. Anchors admit X in [-2,width+1] and Z in [-2,depth+1] to cover off-board
rolls. The state argument uses the existing ten-word core state; a nonzero split
flag is rejected. Array storage must meet the stated sizes; these are
caller-owned small buffers.

Invalid dimensions/roll inputs return false before changing their outputs.
`blox_instance` prepares into fixed scratch and copies only on success, so a
rejected transform leaves all 42 destination words unchanged. It inherits the
shared helper's coefficient/translation bounds rather than weakening them.

## Coordinate scaling and normals

The original 20-degree lens requires X/Y magnification of about 2.658 relative
to the GPU's fixed focal length. Combining that with the long block axis at
scale 2 would exceed the affine helper's coefficient limit of 4.

This example therefore uses **twice the original local mesh coordinates** and
half the original instance scale. For the rendered box, source corners are
approximately +/-0.98 rather than +/-0.49. Whole-block scales are permutations
of (0.5,1,0.5), within the shared helper's admitted [0.5,2]. This changes no world
position and requires no GPU logic or generic helper change. Any native asset
used with these poses must honor this coordinate convention.

`gpu_instance_trs` computes rigid Camera*Model position and the corresponding
inverse-scale normal matrix. A second `gpu_affine_compose` applies the lens to
the **position prefix only**. Normals remain independent of lens magnification.
The camera basis retains the original yaw/pitch and changes world Y-up to GPU
Y-down/forward-Z. Fixed camera constants replace runtime trigonometric divisions;
the shared helper owns trigonometric/reciprocal implementation.

The camera fits eight board corners at world heights zero and two, including
the original 3% margin. Game X/Z anchors are translated by half the board size
before camera transformation. `drop` and intermediate rolling poses can extend
outside that initial framing; their validity is not a claim that every such
pose remains on screen.

## Numeric validation boundary

`rcc_geometry.rs` compiles the actual RCC module with the GPU transform helpers, runs it
on CPU V3 with a two-million-instruction bound per case, and compares its output
to imported independent Three.js matrices. It covers 180 poses, three camera
sizes and three anchors (1,620 executions), plus rejected-input/output-retention
checks. All cases use default CPU V3 compiler optimization settings.

The source-coordinate candidate evaluated by the test is origin=-64256,
quant_shift=7 and delta10=0/1004, giving symmetric Q16.16 corners +/-0.98046875.
The test applies CPU-produced matrices through the documented signed native MAC
and unshifted F16 calculation. It checks forward positive Z and the signed26
range, then compares projected corner locations against the unquantized
reference. Regression limits are 0.004 for matrix elements, 0.0002 for normal
elements and 1/8 pixel for these sampled corners. Axis-aligned source normals at
both signed normal10 extremes are also checked against the normal18 range.

This is a source-format and arithmetic-boundary check, not a packed mesh upload
or GPU raster test. Interior pixel coverage, depth interpolation, alpha and
lighting remain outside its evidence. Instruction counts include camera setup,
test output copying and bounds checking; they are not frame-cycle predictions.
The host clipped-shadow reference remains separate from this RCC module.
