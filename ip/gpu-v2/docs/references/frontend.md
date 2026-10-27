# Typed frontend and source models

## Commands and local storage

The frontend model accepts semantic DMA, WAIT-token, DRAW-vertex-range,
consumer RELEASE and frontend FENCE commands. They are typed host inputs;
there is no encoded CPU command-buffer parser. Unsupported commands fault.

DMA address/count are 8-byte aligned, with nonzero length at most 4096 bytes,
token 0..3, no overflow and no4-KiB-region crossing. One active DMA owns one of
two regions; bounded descriptor admission reserves free regions. Four 1024 × 16
scratchpad banks use DMA port0 and core port1, with collisions forbidden.

Regions move `FREE -> FILLING -> READY -> IN_USE -> FREE` with generation leases.
Completion publishes after the final write. DMA receive/error drain advances on
wall edges while core micro-operations pause under CE.

Event identities include DMA0..3, COMMAND, TRIANGLE_CREDIT, CACHE_DONE and FAULT.
Dispatch occurs at idle/WAIT boundaries. Handler ACK and WAIT token consumption
are distinct; same-edge new events beat ACK. Frontend FENCE covers accepted DMA
and Vertex publication. Backend completion and display visibility have other
owners.

## Vertex results

PackedVertex v6 has its own quantized grid/normal/UV/tint encoding. Serial Vertex
variants return seven 36-bit rows: clip XYZW, N36, UV UNORM12 and RGB565. Ready
publication follows all rows; the producer must be inactive before slot release
permits reuse. Explicit emitter/emu choice determines the numerical pipeline.
These retained APIs have independent ports from the selected GPU's V5 records.

## Owned source capture

SourceCapture obtains complete owned snapshots from published vertices. Seal
and reference rules distinguish raw producer release, physical slot reuse and
snapshot-ticket consumption. The ordered source-record component accepts a
header and sequential rows into bounded RAM, publishes only complete records,
and holds synchronous return ownership under pause.

The [record controller](/ip/gpu-v2/docs/references/record-ownership.md) transports opaque fan records and
returns last-use/release feedback. Neither controller performs Triangle setup.
Their source ownership can be reused without treating its row layout as a
numerical hardware ABI.

Implementation anchors: [command ports](/ip/gpu-v2/src/frontend/ports.rs),
[scratchpad](/ip/gpu-v2/src/scratchpad/mod.rs), [Vertex](/ip/gpu-v2/src/vertex/mod.rs),
[capture](/ip/gpu-v2/src/frontend/source_capture.rs),
[source record](/ip/gpu-v2/src/frontend/source_record.rs).
