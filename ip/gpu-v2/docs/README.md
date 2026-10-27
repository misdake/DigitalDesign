# GPU v2 specification

GPU v2 transforms packed meshlets into a materialized RGB565/D16 framebuffer.
These documents describe the implemented standalone GPU and explain how its
storage and arithmetic realize that behavior. Start with the work flowing
through the GPU; use the later chapters to look up encodings and schedules.

## Read the GPU from input to output

| Chapter | Question answered |
| --- | --- |
| [Overview](/ip/gpu-v2/docs/overview.md) | What does the GPU do, and what owns each part of a render? |
| [Submitting work](/ip/gpu-v2/docs/submission.md) | How do I configure, load, start, drain and flush one meshlet? |
| [Command streams](/ip/gpu-v2/docs/commands.md) | How do explicit DMA, meshlet draws, CLEAR and state changes execute? |
| [Fixed-grid UI](/ip/gpu-v2/docs/ui.md) | How do 8x16 chars draw through shared DMA and framebuffer storage? |
| [DRAM commands](/ip/gpu-v2/docs/commands.md) | How does a prewritten command buffer execute a complete standalone frame? |
| [Data formats](/ip/gpu-v2/docs/formats.md) | How are vertices, indices, matrices, metadata and texture assets encoded? |
| [Geometry](/ip/gpu-v2/docs/geometry.md) | How do transformed vertices become covered, interpolated fragments? |
| [Fragment arithmetic](/ip/gpu-v2/docs/fragment-math.md) | How do normal, UV or RGB become the final RGBA value? |
| [Execution and storage](/ip/gpu-v2/docs/execution.md) | How are partial results held, scheduled and retired under stalls? |
| [Sampling throughput](/ip/gpu-v2/docs/sampling-throughput.md) | Which local rates, latency and tile/mip costs determine the pixel budget? |
| [Framebuffer](/ip/gpu-v2/docs/framebuffer.md) | How do fragments update color/depth and reach external memory? |
| [Memory interface](/ip/gpu-v2/docs/memory.md) | How do the three masters transfer and terminate transactions? |

For first use, read Overview and Submitting work, then consult the format tables
needed for your input. For numerical changes, follow Geometry, Fragment
arithmetic and Framebuffer in that order. For control or storage changes, read
Execution and storage together with Memory interface.

## Independent component APIs

The crate also implements numerical oracles, serial/cycle models and reusable
control components. Their [own index and difference table](/ip/gpu-v2/docs/references/README.md)
identify their inputs and arithmetic. They are separate compositions; choose
an API explicitly before using its formats or output as a comparison.

Specifications belong here after implementation and verification. Pending
top-level, module and software designs remain in the local project design tree.
Each directory maintains its own document index. Development notes belong in
the project daily/weekly journal.
