# Experiment consolidation

The October 2026 cleanup reviewed the main checkout, both associated worker
checkouts, and external geometry backups. Generated Cargo output, obsolete
candidate archives and copied checkout snapshots are disposable after their
maintained source, tests and useful conclusions have been retained. The external
frontend rewrite and original device/display/multiplier/HDMI/audio reference
materials are outside the deletion set.

## Retained implementations

The current [II4 post-interpolator factory](postquad-performance.md) retains
current Context guards and exact coefficient ownership. Its closed Lighting
core uses private compute CE while public acknowledgments and result writes
remain on common commit CE. The closed parent proves compute CE implies actual
core output readiness. Generic Lighting exports retain their backpressure logic.

Useful worker probes now live in `examples/`: H9 numerical replay, physical
texture/weight order, weight-store codec/lifetime, stream acceptance/commit
validation, half-up filter and two-texture-slot export. Empty/unknown active
stream inputs are rejected; inactive payloads need not be decoded. The weight
oracle rejects freeing an ID on the same edge as a new paid read.

The source-record, command payload, explicit Vertex references and three-field
recovery/numerical studies keep their independently qualified component APIs.
Their limits are described in [frontend](frontend.md) and [triangle](triangle.md).
Obsolete Vertex80/XYW/parallel/stored-chain factories remain retired.

CPU D-cache final-response storage, ProductPath address cursors and shared-wide
arithmetic were already adopted in the main history. Their old branches are
not merged again. Direct SDRAM-event capture is rejected: fewer FF did not
offset increased Logic and new related-clock setup failures.

## Reproduction and historical boundaries

The [complete II4 replay and matched fitter](../examples/postquad_ii4/README.md)
are independent of deleted worker/experiment paths. The
[immutable geometry references](../examples/geometry_reference/README.md)
retain only two selected standalone HDL sources and five bounded cases.
They do not select a geometry implementation or change canonical precision.

Previous `target/` report paths in historical discussions are retired evidence
identities, not runtime dependencies. The local project records retain the
per-directory verdict catalog, selected PnR/performance information and source
identities. Rejected alternatives include no-gain controller rewrites,
FF-only storage trades, failed global-enable timing repairs and unaccepted
precision reductions. A leaf fit or coexistence observer never establishes
complete GPU integration or a board result.
