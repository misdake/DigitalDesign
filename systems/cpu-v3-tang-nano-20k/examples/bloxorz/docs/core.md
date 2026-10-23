# Integer game core and reference replay

The application core is RCC source compiled for CPU V3. Game coordinates use
X right, Z toward the viewer and Y up; camera conversion belongs to rendering.
Rules do not depend on frames, GPU state, timers or floating-point arithmetic.

## Level format

Levels are trusted, importer-validated arrays of u16. One level is resident at a
time. The eight-word header is width, height, start X, start Z, start orientation,
initial bridge mask, cell count and schema=1. Width/height are at most32, cell
count at most256. Each row-major cell has eight words:

| Word | Meaning |
| --- | --- |
| 0 | Tile: void0, normal1, fragile2, soft3, heavy4, bridge5, goal6, splitter7 |
| 1 | Bridge channel bit |
| 2,3 | Ordered switch operations compiled to `(mask & keep) ^ xor` |
| 4..7 | Split destinations x1,z1,x2,z2; x1=65535 means no destination |

The keep/xor transform preserves the ordering of stacked on/off/toggle operations
without walking an operation list on CPU. A maximum-size level occupies4112 bytes;
the imported set's largest board occupies2416 bytes. These are ordinary CPU game
data, separate from GPU physical Mesh/Texture allocations.

## Rules and storage

`blox_dispatch` accepts +X0, -X1, +Z2, -Z3, restart4, undo5 and swap6. Normal
moves and swap stop at a terminal state or65535 moves. Invalid commands and
unavailable undo/swap leave state and history unchanged. Restart remains legal.

A move evaluates covered switches, splitter teleport, support and the goal.
Heavy switches and fragile failure require a standing whole block. A split cube
can activate soft switches; swap changes the active cube, costs a move and
reevaluates its tile. Adjacent cubes merge and evaluate the merged footprint
again, including its switch actions, matching the reference engine.

Working state is ten words: X, Z, orientation, bridge mask, phase, move count,
other X/Z, active cube and split flag. Phases are playing0, success1 and fall2.
The six-word snapshot is exactly the original three little-endian u32 values
represented as low/high u16 pairs:

| u32 | Bits |
| --- | --- |
| 0 | signed X16, signed Z16 |
| 1 | orientation2, bridge mask12, phase2, moves16 |
| 2 | signed other X15, signed other Z15, active1, split1 |

The fixed256-entry undo ring uses3072 bytes plus four bytes of head/count.
When full, it discards the oldest snapshot and preserves the latest256 actions.
This is an explicit bound compared with the reference's growing history.
Restart clears history; terminal states can be undone. Trusted level bounds
keep reachable coordinates representable, including a last off-board fall.
Raw forged state/level arrays are not a checked public serialization API.

## Execution and validation boundary

The host example compiles the core once and runs each imported trace on CPU V3.
The test harness preloads the level at physical word0x5000 and commands at0x6000;
the CPU writes accepted flags and packed states from0x8000. These are replay-only
locations, not a production boot layout. Each trace has at most512 commands and
a20-million-step execution bound. Expected states come from the original engine,
including rejected commands, failures, restart, undo and split operations.

Dedicated compiled tests cover an off-board lying block, a wrapped undo ring,
the move-count limit, invalid commands and signed snapshot roundtrips. The
core uses the default CPU V3 compiler optimization settings. Rule replay does
not certify GPU rendering, real input, Display scanning, Flash packaging or board
behavior.
