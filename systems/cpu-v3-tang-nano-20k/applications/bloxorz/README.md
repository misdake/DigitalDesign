# Shared Bloxorz application data

This directory owns the integer rule engine, imported reference cases, reusable
geometry and deterministic asset producers used by both the production
`gpu_renderer_bloxorz` application and the bounded `bloxorz` reference harness.
The renderer owns live input, animation, scene recording, shadows and presentation;
the harness owns independent state/geometry replay and legacy GPU fixtures.

The shared specifications are indexed in [docs](/systems/cpu-v3-tang-nano-20k/applications/bloxorz/docs/README.md).
Generated assets stay in Cargo OUT_DIR or target; imported reference data remains
tracked input. Refresh tools require an explicit reference source.
