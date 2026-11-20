# Explicit GPU API test fixtures

These RCC callers exercise the compiler and GPU API with the shared native
asset producer at `applications/rendering/tools/minimal_assets.py`.

| Fixture | Coverage | Test |
| --- | --- | --- |
| `rcc/main.rs` | Textured, lit triangle and OBS overlay, command-buffer ownership, GPU/Display retirement, static and stack budgets in both GPU check profiles. | `host`, filter `minimal_application_uses_actual_packed_assets_and_public_renderer_calls` |
| `rcc/members.rs` | Two draws snapshot a mutable Model, then three frames present in B/A/B order. | `graphics_member`, filter `actual_minimal_member_demo_runs_three_frames` |
| `rcc/changes.rs` | Eleven frames exercise Camera, Model, Material, lighting and UI edits, including snapshot semantics inside a live Pass. | `host`, filter `actual_compiled_changes_run_on_the_production_core` |

Tests compile these sources directly. Controlled completion peers exercise
software ownership and command data. Production pixel and scanout tests have
separate entry points.
