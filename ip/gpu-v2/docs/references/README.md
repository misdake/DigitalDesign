# Independent reference and component APIs

These implemented APIs provide numerical answers, bounded execution models or
reusable ownership controllers. Select their constructor and profile explicitly.
The standalone GPU specification is [one level above](/ip/gpu-v2/docs/README.md).

## Choose the intended contract

| Topic | Selected standalone GPU | Independent API described here |
| --- | --- | --- |
| Submission | Descriptor/start, two fixed input pages | Typed DMA/WAIT/DRAW/RELEASE/FENCE frontend commands |
| Vertex payload/output | V96, retained V5 | PackedVertex v6 and seven 36-bit serial Vertex rows |
| Geometry | Three-component transform, no near clipping, analytic final LOD | XYW clipping/fans, configurable near/guard, helper derivatives |
| Color | Per-tap half-up shift9, bounded pair/accumulator lanes | Full products, one exact nearest division by 511 |
| Final | Separate diffuse/specular truncation | RNE of the combined diffuse/specular sum |
| Lighting power | Direct h9,544 used 9-bit base / 7-bit delta entries | Ordinary886-entry 16-bit base / 12-bit delta alternative and explicit calendars |
| Framebuffer | Q6 alpha, eight resident 16x8 TDP half-tiles, explicit maintenance | Ideal alpha/255 oracle; serial/overlapped model calendars |
| Memory | Three halfword-addressed masters | Byte-addressed burst facade and vendor-service adapters |
| Timing | Enabled compute edges plus independent wall maintenance | Functional/count/cycle/emu contracts selected per API |

The integer geometry comparison in the integration example targets the selected
arithmetic while translating its own comparison records. That does not make
all host oracles use selected V5/T16 formats. Numerical agreement, cycle-model
agreement, RTL behavior, physical timing and board operation remain separate
verification claims.

## API documents

| Document | Purpose |
| --- | --- |
| [Frontend models](/ip/gpu-v2/docs/references/frontend.md) | Typed commands, scratchpad, Vertex and owned source capture |
| [Rendering models](/ip/gpu-v2/docs/references/rendering.md) | Independent Triangle/ROP oracles, Sampling/pixel models and recovery leaf |
| [Record ownership](/ip/gpu-v2/docs/references/record-ownership.md) | Two-slot opaque transport, read-return credit and cancellation |
| [Memory models](/ip/gpu-v2/docs/references/memory.md) | GPU burst facade, shared service adapters and explicit service options |
| [Command transport cycle model](/ip/gpu-v2/docs/references/cycle-transport.md) | Production DMA/router wall-edge ownership and native-memory differential fixtures |
| [Command-control cycle subset](/ip/gpu-v2/docs/references/cycle-command.md) | Device4 submission, synchronous command fetch, fence/flush/END and bounded subset qualification |
| [Input and Vertex cycle subset](/ip/gpu-v2/docs/references/cycle-vertex.md) | Production input pages, metadata, bounds/V-mode, old-state storage and explicit T boundary |
| [Indexed T16 cycle extension](/ip/gpu-v2/docs/references/cycle-triangle.md) | Shared production T32/index, sixteen-slot ownership, epoch lifetime and synchronous T16 export |
| [Installed-state and DRAW control](/ip/gpu-v2/docs/references/cycle-draw-control.md) | Explicit CP object/DMA/DRAW control; controlled-peer qualification, with the rendering pipeline still external |
| [Complete command GPU composition](/ip/gpu-v2/docs/references/cycle-auxiliary.md) | CLEAR bypass, UI source-Q/DMA ownership, raw MEMCPY and full command/native-memory qualification |
| [Preparation and interpolation cycle models](/ip/gpu-v2/docs/references/cycle-draw-interpolation.md) | Private preparation, context replay and actual four-phase compute/LOD; Raster and pool remain external |
| [Recursive Raster and producer pool](/ip/gpu-v2/docs/references/cycle-draw-producer.md) | Physical walker/scanner, authoritative 32-quad pool and live indexed frontend composition; pixel consumers remain external |
| [Selected Final and framebuffer TDP](/ip/gpu-v2/docs/references/cycle-draw-pixel.md) | Two-credit/shared arithmetic, four-bank maintenance and installed CP/DMA RGB composition, including native-MC qualification; Sampling remains external |
| [Selected live-global Lighting](/ip/gpu-v2/docs/references/cycle-draw-lighting.md) | II4 scheduled H9 arithmetic, pool tuple loans, live globals and actual CP/DMA RGB/lit composition; Sampling remains external |
| [Command Sampling cycle owners](/ip/gpu-v2/docs/references/cycle-draw-sampling.md) | Source-Q C6, direct weights and Member/Work16/Packet/native Group48; texture/weight banks and Color are external |

All simulator entry points require finite cycle/step/work bounds. Host vector
sizes, operation counts and fixture intervals do not define hardware ABI,
physical storage capacity or whole-system throughput.
