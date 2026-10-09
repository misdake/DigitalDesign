# GPU v2 documentation

Component documents distinguish baseline contracts from later qualified worktree
versions. See the source and qualification boundaries in [Texture](texture.md#source-and-qualification-boundaries)
and [Pixel composition](pixel-system.md#source-and-qualification-boundaries).
Local `gpu-v2-module-status` tracks checkout/branch identity and active work.

| Document | Scope |
| --- | --- |
| [Architecture](architecture.md) | Crate boundary, component ports and model ownership |
| [Functional oracle chain](oracle-chain.md) | Shared Rust mesh-to-framebuffer composition, configurable frontend precision, bounded FIFOs and A/B WebSocket review |
| [Triangle-record transport](geometry-record-transport.md) | Bounded Rust two-slot publication, shared read-return credit, consumer release and normal-path source lease connection |
| [Lighting](lighting.md) | Frozen Floor/free core with prepared power masks, S2.10 normal, Q13 RSQRT TDP storage and scoped standalone qualification |
| [Rust browser review](../web/README.md) | Native Rayon/WebSocket A/B review, interactive lighting schedule workbench, optional historical WASM diagnostic, build and verification |
| [Frontend](frontend.md) | Command processor, scratchpad, vertex transform and bounded Rust composition |
| [Triangle](triangle.md) | Oracle-only clipping, coverage, perspective fields, precision study and workload analysis |
| [Texture](texture.md) | UNORM9 preparation/cache/Color, serial versus overlapping Runtime, and versioned 16-slot qualification |
| [Framebuffer](framebuffer.md) | Materialized ROP oracle, serial/II8 control calendars and real MC test composition; no counted arithmetic or RTL |
| [Pixel composition](pixel-system.md) | Published row/SPSC foundation, two draw banks, pipelined Final, actual Lighting/Sampling and serial ROP/shared-MC qualification |
| [SDRAM memory controller](sdram-memory-controller.md) | GPU-owned 128B read/write transport, vendor test adapters and bounded service/calibration tests |
