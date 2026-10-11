# Quad output protocol draft

Status: draft, 2026-10-10. This is the proposed rasterizer/interpolator output
contract derived from the actual post-quad composition. Live raster ingress
and render/display ownership still require separate integration acceptance.

Scope: `gpu_v2_postquad`. The top has one immutable draw bank during
an in-flight epoch. This draft describes the actual source boundary, not a
parallel quad API or a proposed multi-draw interface.

1. **Eight accepted rows per quad.** Offer `in_valid`, `in_begin[22:0]`,
   `in_basic[31:0]`, `in_light[35:0]` and `in_uv[35:0]`. The top supplies `in_row[2:0]` and
   `in_ready`. Advance the producer only on `in_valid && in_ready`; hold the
   entire offered row through stalls. Do not use elapsed clocks or branch
   source ACKs to advance the global quad sequence.

2. **Begin encoding.** On row0, provide mask[3:0], draw-bank bit4=0,
   unlit5, untextured6, force-coarsest7 and origin15[22:8]. Origin is
   `{Y/2[6:0], X/2[7:0]}`; X/Y are even, X<=510 and Y<=254 for the512x256
   target. The descriptor must stay stable throughout the eight offered rows.
   The consumer captures row0 metadata; later rows do not start another quad.

3. **Basic rows.** Lane order is 0=(x,y),1=(x+1,y),2=(x,y+1),3=(x+1,y+1).
   Even row2*lane contains little-endian R[7:0],G[15:8],B[23:16]; set upper8
   bits to zero. Odd row2*lane+1 contains unsigned depth16, with upper16 zero.
   The depth upper-bit guard is an actual ingress fault check.

4. **Lighting rows.** Even rows contain signed normal X/Y/Z as three12-bit
   S12F10 values. Odd rows contain signed NDC X/Y as two16-bit S16F14 values,
   with upper4 bits zero. Supply legal, finite quantized attributes for every
   lane; the coverage mask suppresses uncovered arithmetic. The producer does
   not normalize or supply computed g/h. Unlit uses the actual branch bypass.

5. **Sampling rows and helpers.** Odd rows contain one pair of unwrapped
   signed18-bit S18F16 UV values: U[17:0],V[35:18]. All four helper pairs are
   required even when a lane is uncovered, because D uses the full2x2 quad.
   Quantize once at the raster boundary. Do not wrap UV before derivatives.
   The producer handles out-of-domain helpers using the established force-
   coarsest policy; it must not silently clamp a normal derivative. Even-row
   UV is unused and should be zero. Untextured skips Sampling ownership.

6. **Coverage and publication.** All eight rows must be offered even for a
   zero mask. Zero-mask quads consume no published global/result owner. A
   nonempty quad publishes on accepted row7. The global ring has32 slots;
   local source queues have Light2 entries and Sample8 entries. Credits and
   branch bypass choices are checked at row0 using old state. A partial quad
   holds its reserved tail until completion; continuation rows do not request
   another global allocation. The producer supplies no guessed completion.

7. **Draw/configuration lifetime.** Keep Sampling config27, texture slots,
   Lighting uniforms/epoch/mode/shininess, specularRGB, alpha and depth/blend
   pins immutable until `pipeline_idle`. Sampling config27 packs signed
   bias[15:0], texture slot[19:16], maxN[23:20], full-mip24 and filter[26:25].
   For a draw change, stop source admission, drain the current pipeline, then
   change immutable pins and request the explicit `draw_rebind` event while
   `pipeline_idle`. `pipeline_idle` permits dirty framebuffer cache contents;
   it is not a memory-visible flush certificate. The current top uses bank0.
   Final uses RGB8 tint/texture/specular and g/h in unsigned Q8 with endpoints
   0..256; alpha is a draw-wide unsigned byte, not a separate per-lane field.
   Depth function encoding is 0 NEVER, 1 LESS, 2 EQUAL, 3 LESS_EQUAL,
   4 GREATER, 5 NOT_EQUAL, 6 GREATER_EQUAL, 7 ALWAYS. Blend is 0 REPLACE,
   1 SRC_OVER. The cache captures depth/blend context with its input tail;
   pending ROP work keeps that captured context after global retirement.
   Texture asset mutation/cache invalidation and render/display ownership
   are additional command-layer contracts, outside this single-draw draft.

8. **CE and wall-clock returns.** Source acceptance and compute consumption
   share the actual joined commit/CE. With CE0, offered sources and arithmetic
   owners remain stable. Already paid synchronous RAM reads still capture on
   the wall clock into their reserved destinations. External memory is also
   wall-clock owned; do not pause read beats, terminal responses or write ACKs
   merely because compute CE is zero.

9. **Actual result ownership.** Lighting and Sampling publish actual numeric
   results into their result banks. Actual last-covered result consumption
   establishes branch completion. Final and the framebuffer sink produce
   eight real color/depth rows. Only accepted sink row7 retires the global
   slot; publication, source dequeue, Final issue and framebuffer commit are
   distinct events. Public Sampling IDs16 remain distinct from Context8 and
   the global32 ring; same-edge final RGB consumption cannot fund alias reuse.

10. **Shared external memory and final visibility.** The top arbitrates one
    outstanding128-byte request between texture and framebuffer traffic.
    Hold each presented request until `mem_req_ready`; preserve its locked
    owner through all16 little-endian64-bit beats and the terminal response.
    Respect write-ready backpressure and report terminal success/failure.
    Request flush after source/pipeline drain. Completion requires actual
    framebuffer `flush_complete`, empty pipeline/sink and no outstanding
    memory owner. A global retirement or dirty-cache commit alone does not
    prove the final external framebuffer image is visible.

Acceptance should include sparse/zero coverage, signed UV wrap helpers,
global32/public16 wrap, independent branch bypasses, Final backpressure, CE0
paid returns, request/write stalls, terminal failure, and bounded live reset.
Every run has a finite maximum wall-clock count. Throughput measurements use
separate clean resident/cold profiles; adversarial qualification is reported
separately rather than folded into an alleged arithmetic II.

The selected implementation is `postquad::verilog_selected`, with Public16,
Context8, C6 and Work16 retained. Its ROM qualification, complete-chain fits
and remaining whole-GPU boundary are linked from [the performance report](postquad-performance.md).
