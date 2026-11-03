# Installed-state and DRAW command control

`command::Processor::with_draws()` and `command::Control::with_draws()` add the
production command processor's DMA, installed-state and meshlet launch registers.
Default constructors keep the existing NOP/WAIT/FLUSH/END subset and explicit
unsupported-opcode boundary. Neither constructor supplies Raster, interpolation,
pixel processing or a framebuffer. RAM and render/flush ports remain caller-owned.

## Commands and source ports

The explicit constructor supports opcodes 0,1,2,3,4,5,6,7,9,A,F using the
[production encoding](/ip/gpu-v2/docs/commands.md). CLEAR, UI and MEMCPY remain
unsupported, and D/E are invalid. DMA proposals feed the existing command DMA;
the control composition forwards actual begin, header, completion and uniform
events into the processor. Texture and framebuffer ports feed the existing
command router. These ports do not execute an unimplemented backend.

State objects use the original synchronous CP source port. APPLY_R requests
`{block7,object_index2}` and advances only with source readiness and CE. APPLY_W
consumes the held old Q on the next enabled edge. Global, Render and Target read
three, two and one 64-bit rows respectively. Fields update on their individual
consume edges; only the final row sets the object's valid bit. A later invalid
row does not define rollback of earlier writes.

DRAW checks the installed bindings on EXEC, waits for its published input page
and legal DMA exclusion, then reads the selected uniform's qword11 for RGBA.
COLOR_W rejects a nonzero high32. DRAW_START holds its page/start offer until
an enabled meshlet-ready edge, clears stored/all-fence permission and waits for
meshlet_done. That completion permits the next command without implying that
indexed exports, R/I or pixels drained.

## Installed state and completion

`Bindings` exposes the sole live CP configuration fields, including the final
factory's fixed-framebuffer texture-view bit. Target fields retain 512-byte units
and 64-KiB alignment. The CP checks its actual 400-unit plane extents; driver
allocation and alias protection remain separate. Ordinary POT descriptor fields
retain the trusted command policy; fixed views retain the stricter final-factory
checks. The interface does not add an independent texture slot table or alpha.

DMA completion and header events are wall-owned, including CE0. New submission
clears binding/page/uniform validity while retaining the field payload as in RTL.
Reset clears the control state but preserves the constructor's explicit scope.
First SET_TARGET requires stored permission, so the caller performs a successful
render wait and an actual empty-cache flush before initial target installation.
FLUSH only sets stored on an enabled observation of flush_complete. ENDING still
waits for render, DMA and router drain on a wall edge before clearing BUSY.

All outputs observe old registers. `clock` commits one edge without advancing
RAM or memory service. Invalid commands return the original contract at EXEC;
bad object/mesh-color data return it at APPLY_W/COLOR_W. These are simulation
monitor boundaries, not added backpressure or recovery gates. Defined legal
prefix effects remain real; behavior after a rejected edge is unspecified.

## Qualification boundary

`vliw_cycle_draw` checks two submissions, POT/fixed-view fields, synchronous
source-Q and CE/readiness stalls. Its first fixture uses explicitly controlled
DMA, source-RAM, meshlet/render and flush peers. It does not qualify a native-MC
DRAW, real cache flush latency, a rendered pixel, whole-GPU completion or C4.
The ignored differential entry is explicitly runnable; it exports both complete
command profiles and elaborates them with the vendor primitive/GSR binding,
then compares the selected final CP module on every sampled edge. Actual CP
mutations must diverge and invalid inputs must hit the precise monitor.

Each fixture has a 100000-edge bound. Compile and RTL processes have 30-second
limits and an independent testbench watchdog. Unique checkpoint directories
retain source/profile/port/ROM inventories, vectors, raw logs and verdicts.
Elaboration of a full export is not execution of a complete DRAW.
