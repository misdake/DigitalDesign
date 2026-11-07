# CPU V3 application host tools

`cpu-v3-app` lists application-owned descriptions and explicitly builds a fresh
snapshot through the System exporter. Its library shares the catalog and
preparation mechanism with the authoritative build script, plus the fresh-output
policy with the materializer. Normal dependencies are the pure boot format and
JSON support; there is no dependency back on the complete System or RCC.

See the [application workflow](/systems/cpu-v3-tang-nano-20k/docs/application-workflow.md).
`cpu-v3-pack` belongs to the separate std-only `boot-format` package.
