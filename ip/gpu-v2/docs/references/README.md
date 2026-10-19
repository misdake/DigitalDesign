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
| [Rendering models](/ip/gpu-v2/docs/references/rendering.md) | Triangle oracle, functional renderer, Sampling/pixel models and recovery leaf |
| [Record ownership](/ip/gpu-v2/docs/references/record-ownership.md) | Two-slot opaque transport, read-return credit and cancellation |
| [Memory models](/ip/gpu-v2/docs/references/memory.md) | GPU burst facade, shared service adapters and explicit service options |

All simulator entry points require finite cycle/step/work bounds. Host vector
sizes, operation counts and fixture intervals do not define hardware ABI,
physical storage capacity or whole-system throughput.
