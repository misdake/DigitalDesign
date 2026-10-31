# Retired renderer regression sources

These byte-preserved RCC modules retain the previous Renderer/Frame scheduler, scene registry, UI mirrors/pins and capture-v1 contract
only for existing regression tests. They are not production application modules.
`tests/support/legacy_renderer_loader.rs` composes them with current common
modules under `target/legacy-renderer-tests`; its exports exist only with
`cfg(test)`. Custom test roots retain their own file identities and overrides.
The explicit API compiler never selects this overlay. Production boot projects
accept `gpu-api`; the retired `renderer` directive rejects.

The native single-CB library remains in production rcc/ for gpu_demo,
dualshock2_ui and its direct diagnostics. Its driver and encoder state have real
consumers; they are not the retired two-input resource scheduler.

The existing host, capture-layout/draw and native diagnostic assertions remain
active. Normal explicit-source diagnostics and scene callers use `rcc/gpu_api.rs`.
