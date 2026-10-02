# GPU v2 documentation

| Document | Scope |
| --- | --- |
| [Architecture](architecture.md) | Crate boundary, component ports and model ownership |
| [Lighting](lighting.md) | Oracle/counting/timing, cycle emu/RTL, resource/system alternatives, fitted area and numerical tradeoffs |
| [Lighting WASM review](../web/README.md) | Real Rust browser comparison, deterministic scene controls, build and verification |
| [Frontend](frontend.md) | Command processor, scratchpad, vertex transform and bounded Rust composition |
| [Triangle](triangle.md) | Oracle-only clipping, coverage, perspective fields, precision study and workload analysis |
| [Texture](texture.md) | UNORM9 oracle/counted; universal periodic preparation, bounded cache/color and cycle-MC timed composition |
| [Framebuffer](framebuffer.md) | Materialized ROP oracle, serial/II8 control calendars and real MC test composition; no counted arithmetic or RTL |
| [SDRAM memory controller](sdram-memory-controller.md) | GPU-owned 128B read/write transport, vendor test adapters and bounded service/calibration tests |
