---
id: gpu-explicit-examples
status: active
last-verified: 2026-11-24
---

# GPU resource API callers

Read the [public GPU guide](/systems/cpu-v3-tang-nano-20k/docs/gpu-api-guide.md)
before the larger callers. Applications own draw order, Camera/Model/Material,
input/time, external working allocations, GPU completion and Display handoff.
The driver owns resource registration, native encoding and device protocols;
it contains no demo asset selection, Bloxorz rules or transparency sorter.

## Which example answers which question?

| Caller | Focus | Command-buffer organization |
| --- | --- | --- |
| [Hello GPU](/systems/cpu-v3-tang-nano-20k/examples/sample_hello_gpu/rcc/main.rs) and [Scene](/systems/cpu-v3-tang-nano-20k/examples/sample_hello_gpu/rcc/scene.rs) | First triangle plus UI; split frame orchestration from owned scene resources. | One CB, reset/record/submit/wait each frame, then swap/confirm. |
| [Showcase](/systems/cpu-v3-tang-nano-20k/examples/demo_showcase/rcc/main.rs) | Geometry, Camera, lighting, material and input pages; CPU framebuffer work has explicit GPU ordering. | One CB; CPU pages also complete a GPU clear before CPU drawing and the UI pass. |
| [RTT](/systems/cpu-v3-tang-nano-20k/examples/demo_render_to_texture/rcc/scene.rs) | Render offscreen, then sample its fixed framebuffer view. | One CB reused for two serial submissions; texture invalidation between producer and consumer. |
| [Billboard](/systems/cpu-v3-tang-nano-20k/examples/demo_billboard/rcc/main.rs) | Vertical trees, camera-plane effects, layer sorting and explicit transparent state. | One per-frame CB; sorting and effect selection belong to the application. |
| [Box/rabbit controller](/systems/cpu-v3-tang-nano-20k/examples/demo_box_rabbit/rcc/public_main.rs) | Moving camera, persistent room/RGB stream, ordinary rabbit/UI and input/timing. | Two CPU slots with fixed roles; prepared room/RGB appended with the current Pass Camera, dynamic CPU recording during static execution, then serial GPU submission. |
| [Bloxorz](/systems/cpu-v3-tang-nano-20k/examples/game_bloxorz/rcc/main.rs) | Whole-stream static replay plus prepared dynamic components, shadows, fades and HUD. | Two CPU slots: static level stream with target-only rebinding, dynamic CB with application-allocated prepared parts. |

Each application preserves a writable Back and confirms presentation before
reusing the former Front. This is application scheduling, not an implicit wait
inside `set_next_swap`. Two CPU CBs allow preparation/execution overlap; the
hardware still executes only one submitted CB at a time.

## Resource generation is application input

Each example's `application.rs` selects its source and asset producer. Producers
write native meshlet/texture/font payloads and descriptions; the build allocates
external addresses and packages the result. They do not record a frame CB.
Start with [Hello's producer](/systems/cpu-v3-tang-nano-20k/examples/sample_hello_gpu/tools/assets.py),
then its [build recipe](/systems/cpu-v3-tang-nano-20k/examples/sample_hello_gpu/application.rs).
The [static asset contract](/systems/cpu-v3-tang-nano-20k/docs/static-assets.md)
explains includes, relocation, alignment and loader-owned bytes.

Hello/Showcase use the public source-asset path. Older RTT/Billboard/Bloxorz
producers retain hand-packed native fields and shared reference helpers. These
are existing implementations, not prerequisites for a new application.
`applications/rendering/rcc/gpu_demo_api.rs` is ordinary application support,
not a driver service. `rcc/gpu_board.rs` constructs fixed board working extents;
new applications may declare their own static buffers instead.

## Larger examples: read only the relevant boundary first

In RTT, the `0xc000` and `0xc800` sector reservations are application placement
choices excluded by its packer, not universal GPU addresses. The producer's
completion precedes sampling, and changed content gets explicit texture-cache
invalidation. The consumer uses Nearest/Bilinear: fixed framebuffer views reject
the generic Trilinear default. The offscreen TextureView is created once and retained across frames; updating
its backing pixels does not require re-registering the view. The mesh is also
registered once: producer/consumer MeshPtr slices share its directory owner.

In Billboard, facing controls expansion; blend/depth-write choices and sorted
draw order remain explicit. Stage/layer selection uses `MeshPtr::slice`, without
per-frame arena allocations or directory copies. The package stores 199 updates
as compact sprite parameters, not baked per-frame meshlets. Nine static meshlets
and ten reusable sprite slots cover the scene. CPU update writes the selected
centers, rotated corners and UVs after prior GPU retirement; material color and
texture selection remain ordinary draw inputs. Paused frames retain the slots.

In Bloxorz, read `refresh_static_scene`, `record_dynamic_scene` and the submission
loop separately. The static CB stores commands and World/material snapshots;
it is not a cached framebuffer of all static geometry. The fixed background
image and offscreen fade snapshot are separate pixel resources. Level/geometry
changes invalidate the stream, while A/B changes retarget it. Prepared parts
store bridges/switches/pads and inherit the enclosing Pass camera/light; their
selection and capacity choices are Bloxorz policy, declared in its PreparedPool.

The controller preserves native scene choices: trilinear texture filtering,
Less with depth write, rabbit shininess S32, and linear-light RGB565 presentation.
Its static room/RGB and dynamic rabbit/UI streams both use ordinary `submit`.
There is no `submit_capturable`, pause gate or board capture producer in the
current public submission API. Application timing includes CPU control/wait
overhead and must not be described as pure GPU execution time.

The related RGBA4444 caller keeps all scene resources in
[RgbaScene](/systems/cpu-v3-tang-nano-20k/examples/demo_box_rabbit/rcc/rgba_scene.rs).
Camera changes and CB recording are per frame; mesh, models, textures, materials
and separate opaque/transparent states persist. Neither resource registration
nor state setters belong in its draw-only recording method.

## Regression fixtures are not application tutorials

The small members/changes fixtures under `tests/fixtures/gpu_renderer/rcc/`
exercise mutable-source snapshots and explicit ownership. Host tests under
`tests/host/compiler/` cover resource construction, privacy, generations, native
recording, prepared allocations, target-only relocation and bounded device peers.
Their completion peers need not render pixels or model physical cache timing.
Historical capture/qualification adapters are separate test infrastructure;
their old loader names and capture protocols do not describe today's public API.

For rendering/performance qualification use the current
[application workflow](/systems/cpu-v3-tang-nano-20k/docs/application-workflow.md),
[profiling guide](/systems/cpu-v3-tang-nano-20k/docs/application-profiling.md) and
[same-build diagnostics](/systems/cpu-v3-tang-nano-20k/docs/application-build-artifact.md).
