# GPU v2 documentation

| Document | Scope |
| --- | --- |
| [Architecture](architecture.md) | Crate boundary, component ports and model ownership |
| [Lighting](lighting.md) | Implemented Rust oracle, counted work, bounded timed reservations and data layout |
| [Frontend](frontend.md) | Command processor, scratchpad, vertex transform and bounded Rust composition |
| [Triangle](triangle.md) | Oracle-only clipping, coverage, perspective fields, precision study and workload analysis |
| [Texture](texture.md) | Oracle-only mip/LOD/Group4/filter precision study and functional cache using the SDRAM service |
| [SDRAM memory controller](sdram-memory-controller.md) | Vendor combination, GPU Rust oracle adapter and bounded service/calibration tests |
