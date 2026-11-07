# Authoritative application image build

This support package owns the sole application `build.rs`, its generated
`OUT_DIR` artifacts, and `cpu-v3-boot-assets`. The small library exposes those
same exact assets for explicit producer qualification. It does not depend on the
complete System composition. Model binaries have no dependency on this package;
the System's dev dependency supplies board/application qualification fixtures.

Normal users select a recipe through the [application command](/systems/cpu-v3-tang-nano-20k/docs/application-workflow.md).
The build script compiles RCC once and packs through the shared pure format.
Materialization and offline contract tests belong here. Their Python reader
qualification is kept beside the Rust producer contract under `tests/`.
